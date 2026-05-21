//! Error types for networking and replication.

use thiserror::Error;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors from snapshot handling, transport operations, or replication.
#[derive(Debug, Error)]
pub enum Error {
    /// Serializing/deserializing a snapshot or gossip batch failed.
    #[error("codec error: {0}")]
    Codec(String),

    /// A snapshot failed its integrity check (hash mismatch or wrong event count).
    #[error("snapshot integrity check failed: {0}")]
    Integrity(String),

    /// A [`crate::transport::NetworkTransport`] operation failed.
    #[error("transport error: {0}")]
    Transport(String),

    /// The underlying [`creda_store::Store`] returned an error.
    #[error("store error: {0}")]
    Store(String),
}

impl From<creda_events::Error> for Error {
    fn from(e: creda_events::Error) -> Self {
        Error::Codec(e.to_string())
    }
}

impl From<creda_store::Error> for Error {
    fn from(e: creda_store::Error) -> Self {
        Error::Store(e.to_string())
    }
}
