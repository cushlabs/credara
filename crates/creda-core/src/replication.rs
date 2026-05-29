//! Replication orchestration — the seam between the engine and the network (spec §6, §7.1).
//!
//! [`Replicator`] is **transport-agnostic**: it is generic over [`NetworkTransport`], so all of
//! its logic — deriving the topic bucket for an event, building and publishing gossip batches,
//! ingesting received batches through the engine's signature gate, and running anti-entropy
//! reconciliation — compiles and is unit-tested in the default build with a mock transport. The
//! real libp2p transport ([`creda_net::Libp2pTransport`], feature `libp2p`) plugs into the exact
//! same interface; the daemon wires the two together (see `grpc.rs`, feature `libp2p`).
//!
//! This mirrors the engine/gRPC split: the domain logic lives here and is verifiable without the
//! heavy dependency; only the adapter is feature-gated and reconciled when its feature is built.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use creda_events::{EventId, EventPayload, IdentityEventNode, IdentityEventType};
use creda_net::bucketing::dht_key_from_demographics;
use creda_net::{bucket_of, reconcile, GossipBatch, NetworkTransport, SeenSet};

use crate::engine::{CredaCore, Ingest, VerifyingKeyResolver};
use crate::error::Result;

/// Counts from ingesting one or more events. `reasons` collects rejection reasons for
/// logging/metrics — they describe why an event was refused, never a reason to trust it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IngestSummary {
    pub accepted: usize,
    /// Already held, or a redelivery suppressed by the dedup set.
    pub duplicates: usize,
    pub rejected: usize,
    pub reasons: Vec<String>,
}

impl IngestSummary {
    fn record(&mut self, outcome: Ingest) {
        match outcome {
            Ingest::Accepted => self.accepted += 1,
            Ingest::AlreadyHave => self.duplicates += 1,
            Ingest::Rejected(reason) => {
                self.rejected += 1;
                self.reasons.push(reason);
            }
        }
    }
}

/// Drives replication for one peer over a chosen [`NetworkTransport`].
///
/// The transport is chosen once at startup (libp2p in production, a loopback/mock in tests), so
/// the replicator owns it by value rather than behind a `dyn` boundary — matching the trait's
/// async-fn-in-trait design, which is intentionally not object-safe.
pub struct Replicator<T: NetworkTransport> {
    core: Arc<CredaCore>,
    transport: T,
    resolver: Arc<dyn VerifyingKeyResolver>,
    seen: Mutex<SeenSet>,
    sequence: AtomicU64,
    local_peer_id: Vec<u8>,
}

impl<T: NetworkTransport> Replicator<T> {
    /// Build a replicator. `dedup_capacity` bounds the gossip de-duplication set (§6.1.4).
    pub fn new(
        core: Arc<CredaCore>,
        transport: T,
        resolver: Arc<dyn VerifyingKeyResolver>,
        dedup_capacity: usize,
    ) -> Self {
        let local_peer_id = transport.local_peer_id();
        Self {
            core,
            transport,
            resolver,
            seen: Mutex::new(SeenSet::new(dedup_capacity)),
            sequence: AtomicU64::new(0),
            local_peer_id,
        }
    }

    /// The underlying transport (for the daemon to drive DHT provide/announce on its own cadence).
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Subscribe to the topic buckets this peer serves (§6.2.4).
    pub async fn subscribe_buckets(&self, buckets: &[u64]) -> Result<()> {
        for &bucket in buckets {
            self.transport.subscribe_bucket(bucket).await?;
        }
        Ok(())
    }

