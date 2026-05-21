//! Error types for the Export Gate.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("store error: {0}")]
    Store(String),

    #[error("graph error: {0}")]
    Graph(String),

    #[error("event error: {0}")]
    Event(String),

    /// Could not build a valid wall-clock timestamp for the ExportReceipt.
    #[error("timestamp error: {0}")]
    Timestamp(String),
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
impl From<creda_events::Error> for Error {
    fn from(e: creda_events::Error) -> Self {
        Error::Event(e.to_string())
    }
}
