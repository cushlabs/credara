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

use creda_events::{CertificateFingerprint, EventId, EventPayload, IdentityEventNode};
use creda_graph::{
    evaluate, project, AuthorizationDecision, AuthorizationQuery, ConfidenceConfig,
    EffectiveIdentity, Subgraph,
};
use creda_net::Snapshot;
use creda_store::Store;

use crate::config::CredaConfig;
use crate::error::Result;
use crate::signer::Signer;

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
        let node = self
            .signer
            .create_event(payload, parent_ids, clock, now_rfc3339(), None)?;
        self.store.put_event(&node)?;
        Ok(node)
    }

    /// `GetEvent` (§10.1.3).
    pub fn get_event(&self, id: &EventId) -> Result<Option<IdentityEventNode>> {
        Ok(self.store.get_event(id)?)
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
    pub fn event_count(&self) -> Result<usize> {
        Ok(self.store.all_event_ids()?.len())
    }
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