    /// Publish a locally-created event to the gossip bucket of the subgraph it belongs to
    /// (§6.2.2, §6.2.4). Returns the bucket it was published to, or `None` if the event's
    /// subgraph has no primary DHT key yet (no Assert with family+DOB+sex) and so cannot be
    /// routed — the caller can retry once the identifying Assert is present.
    pub async fn publish_event(&self, node: &IdentityEventNode) -> Result<Option<u64>> {
        let Some(bucket) = self.bucket_for_event(node)? else {
            return Ok(None);
        };
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst);
        let batch = GossipBatch::new(self.local_peer_id.clone(), sequence, vec![node.clone()]);
        self.transport.publish_batch(bucket, &batch).await?;
        Ok(Some(bucket))
    }

    /// Ingest a received gossip batch (§6.2.2). Whole-batch and per-event de-duplication run
    /// first (§6.1.4); each surviving event passes through the engine's mandatory signature
    /// gate ([`CredaCore::ingest_event`]). A bad event is counted and skipped, not fatal.
    pub fn ingest_batch(&self, bytes: &[u8]) -> Result<IngestSummary> {
        let batch = GossipBatch::from_bytes(bytes)?;
        let mut summary = IngestSummary::default();

        // Batch-level dedup: if we've already processed this (sender, sequence), drop the whole
        // batch (§6.1.4).
        if self
            .seen
            .lock()
            .expect("dedup mutex")
            .seen_batch(&batch.sender, batch.sequence)
        {
            summary.duplicates += batch.events.len();
            return Ok(summary);
        }

        for node in batch.events {
            // Event-level dedup: ignore a UUID we've recently seen, regardless of batch.
            let already_seen = self
                .seen
                .lock()
                .expect("dedup mutex")
                .seen_event(&node.id);
            if already_seen {
                summary.duplicates += 1;
                continue;
            }
            let outcome = self.core.ingest_event(node, &*self.resolver)?;
            summary.record(outcome);
        }
        Ok(summary)
    }

    /// End-to-end anti-entropy round against `peer` (§6.1.8): fetch the peer's manifest (its
    /// UUID set), reconcile against our local store, fetch what we're missing, and ingest
    /// through the signature gate. This is what the daemon's periodic AE scheduler calls.
    pub async fn run_anti_entropy_round(&self, peer: &[u8]) -> Result<IngestSummary> {
        let remote_ids = self.transport.request_manifest(peer).await?;
        self.anti_entropy(peer, &remote_ids).await
    }

    /// One anti-entropy round against `peer`, given the peer's set of event ids (§6.1.8). We
    /// fetch the events we are missing and ingest them through the same signature gate. Events
    /// the *peer* is missing are served when it runs its own round against us (or answers our
    /// `PeerRequest::Events`), so this method only pulls.
    pub async fn anti_entropy(&self, peer: &[u8], remote_ids: &[EventId]) -> Result<IngestSummary> {
        let local: BTreeSet<EventId> = self.core.all_event_ids()?.into_iter().collect();
        let remote: BTreeSet<EventId> = remote_ids.iter().copied().collect();
        let delta = reconcile(&local, &remote);

        let mut summary = IngestSummary::default();
        if delta.local_missing.is_empty() {
            return Ok(summary);
        }
        let fetched = self.transport.request_events(peer, &delta.local_missing).await?;
        for node in fetched {
            let outcome = self.core.ingest_event(node, &*self.resolver)?;
            summary.record(outcome);
        }
        Ok(summary)
    }

    /// The bucket for the subgraph an event belongs to: materialize the connected subgraph and
    /// derive the primary DHT key from its identifying Assert (§6.1.6, §6.2.4).
    fn bucket_for_event(&self, node: &IdentityEventNode) -> Result<Option<u64>> {
        let subgraph = self.core.get_subgraph(&[node.id])?;
        for assert in subgraph.nodes_of_type(IdentityEventType::Assert) {
            if let EventPayload::Assert { demographics, .. } = &assert.payload {
                if let Some(key) = dht_key_from_demographics(demographics) {
                    return Ok(Some(bucket_of(&key)));
                }
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex as StdMutex;

    use creda_events::{
        AdministrativeGender, CertificateFingerprint, Demographics, SignatureAlgorithm, SigningKey,
        TokenizedDate, TokenizedString, VerifyingKey, VerificationMethod,
    };
    use creda_store::MemoryStore;

    use crate::config::CredaConfig;
    use crate::signer::InMemorySigner;

    // ---- a key map standing in for the participant registry ----
    struct MapResolver(HashMap<Vec<u8>, VerifyingKey>);
    impl VerifyingKeyResolver for MapResolver {
        fn resolve(&self, fp: &CertificateFingerprint) -> Option<VerifyingKey> {
            self.0.get(fp.as_bytes()).cloned()
        }
    }
    fn resolver_for(fp: &CertificateFingerprint, vk: &VerifyingKey) -> Arc<dyn VerifyingKeyResolver> {
        let mut m = HashMap::new();
        m.insert(fp.as_bytes().to_vec(), vk.clone());
        Arc::new(MapResolver(m))
    }
    fn empty_resolver() -> Arc<dyn VerifyingKeyResolver> {
        Arc::new(MapResolver(HashMap::new()))
    }

    // ---- a transport that records publishes and serves canned request_events ----
    #[derive(Clone)]
    struct MockTransport {
        published: Arc<StdMutex<Vec<(u64, GossipBatch)>>>,
        subscribed: Arc<StdMutex<Vec<u64>>>,
        canned: Arc<StdMutex<Vec<IdentityEventNode>>>,
        peer_id: Vec<u8>,
    }
    impl MockTransport {
        fn new(id: &[u8]) -> Self {
            Self {
                published: Arc::new(StdMutex::new(Vec::new())),
                subscribed: Arc::new(StdMutex::new(Vec::new())),
                canned: Arc::new(StdMutex::new(Vec::new())),
                peer_id: id.to_vec(),
            }
        }
        fn set_canned(&self, events: Vec<IdentityEventNode>) {
            *self.canned.lock().unwrap() = events;
        }
        fn published(&self) -> Vec<(u64, GossipBatch)> {
            self.published.lock().unwrap().clone()
        }
    }
    impl NetworkTransport for MockTransport {
        async fn publish_batch(&self, bucket: u64, batch: &GossipBatch) -> creda_net::Result<()> {
            self.published.lock().unwrap().push((bucket, batch.clone()));
            Ok(())
        }
        async fn subscribe_bucket(&self, bucket: u64) -> creda_net::Result<()> {
            self.subscribed.lock().unwrap().push(bucket);
            Ok(())
        }
        async fn unsubscribe_bucket(&self, _bucket: u64) -> creda_net::Result<()> {
            Ok(())
        }
        async fn dht_provide(&self, _key: creda_net::DhtKey) -> creda_net::Result<()> {
            Ok(())
        }
        async fn dht_find_providers(&self, _key: creda_net::DhtKey) -> creda_net::Result<Vec<Vec<u8>>> {
            Ok(Vec::new())
        }
        async fn request_events(
            &self,
            _peer: &[u8],
            ids: &[EventId],
        ) -> creda_net::Result<Vec<IdentityEventNode>> {
            let canned = self.canned.lock().unwrap();
            Ok(canned.iter().filter(|n| ids.contains(&n.id)).cloned().collect())
        }
        async fn request_manifest(&self, _peer: &[u8]) -> creda_net::Result<Vec<EventId>> {
            let canned = self.canned.lock().unwrap();
            Ok(canned.iter().map(|n| n.id).collect())
        }
        async fn connected_peers(&self) -> creda_net::Result<Vec<Vec<u8>>> {
            Ok(Vec::new())
        }
        fn local_peer_id(&self) -> Vec<u8> {
            self.peer_id.clone()
        }
    }

    fn demographics(dob: &str) -> Demographics {
        Demographics {
            name_family: Some(vec![TokenizedString("tok:smith".into())]),
            date_of_birth: Some(TokenizedDate(format!("tok:{dob}"))),
            sex: Some(AdministrativeGender::Female),
            ..Default::default()
        }
    }
    fn signed_assert(key: &SigningKey, dob: &str) -> IdentityEventNode {
        IdentityEventNode::create(
            EventPayload::Assert {
                demographics: demographics(dob),
                verification_method: VerificationMethod::GovernmentPhotoId,
            },
            vec![],
            key,
            1,
            "2026-01-01T00:00:00Z",
            None,
        )
        .unwrap()
    }
    fn core_with(signer: InMemorySigner) -> Arc<CredaCore> {
        Arc::new(CredaCore::new(
            Box::new(MemoryStore::new()),
            Box::new(signer),
            CredaConfig::default(),
        ))
    }

    #[tokio::test]
    async fn publish_then_ingest_roundtrip_with_real_signature_check() {
        // Peer A creates a signed Assert through its engine.
        let key_a = SigningKey::generate(SignatureAlgorithm::Ed25519).unwrap();
        let vk_a = key_a.verifying_key();
        let fp_a = CertificateFingerprint::new(vk_a.fingerprint());
        let core_a = core_with(InMemorySigner::from_key(key_a));
        let node = core_a
            .create_event(
                EventPayload::Assert {
                    demographics: demographics("1980-01-01"),
                    verification_method: VerificationMethod::GovernmentPhotoId,
                },
                vec![],
            )
            .unwrap();

        // Publish it; the mock records the batch and its bucket.
        let tx = MockTransport::new(b"peerA");
        let repl_a = Replicator::new(core_a, tx.clone(), empty_resolver(), 1024);
        let bucket = repl_a.publish_event(&node).await.unwrap();
        assert!(bucket.is_some(), "an Assert with family+DOB+sex must route to a bucket");
        let pubs = tx.published();
        assert_eq!(pubs.len(), 1);
        assert_eq!(pubs[0].0, bucket.unwrap());
        assert_eq!(pubs[0].1.events[0].id, node.id);

        // Peer B (knows A's key) ingests the wire bytes — real signature verification.
        let core_b = core_with(InMemorySigner::generate().unwrap());
        let repl_b = Replicator::new(
            core_b.clone(),
            MockTransport::new(b"peerB"),
            resolver_for(&fp_a, &vk_a),
            1024,
        );
        let wire = pubs[0].1.to_bytes().unwrap();
        let summary = repl_b.ingest_batch(&wire).unwrap();
        assert_eq!(summary.accepted, 1);
        assert!(core_b.get_event(&node.id).unwrap().is_some());

        // Redelivery is suppressed by batch-level dedup.
        let again = repl_b.ingest_batch(&wire).unwrap();
        assert_eq!(again.accepted, 0);
        assert_eq!(again.duplicates, 1);
    }

    #[test]
    fn tampered_or_unsigned_events_are_rejected() {
        let key_a = SigningKey::generate(SignatureAlgorithm::Ed25519).unwrap();
        let vk_a = key_a.verifying_key();
        let fp_a = CertificateFingerprint::new(vk_a.fingerprint());
        let node = signed_assert(&key_a, "1980-01-01");

        // Tamper with the payload after signing.
        let mut bad = node.clone();
        if let EventPayload::Assert { demographics, .. } = &mut bad.payload {
            demographics.date_of_birth = Some(TokenizedDate("tok:1999-09-09".into()));
        }

        let core_b = core_with(InMemorySigner::generate().unwrap());
        let repl_b = Replicator::new(
            core_b.clone(),
            MockTransport::new(b"b"),
            resolver_for(&fp_a, &vk_a),
            64,
        );
        let batch = GossipBatch::new(b"peerA".to_vec(), 0, vec![bad.clone()]);
        let s = repl_b.ingest_batch(&batch.to_bytes().unwrap()).unwrap();
        assert_eq!(s.accepted, 0);
        assert_eq!(s.rejected, 1);
        assert!(core_b.get_event(&bad.id).unwrap().is_none());

        // An event from an unknown signer (no key) is also refused.
        let core_c = core_with(InMemorySigner::generate().unwrap());
        let repl_c = Replicator::new(core_c.clone(), MockTransport::new(b"c"), empty_resolver(), 64);
        let good_batch = GossipBatch::new(b"peerA".to_vec(), 0, vec![node.clone()]);
        let s2 = repl_c.ingest_batch(&good_batch.to_bytes().unwrap()).unwrap();
        assert_eq!(s2.rejected, 1);
        assert!(core_c.get_event(&node.id).unwrap().is_none());
    }

    #[tokio::test]
    async fn run_anti_entropy_round_fetches_manifest_and_heals_gaps() {
        let key_a = SigningKey::generate(SignatureAlgorithm::Ed25519).unwrap();
        let vk_a = key_a.verifying_key();
        let fp_a = CertificateFingerprint::new(vk_a.fingerprint());
        let a = signed_assert(&key_a, "1980-01-01");
        let b = signed_assert(&key_a, "1990-02-02");

        // Peer B already holds `a`. The (mock) remote serves a manifest of [a, b] and serves the
        // event nodes themselves on follow-up. run_anti_entropy_round drives the whole pull.
        let core_b = core_with(InMemorySigner::generate().unwrap());
        let tx = MockTransport::new(b"peerB");
        tx.set_canned(vec![a.clone(), b.clone()]); // manifest = ids; request_events filters
        let repl_b = Replicator::new(core_b.clone(), tx, resolver_for(&fp_a, &vk_a), 64);

        // Seed peer B with `a` so the delta is exactly {b}.
        repl_b
            .ingest_batch(&GossipBatch::new(b"seed".to_vec(), 0, vec![a.clone()]).to_bytes().unwrap())
            .unwrap();

        let summary = repl_b.run_anti_entropy_round(b"peerA").await.unwrap();
        assert_eq!(summary.accepted, 1, "AE round should fetch and accept the one missing event");
        assert!(core_b.get_event(&b.id).unwrap().is_some());
    }

    #[tokio::test]
    async fn anti_entropy_fetches_and_ingests_missing_events() {
        let key_a = SigningKey::generate(SignatureAlgorithm::Ed25519).unwrap();
        let vk_a = key_a.verifying_key();
        let fp_a = CertificateFingerprint::new(vk_a.fingerprint());
        let a = signed_assert(&key_a, "1980-01-01");
        let b = signed_assert(&key_a, "1990-02-02");

        // Peer B already holds `a`; the remote peer holds both `a` and `b`.
        let core_b = core_with(InMemorySigner::generate().unwrap());
        let tx = MockTransport::new(b"peerB");
        tx.set_canned(vec![b.clone()]); // the transport will serve `b` on request
        let repl_b = Replicator::new(core_b.clone(), tx, resolver_for(&fp_a, &vk_a), 64);
        repl_b
            .ingest_batch(&GossipBatch::new(b"seed".to_vec(), 0, vec![a.clone()]).to_bytes().unwrap())
            .unwrap();

        let summary = repl_b.anti_entropy(b"peerA", &[a.id, b.id]).await.unwrap();
        assert_eq!(summary.accepted, 1, "should fetch and accept the one missing event");
        assert!(core_b.get_event(&b.id).unwrap().is_some());
    }
}
