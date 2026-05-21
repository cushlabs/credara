//! Test-data filtering (spec §11.4.1).
//!
//! Synthetic (test-data-tagged) events propagate and replicate like real events, but are filtered
//! out of **clinical** responses and from real patients' confidence scoring, while remaining fully
//! visible to **operator**-scoped queries. These helpers implement that partition over a set of
//! events; in production the Bridge applies the same rule by SMART scope (§11.4.1).

use creda_events::IdentityEventNode;

/// The clinical view: real events only (test-data events are filtered out, §11.4.1).
pub fn clinical_view(events: &[IdentityEventNode]) -> Vec<&IdentityEventNode> {
    events.iter().filter(|e| !e.is_test_data()).collect()
}

/// The operator view: every event, including synthetic test data (§11.4.1).
pub fn operator_view(events: &[IdentityEventNode]) -> Vec<&IdentityEventNode> {
    events.iter().collect()
}
