//! # creda-export-gate
//!
//! The **source-side** half of dual-control authorization enforcement (spec §4.5.1, §10.2):
//! before data leaves a source system, the Export Gate validates the Portable Authorization
//! Artifact governing the release and, on success, emits an `ExportReceipt` recording it.
//!
//! It is intentionally thin (§10.2.3): it does **not** reimplement authorization logic — it
//! reuses [`creda_graph::evaluate`] (the seven-step algorithm, §4.6) over the local DAG view and
//! acts on the result. Export uses a **deny-by-default** posture: egress requires an explicit,
//! covering, unexpired, correctly-scoped, audience-matched, unrevoked Grant (§10.2.2) — there is
//! no treatment-presumed shortcut for data leaving the institution.

mod error;
mod gate;

pub use error::{Error, Result};
pub use gate::{ExportGate, ExportOutcome, ExportRequest};
