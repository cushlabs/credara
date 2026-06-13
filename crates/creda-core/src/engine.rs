//! The Creda Core engine — composing the event model, store, and graph reasoning (spec §10.1).
//!
//! `CredaCore` is the synchronous heart of a peer: it owns a [`Store`] and a [`Signer`] and
//! exposes the operations the gRPC API surfaces (§10.1.3) — create/get events, materialize a
//! subgraph, project effective identity, match by tokens, evaluate authorization, snapshot.
//! It is deliberately synchronous and transport-free: the gRPC server (feature `grpc`) and the
//! networking layer (feature `libp2p`) are thin async adapters on top, so this — the part where
//! the real domain logic lives — is unit-testable with an in-memory store and no async runtime.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use creda_events::{
    CertificateFingerprint, EventId, EventPayload, IdentityEventNode, TestDataTag, VerifyingKey,
};
use creda_graph::{
    evaluate, project, AuthorizationDecision, AuthorizationQuery, ConfidenceConfig,
    EffectiveIdentity, Subgraph,
};
use creda_net::Snapshot;
use creda_store::Store;

use crate::config::CredaConfig;
use crate::error::Result;
use crate::signer::Signer;

/// Resolves an event author's verifying key from its certificate fingerprint — the seam to the
/// participant registry / UDAP certificate infrastructure (Appendix C, open question). Ingest
/// needs the public key to verify a received event's signature; the key is *not* carried in the
/// event (only its fingerprint is), so it must be looked up.
///
/// The production implementation backs onto the UDAP/Participant Registry; that integration is an
/// open question. Tests and the loopback test bed supply an in-memory key map.
pub trait VerifyingKeyResolver: Send + Sync {
    /// The verifying key for an institution fingerprint, or `None` if the signer is unknown
    /// (an unknown signer means the event cannot be authenticated and must be rejected).
    fn resolve(&self, fingerprint: &CertificateFingerprint) -> Option<VerifyingKey>;
}

/// The outcome of ingesting a replicated event (§3.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ingest {
    /// Verified and stored.
    Accepted,
    /// Already present locally (idempotent no-op).
    AlreadyHave,
    /// Refused — the reason is suitable for logging/metrics, not for trusting.
    Rejected(String),
}

/// The composed peer engine.
pub struct CredaCore {
    store: Box<dyn Store>,
    signer: Box<dyn Signer>,
    config: CredaConfig,
    confidence: ConfidenceConfig,
    // Per-peer monotonic logical clock. NOTE: the spec's logical clock is per-subgraph (§3.5);
    // this per-peer monotonic counter is a sufficient Lamport-style stand-in for M5 — a proper
    // per-subgraph clock keyed off the materialized subgraph is a refinement.
    clock: AtomicU64,
}

impl CredaCore {
    /// Build the engine from its composed parts.
    pub fn new(store: Box<dyn Store>, signer: Box<dyn Signer>, config: CredaConfig) -> Self {
        Self {
            store,
            signer,
            config,
            confidence: ConfidenceConfig::default(),
            clock: AtomicU64::new(1),
        }
    }

    /// This peer's institution fingerprint.
    pub fn institution_id(&self) -> CertificateFingerprint {
        self.signer.institution_id()
    }

    /// `CreateEvent` (§10.1.3): build, validate, sign, and persist a new event. Read-your-writes
    /// holds — the event is in the local store before this returns (§7.1.3).
    pub fn create_event(
        &self,
        payload: EventPayload,
        parent_ids: Vec<EventId>,
    ) -> Result<IdentityEventNode> {
        let clock = self.clock.fetch_add(1, Ordering::SeqCst);
        let node = if self.config.synthetic_only {
            // Synthetic-only guardrail (docs/PILOT.md): auto-tag every locally created event as
            // test data so this peer can only ever emit provably-synthetic events.
            self.signer.create_test_event(
                payload,
                parent_ids,
                clock,
                now_rfc3339(),
                None,
                synthetic_pilot_tag(),
            )?
        } else {
            self.signer
                .create_event(payload, parent_ids, clock, now_rfc3339(), None)?
        };
        self.store.put_event(&node)?;
        Ok(node)
    }

    /// `GetEvent` (§10.1.3).
    pub fn get_event(&self, id: &EventId) -> Result<Option<IdentityEventNode>> {
        Ok(self.store.get_event(id)?)
    }

