//! # creda-net
//!
//! Networking and replication for Creda (build milestone M4). Governing spec: §6 (Network
//! Architecture) and §7 (Replication and Consistency).
//!
//! ## Approach: assemble, don't build — with the assembled part quarantined
//!
//! Per the spec's build-vs-buy contract (Appendix C) and §6.2.1/§6.3.1, the networking layer is
//! **libp2p** (gossipsub, Kademlia DHT, Noise transport, NAT traversal). We do not reimplement
//! any of that. But libp2p is a large, fast-moving dependency, so this crate is split in two:
//!
//! - **The genuinely-new logic — libp2p-free and unit-testable without a network.** This is the
//!   default build:
//!   - [`bucketing`] — the 1,024 topic-bucket scheme (`Blake3(dht_key) mod 1024`, §6.2.4) and
//!     DHT key derivation (§6.1.6).
//!   - [`antientropy`] — the Merkle-root-over-**UUID-set** mechanism and the reconciliation
//!     delta (§6.1.8). Deliberately hashes UUIDs, not contents, so tombstoning doesn't diverge
//!     roots (§6.1.8, §7.2.2).
//!   - [`snapshot`] — the snapshot format (sorted canonical-CBOR events + a manifest with a
//!     Blake3 integrity hash and event count, §6.2.5/§6.3.2).
//!   - [`gossip`] — the gossip batch envelope and bounded dedup (§6.1.4, §6.2.2).
//!   - [`transport::NetworkTransport`] — the trait boundary that lets libp2p be replaced
//!     without restructuring the rest of the system (§6.3.1, §10.1).
//!
//! - **The assembled part — libp2p, behind an off-by-default feature, in one module.**
//!   [`libp2p_adapter`] (feature `libp2p`) is a thin [`NetworkTransport`] implementation over a
//!   libp2p `Swarm`. It is the single isolation point for libp2p version churn.
//!
//! ### Why libp2p is off by default
//!
//! `cargo build` and `make test` with default features never compile libp2p, so the workspace
//! stays green and fast even as rust-libp2p's API shifts between versions. The heavy compile and
//! any first-build API reconciliation are opt-in (`--features libp2p`), enabled by Creda Core
//! (M5) and the multi-peer test bed (DQ-3). The full multi-peer convergence / partition tests
//! (the M4 "Done when") live in that test bed, since they need real peers and Core to drive them.
//!
//! DHT query-privacy is unresolved — `TODO(open-question-13.3)` / spec §8.5.

pub mod antientropy;
pub mod bucketing;
pub mod error;
pub mod gossip;
pub mod snapshot;
pub mod transport;
mod util;

#[cfg(feature = "libp2p")]
pub mod libp2p_adapter;

pub use antientropy::{merkle_root, reconcile, MerkleRoot, Reconciliation};
pub use bucketing::{bucket_of, topic_for_bucket, topic_for_key, DhtKey, BUCKET_COUNT, TOPIC_PREFIX};
pub use error::{Error, Result};
pub use gossip::{BatchConfig, GossipBatch, SeenSet};
pub use snapshot::{Snapshot, SnapshotManifest};
pub use transport::{EventSource, NetworkTransport};

#[cfg(feature = "libp2p")]
pub use libp2p_adapter::Libp2pTransport;
