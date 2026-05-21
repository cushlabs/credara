//! Error types for the Verifier.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("store error: {0}")]
    Store(String),

    #[error("graph error: {0}")]
    Graph(String),
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
