//! # creda-events
//!
//! The Creda event model — the heart of the system (build milestone M1).
//!
//! This crate is **pure data + cryptography**: it has no network or storage dependencies.
//! It defines the signed DAG event node, the ten event types spanning identity continuity
//! and portable authorization, their payloads, canonical (deterministic) CBOR
//! serialization, Blake3 content hashing, UUIDv7 identifiers, and the algorithm-agile
//! signature scheme.
//!
//! Governing specification sections (see `docs/creda-technical-spec.md`):
//! - §3  Identity Model — the seven identity event types and the signature model.
//! - §4  Portable Authorization — the three authorization event types.
//! - §5  Data Structures — the event node schema, PQC requirements, payload schemas,
//!       UUIDv7 generation, and the Demographics struct.
//!
//! ## Determinism is load-bearing
//!
//! Signature verification requires that the same logical event always serializes to the
//! same bytes. Creda uses canonical CBOR (RFC 8949 Core Deterministic Encoding): map keys
//! are sorted by their encoded form, absent optional fields are omitted (not encoded as
//! null), and no floating-point values are used. See [`canonical`].
//!
//! ## What this crate does NOT do
//!
//! Graph-dependent invariants — e.g. that a `Contest` is created only by a party to the
//! linked subgraph (§3.4.3), or that an `Amend` is signed by the *originating* institution
//! of its target (§3.4.5) — require traversal context and are enforced in `creda-graph`
//! (M3). This crate enforces only the invariants checkable from a single event in
//! isolation; those graph-level rules are documented at their call sites.

pub mod canonical;
pub mod crypto;
pub mod demographics;
pub mod error;
pub mod event;
pub mod hash;
pub mod ids;
pub mod payload;

pub use crypto::{CryptoSignature, SignatureAlgorithm, SigningKey, VerifyingKey};
pub use demographics::{
    AdministrativeGender, Demographics, InstitutionalIdentifier, InsuranceIdentifier,
    StructuredAddress, TokenizedDate, TokenizedString, VerificationMethod,
};
pub use error::{Error, Result};
pub use event::{IdentityEventNode, IdentityEventType, RedistributionPolicy, TestDataTag};
pub use hash::{ContentHash, HashAlgorithm};
pub use ids::{CertificateFingerprint, EventId};
pub use payload::{
    AttestPurpose, AuthorizationScope, ContestReason, EventPayload, GrantAudience, GrantPurpose,
    LinkMethod, TombstoneBasis, UseMode, VolumeConstraints,
};
