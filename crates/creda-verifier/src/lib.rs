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
//! than the threshold for the use's class it reports staleness, the view's age, the classified
//! [`UseClass`], and the applied threshold, so the relying party can apply its own override. The
//! stale-state policy is **per use type** ([`StalenessPolicy`], §13.4.3): a fresh-auth check before
//! a bulk export tolerates far less lag than a routine read. The recommended thresholds are
//! bootstrap defaults to be refined with pilot data (see `docs/staleness-policy.md`); the relying
//! institution keeps override authority.

mod error;
mod staleness;
mod verifier;

pub use error::{Error, Result};
pub use staleness::{StalenessPolicy, UseClass};
pub use verifier::{VerificationReport, Verifier, VerifyRequest};