    /// Fetch several events by id, skipping any not held locally. Used to answer a peer's
    /// targeted event request (§6.1.5, §6.1.8) and to serve replication deltas.
    pub fn get_events(&self, ids: &[EventId]) -> Result<Vec<IdentityEventNode>> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(node) = self.store.get_event(id)? {
                out.push(node);
            }
        }
        Ok(out)
    }

    /// All event ids held locally, sorted (UUIDv7 / creation-time order). Feeds the anti-entropy
    /// Merkle root and reconciliation (§6.1.8).
    pub fn all_event_ids(&self) -> Result<Vec<EventId>> {
        Ok(self.store.all_event_ids()?)
    }

    /// Ingest an event received from a peer (§3.6). **Signature verification is mandatory during
    /// replication**, and it is the gate that protects the whole graph: a peer accepts a foreign
    /// event only if its signature verifies against the author's resolved key, it is structurally
    /// valid, and its content hash (if present) matches. Already-held events are a no-op.
    ///
    /// Returns the [`Ingest`] outcome rather than erroring on a bad event, because a single bad
    /// event in a batch must not abort ingest of the rest; a transport/store failure still errors.
    pub fn ingest_event(
        &self,
        node: IdentityEventNode,
        keys: &dyn VerifyingKeyResolver,
    ) -> Result<Ingest> {
        if self.store.has_event(&node.id)? {
            return Ok(Ingest::AlreadyHave);
        }
        let Some(vk) = keys.resolve(&node.institution_id) else {
            return Ok(Ingest::Rejected("unknown signer (no key for fingerprint)".into()));
        };
        if node.verify_signature(&vk).is_err() {
            return Ok(Ingest::Rejected("signature verification failed".into()));
        }
        if let Err(e) = node.validate_structure() {
            return Ok(Ingest::Rejected(format!("structural validation failed: {e}")));
        }
        if node.verify_content_hash() == Some(false) {
            return Ok(Ingest::Rejected("content hash does not match payload".into()));
        }
        // Synthetic-only guardrail (docs/PILOT.md): on a synthetic-only network, refuse any event
        // that is not test_data-tagged — so untagged (potentially real) data cannot propagate in,
        // even from a signed, admitted peer that is misconfigured.
        if self.config.synthetic_only && !node.is_test_data() {
            return Ok(Ingest::Rejected(
                "synthetic-only network: refusing untagged (non-test-data) event".into(),
            ));
        }
        self.store.put_event(&node)?;
        Ok(Ingest::Accepted)
    }

    /// `GetSubgraph` (§10.1.3): materialize the subgraph reachable from the entry points.
    pub fn get_subgraph(&self, entry_points: &[EventId]) -> Result<Subgraph> {
        Ok(Subgraph::materialize(self.store.as_ref(), entry_points)?)
    }

    /// `GetEffectiveIdentity` (§10.1.3 / §5.2.4).
    pub fn effective_identity(&self, entry_points: &[EventId]) -> Result<EffectiveIdentity> {
        let subgraph = self.get_subgraph(entry_points)?;
        Ok(project(&subgraph, entry_points, &self.confidence, now_unix_secs()))
    }

    /// `MatchByTokens` (§10.1.3): candidate entry points whose demographics carry any of the
    /// given tokens (§5.2.5 index 1). Returns a sorted, de-duplicated set.
    pub fn match_by_tokens(&self, tokens: &[String]) -> Result<Vec<EventId>> {
        let mut hits: BTreeSet<EventId> = BTreeSet::new();
        for token in tokens {
            for id in self.store.entry_points_by_token(token)? {
                hits.insert(id);
            }
        }
        Ok(hits.into_iter().collect())
    }

    /// `EvaluateAuthorization` (§10.1.3 / §4.6) using this peer's configured default posture.
    pub fn evaluate_authorization(
        &self,
        entry_points: &[EventId],
        query: &AuthorizationQuery,
    ) -> Result<AuthorizationDecision> {
        let subgraph = self.get_subgraph(entry_points)?;
        // Volume utilization tracking is a Core responsibility not yet wired (§4.6 step 5);
        // pass an empty map for now.
        let utilization = std::collections::HashMap::new();
        Ok(evaluate(
            &subgraph,
            query,
            self.config.default_posture.to_graph(),
            now_unix_secs(),
            &utilization,
        ))
    }

    /// `creda snapshot` (§10.1.1): serialize this peer's event store to snapshot bytes (§6.2.5).
    pub fn snapshot_bytes(&self) -> Result<Vec<u8>> {
        let snapshot = Snapshot::from_store(self.store.as_ref(), now_unix_secs())?;
        Ok(snapshot.to_bytes()?)
    }

    /// Load a snapshot into the local store (bootstrap, §6.2.5). Returns the number of events.
    pub fn load_snapshot(&self, bytes: &[u8]) -> Result<usize> {
        let snapshot = Snapshot::from_bytes(bytes)?;
        Ok(snapshot.load_into_store(self.store.as_ref())?)
    }

    /// Read-only access to the resolved configuration.
    pub fn config(&self) -> &CredaConfig {
        &self.config
    }

    /// Number of events in the local store (`GetMetrics`, §10.1.3).
    /// Distinct institution audience display names appearing in AuthorizationGrants across the
    /// whole local store (not a single subgraph) — the network-wide "institutions seen here" list
    /// behind the Bridge's `GET /Organization` search. `InstitutionId` audiences (raw fingerprints)
    /// render as `fpr:<hex>`; class / wildcard audiences are their literal names. Synthetic-pilot
    /// scale: a full store scan is acceptable; revisit with a by-type index if the store grows.
    pub fn list_institutions(&self) -> Result<Vec<String>> {
        use creda_events::GrantAudience;
        let mut names: BTreeSet<String> = BTreeSet::new();
        for id in self.store.all_event_ids()? {
            let Some(node) = self.store.get_event(&id)? else {
                continue;
            };
            if let EventPayload::AuthorizationGrant { audience, .. } = &node.payload {
                let name = match audience {
                    GrantAudience::InstitutionId(fpr) => {
                        let hex: String = fpr.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
                        format!("fpr:{hex}")
                    }
                    GrantAudience::InstitutionClass(name) => name.clone(),
                    GrantAudience::ConstrainedWildcard(pattern) => pattern.clone(),
                };
                if !name.is_empty() {
                    names.insert(name);
                }
            }
        }
        Ok(names.into_iter().collect())
    }

    pub fn event_count(&self) -> Result<usize> {
        Ok(self.store.all_event_ids()?.len())
    }
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// The `test_data` tag stamped on every locally created event when `synthetic_only` is set
/// (docs/PILOT.md). `expiration_time: None` — pilot data lives until the network is wiped.
fn synthetic_pilot_tag() -> TestDataTag {
    TestDataTag {
        purpose: "closed-synthetic-pilot".to_string(),
        originating_test: "synthetic-only-network".to_string(),
        expiration_time: None,
    }
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
