//! Content hashing — spec §5.1.2 (PQC: hash function).
//!
//! Blake3 (256-bit) gives a 128-bit post-quantum security margin against Grover search,
//! meeting NIST's recommended floor. The content hash is an **optional integrity check**,
//! never load-bearing for addressing or traversal (those use UUIDs, §3.4.6). It is voided
//! after tombstoning, when the node's content is legitimately replaced.
//!
//! The hash carries an algorithm identifier so the floor can be raised later without a
//! schema change (algorithm agility, §5.1.2).

use serde::{Deserialize, Serialize};

/// Hash algorithm identifier, stored alongside the digest for agility.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HashAlgorithm {
    /// Blake3 with 256-bit output (the launch default).
    Blake3,
}

/// A content hash: algorithm identifier plus digest bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentHash {
    pub algorithm: HashAlgorithm,
    pub digest: Vec<u8>,
}

impl ContentHash {
    /// Compute a Blake3 content hash over the given canonical bytes.
    pub fn blake3(canonical_bytes: &[u8]) -> Self {
        Self {
            algorithm: HashAlgorithm::Blake3,
            digest: blake3::hash(canonical_bytes).as_bytes().to_vec(),
        }
    }

    /// Constant-time-ish equality check that the given canonical bytes hash to this value.
    /// (Integrity check only — not a security boundary, so ordinary comparison is fine.)
    pub fn matches(&self, canonical_bytes: &[u8]) -> bool {
        match self.algorithm {
            HashAlgorithm::Blake3 => {
                blake3::hash(canonical_bytes).as_bytes().as_slice() == self.digest.as_slice()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic_and_verifies() {
        let h1 = ContentHash::blake3(b"hello creda");
        let h2 = ContentHash::blake3(b"hello creda");
        assert_eq!(h1, h2);
        assert!(h1.matches(b"hello creda"));
        assert!(!h1.matches(b"tampered"));
    }
}
