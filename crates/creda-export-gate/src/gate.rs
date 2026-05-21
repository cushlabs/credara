//! The Export Gate: validate-then-release with an emitted receipt (spec §4.5.1, §10.2).

use std::collections::HashMap;

use creda_events::{AuthorizationScope, EventId, EventPayload, IdentityEventNode, SigningKey};
use creda_graph::{evaluate, AuthorizationQuery, DefaultPosture, Subgraph};
use creda_store::Store;

use crate::error::{Error, Result};

/// A request to release data: which patient subgraph, and the authorization query describing the
/// requester, purpose, use-mode, and scope of the release.
pub struct ExportRequest {
    pub entry_points: Vec<EventId>,
    pub query: AuthorizationQuery,
}

/// The Gate's decision. On `Permitted`, the caller persists and gossips the `ExportReceipt`
/// (which records the release under a specific Grant, §4.3.3) and then performs the egress.
pub enum ExportOutcome {
    Permitted { receipt: IdentityEventNode },
    Refused { reason: String },
}

impl ExportOutcome {
    pub fn is_permitted(&self) -> bool {
        matches!(self, ExportOutcome::Permitted { .. })
    }

    /// The emitted receipt, if the export was permitted.
    pub fn receipt(&self) -> Option<&IdentityEventNode> {
        match self {
            ExportOutcome::Permitted { receipt } => Some(receipt),
            ExportOutcome::Refused { .. } => None,
        }
    }
}

/// Source-side enforcement point. Holds the source institution's signing key so it can emit the
/// `ExportReceipt` (§10.2.2). It does not own the store — the caller passes the local DAG view.
pub struct ExportGate {
    signing_key: SigningKey,
}

impl ExportGate {
    pub fn new(signing_key: SigningKey) -> Self {
        Self { signing_key }
    }

    /// Validate the authorization governing a release and, if valid, emit an `ExportReceipt`.
    ///
    /// `now_unix_secs` is the time used for expiry evaluation and the receipt timestamp;
    /// `logical_clock` is the receipt's per-subgraph clock value (supplied by the caller/Core).
    pub fn authorize_export(
        &self,
        store: &dyn Store,
        request: &ExportRequest,
        now_unix_secs: i64,
        logical_clock: u64,
    ) -> Result<ExportOutcome> {
        let subgraph = Subgraph::materialize(store, &request.entry_points)?;

        // Reuse the authorization algorithm (§4.6). Deny-by-default: egress needs an explicit
        // covering Grant — no treatment-presumed shortcut for data leaving the institution.
        let decision = evaluate(
            &subgraph,
            &request.query,
            DefaultPosture::DenyByDefault,
            now_unix_secs,
            &HashMap::new(),
        );

        let Some(governing_grant_id) = decision.covering_grants.first().copied() else {
            return Ok(ExportOutcome::Refused {
                reason: if decision.authorized {
                    // Should not happen under deny-by-default, but be explicit.
                    "export requires an explicit AuthorizationGrant (no covering artifact)".into()
                } else {
                    decision.reason
                },
            });
        };

        // Record exactly what is being released (§4.3.3).
        let released_scope = AuthorizationScope {
            subgraph_segments: request.query.requested_segments.clone(),
            event_types: request.query.requested_event_types.clone(),
            data_categories: request.query.requested_data_categories.clone(),
        };
        let payload = EventPayload::ExportReceipt {
            governing_grant_id,
            requesting_institution: request.query.requester.fingerprint.clone(),
            released_scope,
        };
        let receipt = IdentityEventNode::create(
            payload,
            vec![governing_grant_id],
            &self.signing_key,
            logical_clock,
            rfc3339(now_unix_secs)?,
            None,
        )?;
        Ok(ExportOutcome::Permitted { receipt })
    }
}

fn rfc3339(unix_secs: i64) -> Result<String> {
    time::OffsetDateTime::from_unix_timestamp(unix_secs)
        .map_err(|e| Error::Timestamp(e.to_string()))?
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|e| Error::Timestamp(e.to_string()))
}
