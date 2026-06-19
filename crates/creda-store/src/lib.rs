//! # creda-store
//!
//! The Creda storage layer (build milestone M2).
//!
//! Each peer persists events in an embedded key-value store keyed by event UUID, with the
//! secondary indexes from spec §5.2.5 maintained alongside (§7.3.1). Everything sits behind a
//! single [`Store`] trait so the backend can be swapped without touching any other crate
//! (§7.4.1). The substrate decision is **RocksDB**; see `docs/storage-substrate.md`, which
//! resolves open question §13.1.1 (the libgit2 alternative was retired — its content-addressed
//! immutable objects fight the §3.4.6 right-to-be-forgotten scrub, and libgit2 lacks the reftable
//! backend needed for millions of subgraph refs).
//!
//! Backends:
//! - [`MemoryStore`] — always available; an in-memory implementation for tests and for
//!   downstream crates that don't want the RocksDB compile.
//! - [`RocksdbStore`] (feature `rocksdb`, default) — the production embedded backend
//!   (§7.4.1), using one column family per index.
//!
//! Governing spec sections: §5.2 (subgraph as query result), §5.2.5 (index structures),
//! §7.3 (storage architecture), Appendix C.1/C.3 (storage substrate).

pub mod error;
pub mod memory;
pub mod store;
pub mod tokens;

#[cfg(feature = "rocksdb")]
pub mod rocks;

pub use error::{Error, Result};
pub use memory::MemoryStore;
pub use store::Store;
pub use tokens::demographic_tokens;

#[cfg(feature = "rocksdb")]
pub use rocks::RocksdbStore;
