//! Error types for the storage layer.

use thiserror::Error;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors produced by a [`crate::Store`] backend.
#[derive(Debug, Error)]
pub enum Error {
    /// The underlying storage engine returned an error.
    #[error("storage backend error: {0}")]
    Backend(String),

    /// An event failed to (de)serialize to/from its stored bytes.
    #[error("codec error: {0}")]
    Codec(String),

    /// Stored data was structurally corrupt (e.g. an index key of unexpected length).
    #[error("store corruption: {0}")]
    Corrupt(String),

    /// A backend method is scaffolded but not yet implemented.
    #[error("not implemented: {0}")]
    Unimplemented(&'static str),
}

impl From<creda_events::Error> for Error {
    fn from(e: creda_events::Error) -> Self {
        Error::Codec(e.to_string())
    }
}

#[cfg(feature = "rocksdb")]
impl From<rocksdb::Error> for Error {
    fn from(e: rocksdb::Error) -> Self {
        Error::Backend(e.to_string())
    }
}
