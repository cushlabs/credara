//! The Verifier: the three-part point-of-use check, local and offline (spec §10.3).

use std::collections::HashMap;

use creda_events::{EventId, IdentityEventType};
use creda_graph::{evaluate, AuthorizationQuery, DefaultPosture, Subgraph};
use creda_store::Store;

use crate::error::Result;

/// What to verify: the patient subgraph entry points, the governing Grant (the artifact under
/// which the data was obtained), and the authorization query describing the intended use.
pub struct VerifyRequest {
    pub entry_points: Vec<EventId>,
    pub governing_grant_id: EventId,
    pub query: AuthorizationQuery,
}

/// The outcome of verification. The three checks are reported separately (§10.3.2), plus
/// staleness (§10.3.3) which is advisory — the relying party decides whether a stale view is
/// acceptable for the use at hand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerificationReport {
    pub authorized: bool,
    pub identity_continuity: bool,
    pub provenance_intact: bool,
    /// True when the local DAG view is older than the configured threshold.
    pub stale: bool,
    /// Age of the local DAG view in seconds (now − last sync).
    pub dag_age_secs: i64,
    pub reason: String,
}

impl VerificationReport {
    /// Whether the use is permitted on the substantive checks. Staleness is reported separately
    /// and left to the caller's policy (§10.3.3).
    pub fn is_valid(&self) -> bool {
        self.authorized && self.identity_continuity && self.provenance_intact
    }
}

/// Relying-side enforcement point. Operates against a local read-only DAG replica.
pub struct Verifier {
    /// DAG views older than this are flagged stale (§10.3.3). `TODO(open-question-13.4.3)`.
    staleness_threshold_secs: i64,
}

impl Verifier {
    pub fn new(staleness_threshold_secs: i64) -> Self {
        Self { staleness_threshold_secs }
    }

    /// Verify a use against the local store. `last_sync_unix_secs` is the time of the most recent
    /// successful synchronization of the local replica (for staleness, §10.3.3). No network call
    /// is made — verification is entirely local (§10.3.3).
    pub fn verify(
        &self,
        store: &dyn Store,
        request: &VerifyRequest,
        now_unix_secs: i64,
        last_sync_unix_secs: i64,
    ) -> Result<VerificationReport> {
        let subgraph = Subgraph::materialize(store, &request.entry_points)?;

        // 1. Authorization validity — reuse the seven-step algorithm; require the *governing*
        //    artifact to be one of the covering Grants (deny-by-default at the point of use).
        let decision = evaluate(
            &subgraph,
            &request.query,
            DefaultPosture::DenyByDefault,
            now_unix_secs,
            &HashMap::new(),
        );
        let authorized =
            decision.authorized && decision.covering_grants.contains(&request.governing_grant_id);

        // 2. Identity continuity — the governing Grant is present and bound to this subgraph
        //    (materialization only reaches it if it is connected to the entry points, §5.2.1).
        let identity_continuity = matches!(
            subgraph.get(&request.governing_grant_id).map(|n| n.event_type),
            Some(IdentityEventType::AuthorizationGrant)
        );

        // 3. Provenance integrity — every event in the relevant chain has its parents locally
        //    (no dangling references; causal consistency, §7.1.2).
        let mut provenance_intact = true;
        'outer: for node in subgraph.nodes() {
            for parent in &node.parent_ids {
                if !store.has_event(parent)? {
                    provenance_intact = false;
                    break 'outer;
                }
            }
        }

        let dag_age_secs = (now_unix_secs - last_sync_unix_secs).max(0);
        let stale = dag_age_secs > self.staleness_threshold_secs;

        let reason = if authorized && identity_continuity && provenance_intact {
            if stale {
                format!("valid, but local DAG view is stale ({dag_age_secs}s old)")
            } else {
                "valid".to_string()
            }
        } else if !authorized {
            decision.reason.clone()
        } else if !identity_continuity {
            "governing Grant is not present/bound in this patient's subgraph".to_string()
        } else {
            "provenance chain is incomplete (missing parent events)".to_string()
        };

        Ok(VerificationReport {
            authorized,
            identity_continuity,
            provenance_intact,
            stale,
            dag_age_secs,
            reason,
        })
    }
}
