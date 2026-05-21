//! # creda-verifier
//!
//! The **relying-side** half of dual-control authorization enforcement (spec §4.5.2, §10.3): at
//! the point of use, confirm that the authorization under which data was obtained still holds and
//! that the data's provenance is intact. It checks three things together (§10.3.2):
//!
//! 1. **Authorization validity** — the governing Grant is signed, scoped, unexpired,
//!    audience-matched, and unrevoked (reuses [`creda_graph::evaluate`], §4.6).
//! 2. **Identity continuity** — the Grant is bound to this patient's subgraph (§5.2.4).
//! 3. **Provenance integrity** — no missing parents in the relevant chain (causal consistency).
//!
//! It runs **locally and offline** (§10.3.3): it only reads a local read-only DAG replica behind
//! the [`creda_store::Store`] trait and never calls the source system. When its DAG view is older
//! than a configurable threshold it reports staleness and the view's age, so the relying party
//! can decide whether stale-state verification is acceptable for the use at hand. The exact
//! stale-state policy is `TODO(open-question-13.4.3)`.

mod error;
mod verifier;

pub use error::{Error, Result};
pub use verifier::{VerificationReport, Verifier, VerifyRequest};
