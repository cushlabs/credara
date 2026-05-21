//! Snapshot format for cold-start bootstrap (spec §6.2.5, §6.3.2, §7.3.3).
//!
//! A snapshot is a sorted sequence of canonical-CBOR-encoded events plus a manifest carrying the
//! snapshot timestamp, event count, and a Blake3 integrity hash. A new or replacement peer loads
//! the most recent snapshot, then runs anti-entropy to catch events created since (§6.2.5). The
//! format is transport-agnostic — the same bytes work whether fetched from object storage
//! (default) or streamed peer-to-peer (§6.3.2). Snapshots are institution-scoped (§6.2.5): they
//! contain only the events a peer holds, not the whole network.

use creda_events::{canonical, IdentityEventNode};
use creda_store::Store;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::util::blake3_32;

const SNAPSHOT_FORMAT_VERSION: u32 = 1;

/// Snapshot metadata (§6.3.2): timestamp, event count, and a Blake3 hash over the (sorted,
/// canonical-encoded) events for integrity verification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub format_version: u32,
    pub created_unix_secs: i64,
    pub event_count: u64,
    pub content_hash: Vec<u8>,
}

/// A complete snapshot: manifest plus the events, sorted by id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub manifest: SnapshotManifest,
    pub events: Vec<IdentityEventNode>,
}

impl Snapshot {
    /// Build a snapshot from a set of events (sorted by id) and a creation time.
    pub fn build(mut events: Vec<IdentityEventNode>, created_unix_secs: i64) -> Result<Self> {
        events.sort_by(|a, b| a.id.cmp(&b.id));
        let content_hash = events_hash(&events)?;
        Ok(Self {
            manifest: SnapshotManifest {
                format_version: SNAPSHOT_FORMAT_VERSION,
                created_unix_secs,
                event_count: events.len() as u64,
                content_hash,
            },
            events,
        })
    }

    /// Build a snapshot of everything in a store (§6.2.5: institution-scoped — a peer's store
    /// holds only the events it has).
    pub fn from_store(store: &dyn Store, created_unix_secs: i64) -> Result<Self> {
        let mut events = Vec::new();
        for id in store.all_event_ids()? {
            if let Some(node) = store.get_event(&id)? {
                events.push(node);
            }
        }
        Self::build(events, created_unix_secs)
    }

    /// Serialize to canonical-CBOR bytes for transport/storage.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(canonical::to_vec(self)?)
    }

    /// Parse and verify a snapshot from bytes. Fails if the integrity hash or event count does
    /// not match (tamper / corruption detection, §6.3.2).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let snapshot: Snapshot = canonical::from_slice(bytes)?;
        snapshot.verify()?;
        Ok(snapshot)
    }

    /// Verify the manifest against the events: event count and Blake3 content hash.
    pub fn verify(&self) -> Result<()> {
        if self.manifest.event_count as usize != self.events.len() {
            return Err(Error::Integrity(format!(
                "manifest event_count {} != actual {}",
                self.manifest.event_count,
                self.events.len()
            )));
        }
        let recomputed = events_hash(&self.events)?;
        if recomputed != self.manifest.content_hash {
            return Err(Error::Integrity("content hash mismatch".into()));
        }
        Ok(())
    }

    /// Load every event in the snapshot into a store. Returns the number of events loaded.
    /// Re-verifies integrity first.
    pub fn load_into_store(&self, store: &dyn Store) -> Result<usize> {
        self.verify()?;
        for event in &self.events {
            store.put_event(event)?;
        }
        Ok(self.events.len())
    }
}

/// Blake3 over the canonical encoding of the (already-sorted) events.
fn events_hash(events: &[IdentityEventNode]) -> Result<Vec<u8>> {
    let bytes = canonical::to_vec(events)?;
    Ok(blake3_32(&bytes).to_vec())
}
