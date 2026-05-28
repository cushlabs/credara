//! The `NetworkTransport` trait — the boundary that quarantines libp2p (spec §6.3.1, §10.1).
//!
//! Everything above this trait (Creda Core, M5) speaks in terms of buckets, batches, DHT keys,
//! and event ids — never libp2p types. The spec is explicit (§6.3.1) that the networking layer
//! is abstracted behind a trait so libp2p "can be replaced if necessary without restructuring
//! the rest of the system" — for binary size, API stability, or health-IT compliance review.
//!
//! The production implementation is [`crate::libp2p_adapter::Libp2pTransport`] (feature
//! `libp2p`). A test/loopback implementation for the multi-peer test bed (DQ-3) is a planned
//! follow-up that lands with Core (M5).
//!
//! The methods are `async` (native async-fn-in-trait, stable since Rust 1.75). That makes the
//! trait not `dyn`-object-safe, so Core holds a concrete transport or is generic over
//! `T: NetworkTransport` rather than a `Box<dyn NetworkTransport>`. This is intentional — the
//! transport is chosen once at peer startup, not swapped at runtime.

use creda_events::{EventId, IdentityEventNode};

use crate::bucketing::DhtKey;
use crate::error::Result;
use crate::gossip::GossipBatch;

/// The peer-to-peer transport Creda Core drives. Implementations wrap a concrete networking
/// stack (libp2p) and expose only protocol-level operations.
pub trait NetworkTransport {
    /// Publish a gossip batch to a topic bucket (§6.2.4). The batch is the unit of propagation
    /// (§6.2.2).
    fn publish_batch(
        &self,
        bucket: u64,
        batch: &GossipBatch,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Subscribe to a topic bucket to receive its events (§6.2.4).
    fn subscribe_bucket(&self, bucket: u64) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Unsubscribe from a topic bucket (during periodic subscription rebalancing, §6.2.4).
    fn unsubscribe_bucket(
        &self,
        bucket: u64,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Announce this peer as a provider for a subgraph's DHT key (§6.1.5, §6.2.4). Refreshed
    /// periodically by Core.
    fn dht_provide(&self, key: DhtKey) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Find peers that have announced themselves as providers for a DHT key (§6.1.5). Returns
    /// peer ids as bytes.
    fn dht_find_providers(
        &self,
        key: DhtKey,
    ) -> impl std::future::Future<Output = Result<Vec<Vec<u8>>>> + Send;

    /// Request specific events by id directly from a peer (the targeted fetch after a DHT
    /// lookup, §6.1.5, and the event-transfer step of anti-entropy, §6.1.8).
    fn request_events(
        &self,
        peer: &[u8],
        ids: &[EventId],
    ) -> impl std::future::Future<Output = Result<Vec<IdentityEventNode>>> + Send;

    /// This peer's own libp2p peer id, as bytes.
    fn local_peer_id(&self) -> Vec<u8>;
}

/// A read-only window into the local event store, used by the transport to answer **inbound**
/// event requests from peers (§6.1.5 targeted fetch and §6.1.8 anti-entropy transfer). It is the
/// symmetric counterpart to `Replicator::ingest_batch`: ingest is for events we *receive*, this
/// is for events we *serve* when asked.
///
/// Implementations are sync and may touch storage; the libp2p adapter dispatches calls on
/// `tokio::task::spawn_blocking` so the swarm event loop never blocks. Missing events are simply
/// omitted from the result — there is no "not found" error.
pub trait EventSource: Send + Sync + 'static {
    fn get_events(&self, ids: &[EventId]) -> Vec<IdentityEventNode>;
}
