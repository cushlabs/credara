//! Gossip batch envelope, batching policy, and bounded deduplication (spec §6.1.4, §6.2.2).
//!
//! Gossip carries **batches** of events, not individual events (§6.2.2), assembled on a dual
//! trigger (time or size) to amortize Noise/gossipsub/round-trip overhead. Receivers dedup at
//! both the event-UUID level and the batch level using a bounded set (§6.1.4). These types are
//! transport-agnostic; the libp2p adapter publishes/receives them.

use std::collections::{HashSet, VecDeque};

use creda_events::{canonical, EventId, IdentityEventNode};
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// A batch of events propagated via gossip (§6.2.2). Carries the sender's peer id and a
/// per-sender sequence number so receivers can dedup whole batches as well as individual events.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GossipBatch {
    /// The sending peer's libp2p peer id, as bytes.
    pub sender: Vec<u8>,
    /// Per-sender monotonically increasing batch sequence number.
    pub sequence: u64,
    /// The batch's events, canonical-CBOR encoded on the wire.
    pub events: Vec<IdentityEventNode>,
}

impl GossipBatch {
    pub fn new(sender: Vec<u8>, sequence: u64, events: Vec<IdentityEventNode>) -> Self {
        Self { sender, sequence, events }
    }

    /// Serialize to canonical-CBOR bytes for the wire.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(canonical::to_vec(self)?)
    }

    /// Parse a batch from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Ok(canonical::from_slice(bytes)?)
    }
}

/// Batching policy (§6.2.2): flush when either trigger fires.
#[derive(Clone, Copy, Debug)]
pub struct BatchConfig {
    /// Flush when the batch reaches this many events.
    pub max_events: usize,
    /// Flush at least this often, regardless of batch size.
    pub flush_interval_ms: u64,
}

impl Default for BatchConfig {
    fn default() -> Self {
        // Spec §6.2.2 defaults.
        Self {
            max_events: 64,
            flush_interval_ms: 100,
        }
    }
}

/// A bounded, FIFO-evicting set for gossip deduplication (§6.1.4): "peers that have already seen
/// an event UUID ignore subsequent deliveries", with a bounded memory footprint. Tracks seen
/// keys in insertion order and evicts the oldest once capacity is exceeded.
pub struct SeenSet {
    capacity: usize,
    set: HashSet<Vec<u8>>,
    order: VecDeque<Vec<u8>>,
}

impl SeenSet {
    /// Create a dedup set holding up to `capacity` recent keys.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            set: HashSet::new(),
            order: VecDeque::new(),
        }
    }

    /// Record an event UUID; returns `true` if it had already been seen (caller should ignore
    /// the redelivery), `false` if it is new.
    pub fn seen_event(&mut self, id: &EventId) -> bool {
        self.mark(id.as_bytes().to_vec())
    }

    /// Record a (sender, batch-sequence) pair for batch-level dedup; returns `true` if already
    /// seen.
    pub fn seen_batch(&mut self, sender: &[u8], sequence: u64) -> bool {
        let mut key = Vec::with_capacity(sender.len() + 8);
        key.extend_from_slice(sender);
        key.extend_from_slice(&sequence.to_be_bytes());
        self.mark(key)
    }

    fn mark(&mut self, key: Vec<u8>) -> bool {
        if self.set.contains(&key) {
            return true;
        }
        if self.order.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.set.remove(&oldest);
            }
        }
        self.set.insert(key.clone());
        self.order.push_back(key);
        false
    }

    /// Number of keys currently tracked.
    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use creda_events::ids::{new_event_id, CertificateFingerprint};

    fn id() -> EventId {
        new_event_id(&CertificateFingerprint::from_public_key_bytes(b"t"))
    }

    #[test]
    fn dedup_reports_repeats() {
        let mut seen = SeenSet::new(100);
        let a = id();
        assert!(!seen.seen_event(&a), "first sighting is new");
        assert!(seen.seen_event(&a), "second sighting is a repeat");
    }

    #[test]
    fn dedup_evicts_oldest_beyond_capacity() {
        let mut seen = SeenSet::new(2);
        let a = id();
        let b = id();
        let c = id();
        assert!(!seen.seen_event(&a));
        assert!(!seen.seen_event(&b));
        assert!(!seen.seen_event(&c)); // evicts a
        assert_eq!(seen.len(), 2);
        assert!(!seen.seen_event(&a), "a was evicted, so it's new again");
    }

    #[test]
    fn batch_dedup_keys_on_sender_and_sequence() {
        let mut seen = SeenSet::new(100);
        assert!(!seen.seen_batch(b"peer1", 1));
        assert!(seen.seen_batch(b"peer1", 1));
        assert!(!seen.seen_batch(b"peer1", 2));
        assert!(!seen.seen_batch(b"peer2", 1));
    }
}
