//! Error types for the event model.

use thiserror::Error;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors produced when building, serializing, signing, verifying, or validating events.
#[derive(Debug, Error)]
pub enum Error {
    /// Canonical CBOR serialization failed.
    #[error("canonical serialization failed: {0}")]
    Serialization(String),

    /// CBOR deserialization failed.
    #[error("deserialization failed: {0}")]
    Deserialization(String),

    /// A signature failed to verify (bytes did not match, or wrong key).
    #[error("signature verification failed")]
    SignatureInvalid,

    /// The signing/verifying key did not match the algorithm requested.
    #[error("key/algorithm mismatch: expected {expected}, got {got}")]
    AlgorithmMismatch { expected: String, got: String },

    /// A post-quantum algorithm was requested but the `pqc` feature is not enabled.
    #[error("signature algorithm {0} unavailable: build with the `pqc` feature")]
    AlgorithmUnavailable(String),

    /// Key material was malformed (wrong length, unparseable).
    #[error("malformed key material: {0}")]
    MalformedKey(String),

    /// Malformed signature bytes (e.g. a hybrid signature that did not decode).
    #[error("malformed signature: {0}")]
    MalformedSignature(String),

    /// A structural invariant for an event was violated (see [`crate::event`]).
    #[error("event validation failed: {0}")]
    Validation(String),
}
