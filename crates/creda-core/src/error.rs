//! Error types for Creda Core.

use thiserror::Error;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors from the core engine, configuration, or signing.
#[derive(Debug, Error)]
pub enum Error {
    /// An event failed creation, validation, or signing (creda-events).
    #[error("event error: {0}")]
    Event(String),

    /// A storage operation failed (creda-store).
    #[error("store error: {0}")]
    Store(String),

    /// A graph operation failed (creda-graph).
    #[error("graph error: {0}")]
    Graph(String),

    /// A networking/snapshot operation failed (creda-net).
    #[error("net error: {0}")]
    Net(String),

    /// Configuration was invalid or could not be loaded (fail-loud at startup, §10.1.6).
    #[error("config error: {0}")]
    Config(String),

    /// An I/O error (config files, key material).
    #[error("io error: {0}")]
    Io(String),
}

impl From<creda_events::Error> for Error {
    fn from(e: creda_events::Error) -> Self {
        Error::Event(e.to_string())
    }
}
impl From<creda_store::Error> for Error {
    fn from(e: creda_store::Error) -> Self {
        Error::Store(e.to_string())
    }
}
impl From<creda_graph::Error> for Error {
    fn from(e: creda_graph::Error) -> Self {
        Error::Graph(e.to_string())
    }
}
impl From<creda_net::Error> for Error {
    fn from(e: creda_net::Error) -> Self {
        Error::Net(e.to_string())
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}
