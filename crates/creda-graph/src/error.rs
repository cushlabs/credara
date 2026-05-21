//! Error types for graph reasoning.

use thiserror::Error;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors produced during subgraph materialization, validation, projection, or authorization.
#[derive(Debug, Error)]
pub enum Error {
    /// A backend [`creda_store::Store`] call failed.
    #[error("store error: {0}")]
    Store(String),

    /// A graph-dependent event invariant was violated (spec §3.4.3, §3.4.5).
    #[error("invariant violation: {0}")]
    Invariant(String),

    /// The graph was structurally inconsistent in a way that should not occur given a
    /// causally-consistent store (e.g. a referenced parent was absent during materialization).
    #[error("graph inconsistency: {0}")]
    Inconsistent(String),
}

impl From<creda_store::Error> for Error {
    fn from(e: creda_store::Error) -> Self {
        Error::Store(e.to_string())
    }
}
