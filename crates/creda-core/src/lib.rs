//! # creda-core
//!
//! Creda Core (build milestone M5): the peer that composes the event model (M1), the store
//! (M2), graph reasoning (M3), and networking (M4) into one process with a gRPC API and CLI
//! (spec §10.1). M5 completes the M1→M5 dependency spine.
//!
//! ## Layering, and what's in the default build
//!
//! The same isolation discipline used across the workspace applies here:
//!
//! - **Default build (verifiable, no heavy/unverifiable deps):**
//!   - [`engine::CredaCore`] — the synchronous engine that wires store + graph and implements
//!     the operations the gRPC surface exposes (create/get events, materialize subgraph, project
//!     effective identity, match by tokens, evaluate authorization, snapshot). This is the heart
//!     of M5 and is unit-tested with an in-memory store.
//!   - [`config`] — hierarchical config (defaults → TOML → env → flags), validated at startup
//!     (§10.1.6).
//!   - [`signer`] — the `Signer` abstraction + in-memory implementation (§10.1.4).
//!   - The `creda` binary (`src/main.rs`) — CLI for `init` / `snapshot` / `inspect`; `serve`
//!     requires the `grpc` feature.
//!
//! - **Opt-in adapters (heavy / write-blind-risky, behind features):**
//!   - `grpc` — a tonic gRPC server wrapping the engine (§10.1.3). Needs `protoc`; codegen runs
//!     only with this feature (see `build.rs`).
//!   - `libp2p` — turns on `creda-net`'s libp2p transport and wires it in (§10.1.5).
//!
//! Keeping gRPC and libp2p opt-in means `cargo build` / `anchor creda` stay green and fast, and
//! the two adapters that are hardest to verify without compiling are reconciled only when their
//! feature is deliberately enabled.

pub mod config;
pub mod engine;
pub mod error;
pub mod registry;
pub mod replication;
pub mod signer;

#[cfg(feature = "grpc")]
pub mod grpc;

#[cfg(feature = "grpc")]
pub mod health;

pub use config::{CredaConfig, PostureSetting};
pub use engine::{CredaCore, Ingest, VerifyingKeyResolver};
pub use error::{Error, Result};
pub use registry::KeyRegistry;
pub use replication::{IngestSummary, Replicator};
pub use signer::{InMemorySigner, Signer};
