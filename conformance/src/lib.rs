//! # creda-conformance
//!
//! Conformance suite + synthetic data generator (build milestone M9, spec §11.4).
//!
//! - [`generator`] — produces synthetic patient subgraphs (deterministic content from a seed,
//!   realistic demographics from public-domain corpora, realistic event chains, configurable
//!   scale and scenarios). Every event is tagged as test data (§11.4.1) via
//!   [`creda_events::IdentityEventNode::create_test_data`], so synthetic events propagate and
//!   replicate like real ones but are filtered from clinical responses.
//! - [`filter`] — the clinical-vs-operator views: clinical responses exclude test-data events
//!   (§11.4.1); operator-scoped queries see everything.
//!
//! The conformance *tests* (in `tests/`) run the generated data through the store and graph and
//! assert the system's contracts: provenance preservation, authorization + revocation
//! enforcement, data-category handling, and test-data filtering.
//!
//! The deployment/multi-peer parts of conformance (a `helm install` on kind/k3d, gossip
//! convergence, anti-entropy repair, partition/rejoin, and the Bound-1 revocation-latency check
//! from §4.7) require real peers and a network, so they live in the `testbed/` (DQ-3) and run
//! once the libp2p transport and gRPC serve path are wired.

pub mod filter;
pub mod generator;

pub use filter::{clinical_view, operator_view};
pub use generator::{Generator, Scenario};
