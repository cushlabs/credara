//! # creda-graph
//!
//! Graph reasoning over the Creda event DAG (build milestone M3). This is the first crate that
//! *interprets* events rather than storing them: it reads from a [`creda_store::Store`],
//! materializes patient subgraphs on demand, and computes the two things the system exists to
//! answer — **who a patient is** (effective identity) and **what is permitted** (authorization).
//!
//! Modules:
//! - [`subgraph`] — materialization and structural queries (spec §5.2.1–§5.2.3).
//! - [`validation`] — the graph-dependent event invariants deferred from M1 (§3.4.3, §3.4.5).
//! - [`identity`] — the effective-identity projection (§5.2.4) with disagreement flagging (§5.3.4).
//! - [`confidence`] — per-field confidence scoring (§5.3.2–§5.3.3).
//! - [`authorization`] — the seven-step authorization evaluation (§4.6).
//!
//! Identity events are *advisory* (the consumer decides how much to trust the projection);
//! authorization events are *enforced* (no covering Grant and no permissive default posture
//! means the request is denied). They share one subgraph but answer different questions (§4.8).

pub mod authorization;
pub mod confidence;
pub mod error;
pub mod identity;
pub mod link_chain;
pub mod subgraph;
pub mod validation;

pub use authorization::{
    evaluate, evaluate_with_link_chain, responder_may_serve, AuthorizationDecision,
    AuthorizationQuery, DefaultPosture, RequesterContext,
};
pub use confidence::{ConfidenceConfig, Contribution, FieldClass};
pub use error::{Error, Result};
pub use identity::{
    project, subgraph_identity, EffectiveIdentity, FieldEntry, FieldKey, FieldValue,
    SubgraphIdentity,
};
pub use link_chain::{evaluate_link_chain, LinkChainConfig, LinkChainResult, MethodCeilings};
pub use subgraph::Subgraph;
