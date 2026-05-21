//! Anti-entropy: Merkle root over the UUID set, and reconciliation deltas (spec §6.1.8).
//!
//! Two peers determine whether their copies of a subgraph are in sync by comparing a Merkle
//! root computed over the **sorted set of event UUIDs** — deliberately NOT over event contents
//! (§6.1.8). Hashing only UUIDs means tombstoning (which mutates content and voids the content
//! hash, §7.2.2) does not cause two peers holding the same event set to diverge. If roots
//! differ, peers exchange UUID sets and transfer the events each side is missing — structurally
//! the same delta-transfer Git uses on fetch.
//!
//! This module is pure: given UUID sets it computes roots and deltas. Actually moving events is
//! the transport's job ([`crate::transport::NetworkTransport`]).

use std::collections::BTreeSet;

use creda_events::EventId;

use crate::util::blake3_32;

/// A Merkle root over a subgraph's UUID set.
pub type MerkleRoot = [u8; 32];

/// Compute the Merkle root over the sorted set of event UUIDs (§6.1.8).
///
/// Leaves are `Blake3(uuid_bytes)` in sorted UUID order; internal nodes are
/// `Blake3(left || right)`; an odd node at a level is promoted unchanged. The empty set hashes
/// to all-zero. Two peers with the same UUID set always compute the same root, independent of
/// content (tombstoning included).
pub fn merkle_root(uuids: &BTreeSet<EventId>) -> MerkleRoot {
    if uuids.is_empty() {
        return [0u8; 32];
    }
    // Leaf hashes in sorted UUID order (BTreeSet iterates sorted).
    let mut level: Vec<[u8; 32]> = uuids.iter().map(|id| blake3_32(id.as_bytes())).collect();

    while level.len() > 1 {
        let mut next: Vec<[u8; 32]> = Vec::with_capacity(level.len().div_ceil(2));
        let mut i = 0;
        while i < level.len() {
            if i + 1 < level.len() {
                let mut buf = [0u8; 64];
                buf[..32].copy_from_slice(&level[i]);
                buf[32..].copy_from_slice(&level[i + 1]);
                next.push(blake3_32(&buf));
            } else {
                // Odd node out: promote unchanged.
                next.push(level[i]);
            }
            i += 2;
        }
        level = next;
    }
    level[0]
}

/// Whether two roots indicate the peers are in sync.
pub fn in_sync(a: &MerkleRoot, b: &MerkleRoot) -> bool {
    a == b
}

/// The result of comparing two UUID sets: which events each side must fetch from the other.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Reconciliation {
    /// UUIDs the remote peer has that the local peer is missing (local should fetch these).
    pub local_missing: Vec<EventId>,
    /// UUIDs the local peer has that the remote peer is missing (local should send these).
    pub remote_missing: Vec<EventId>,
}

impl Reconciliation {
    /// True when both sides already hold the same event set.
    pub fn is_converged(&self) -> bool {
        self.local_missing.is_empty() && self.remote_missing.is_empty()
    }
}

/// Compute the reconciliation delta between a local and remote UUID set (§6.1.8 step 4).
/// Results are sorted (UUIDv7 / creation-time order).
pub fn reconcile(local: &BTreeSet<EventId>, remote: &BTreeSet<EventId>) -> Reconciliation {
    Reconciliation {
        local_missing: remote.difference(local).copied().collect(),
        remote_missing: local.difference(remote).copied().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use creda_events::ids::{new_event_id, CertificateFingerprint};

    fn id() -> EventId {
        new_event_id(&CertificateFingerprint::from_public_key_bytes(b"t"))
    }

    fn set(ids: &[EventId]) -> BTreeSet<EventId> {
        ids.iter().copied().collect()
    }

    #[test]
    fn root_is_deterministic_and_order_independent() {
        let a = id();
        let b = id();
        let c = id();
        let s1 = set(&[a, b, c]);
        let s2 = set(&[c, a, b]); // different insertion order, same set
        assert_eq!(merkle_root(&s1), merkle_root(&s2));
    }

    #[test]
    fn different_sets_differ_and_empty_is_zero() {
        let a = id();
        let b = id();
        assert_ne!(merkle_root(&set(&[a])), merkle_root(&set(&[a, b])));
        assert_eq!(merkle_root(&BTreeSet::new()), [0u8; 32]);
    }

    #[test]
    fn in_sync_matches_equal_sets() {
        let a = id();
        let b = id();
        let local = set(&[a, b]);
        let remote = set(&[a, b]);
        assert!(in_sync(&merkle_root(&local), &merkle_root(&remote)));
        assert!(reconcile(&local, &remote).is_converged());
    }

    #[test]
    fn reconcile_identifies_both_directions() {
        let shared = id();
        let only_local = id();
        let only_remote = id();
        let local = set(&[shared, only_local]);
        let remote = set(&[shared, only_remote]);

        let r = reconcile(&local, &remote);
        assert_eq!(r.local_missing, vec![only_remote]);
        assert_eq!(r.remote_missing, vec![only_local]);
        assert!(!r.is_converged());
        // Roots must differ when sets differ.
        assert!(!in_sync(&merkle_root(&local), &merkle_root(&remote)));
    }
}
