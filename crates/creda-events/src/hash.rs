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
    /// Blake3 with 256-bit output (the launch default for per-event content integrity).
    Blake3,
    /// SHA-512 with 512-bit output (the DHT key derivation primitive, §6.1.6). Chosen for the
    /// network-wide routing role because (a) it's FIPS 180-4 validated under the OpenSSL FIPS
    /// module that ships in UBI / Hummingbird-FIPS, satisfying federal-program requirements
    /// without algorithm migration; and (b) the 512-bit output provides a 256-bit post-quantum
    /// security margin against Grover, leaving substantial headroom even if SHA-256's margin is
    /// later considered insufficient. Available for content_hash too if a peer's verifier policy
    /// admits it.
    Sha512,
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

    /// Compute a SHA-512 content hash over the given canonical bytes. The same primitive used by
    /// DHT key derivation (§6.1.6); shared here so a peer running with a FIPS-strict policy can
    /// author and verify content_hash under the same algorithm as its DHT routing.
    pub fn sha512(canonical_bytes: &[u8]) -> Self {
        Self {
            algorithm: HashAlgorithm::Sha512,
            digest: sha512_bytes(canonical_bytes).to_vec(),
        }
    }

    /// Constant-time-ish equality check that the given canonical bytes hash to this value.
    /// (Integrity check only — not a security boundary, so ordinary comparison is fine.)
    pub fn matches(&self, canonical_bytes: &[u8]) -> bool {
        match self.algorithm {
            HashAlgorithm::Blake3 => {
                blake3::hash(canonical_bytes).as_bytes().as_slice() == self.digest.as_slice()
            }
            HashAlgorithm::Sha512 => {
                sha512_bytes(canonical_bytes).as_slice() == self.digest.as_slice()
            }
        }
    }
}

/// Fixed-size SHA-512 hash. Pulled into creda-events so creda-net and downstream consumers
/// share a single SHA-512 implementation (and one place to swap to the OpenSSL FIPS provider
/// when a FIPS build is selected).
pub fn sha512_bytes(data: &[u8]) -> [u8; 64] {
    use sha2::Digest;
    let mut hasher = sha2::Sha512::new();
    hasher.update(data);
    let digest = hasher.finalize();
    let mut out = [0u8; 64];
    out.copy_from_slice(&digest);
    out
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

    #[test]
    fn sha512_is_deterministic_and_verifies() {
        let h1 = ContentHash::sha512(b"hello creda");
        let h2 = ContentHash::sha512(b"hello creda");
        assert_eq!(h1, h2);
        assert_eq!(h1.digest.len(), 64, "SHA-512 produces 64 bytes");
        assert!(h1.matches(b"hello creda"));
        assert!(!h1.matches(b"tampered"));
    }

    #[test]
    fn sha512_test_vector() {
        // NIST FIPS 180-4 known-answer for "abc": classic 64-byte digest.
        let expected_hex = "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
                            2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f";
        let h = sha512_bytes(b"abc");
        let got_hex: String = h.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(got_hex, expected_hex);
    }
}
