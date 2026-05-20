//! Identifier types — spec §5.1.4 (UUIDv7) and §5.1 (institution fingerprint).
//!
//! Stable UUIDs, not content hashes, are the primary addressing scheme (§3.4.6): tombstoning
//! changes a node's content (and voids its content hash), but all references between events
//! use UUIDs, so the graph topology is unaffected by content mutation.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A Creda event identifier. UUIDv7 (RFC 9562): a Unix-millisecond timestamp in the high
/// bits gives natural time-ordering at the storage layer, with a random low component.
pub type EventId = Uuid;

/// Generate a new time-ordered event id, mixing the creating institution's identity into the
/// random component to make cross-institution collisions negligible (§5.1.4).
///
/// We keep UUIDv7's standard timestamp layout (so storage-layer time ordering is preserved)
/// and derive the random bits from `blake3(institution || fresh_entropy)`. Two institutions
/// generating events in the same millisecond therefore draw from disjoint-with-overwhelming-
/// probability random spaces.
pub fn new_event_id(institution: &CertificateFingerprint) -> EventId {
    let millis = unix_millis();
    let mut entropy = [0u8; 16];
    let mut rng = rand_core::OsRng;
    rand_core::RngCore::fill_bytes(&mut rng, &mut entropy);

    let mut hasher = blake3::Hasher::new();
    hasher.update(institution.as_bytes());
    hasher.update(&entropy);
    let digest = hasher.finalize();
    let d = digest.as_bytes();

    // 10 random bytes feed the UUIDv7 layout after the 48-bit timestamp + version/variant.
    let mut rand_bytes = [0u8; 10];
    rand_bytes.copy_from_slice(&d[..10]);

    uuid::Builder::from_unix_timestamp_millis(millis, &rand_bytes).into_uuid()
}

fn unix_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A UDAP certificate fingerprint identifying the institution (or patient key) that created
/// an event (§5.1). Stored as raw bytes; conventionally a hash of the certificate/public key.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CertificateFingerprint(pub Vec<u8>);

impl CertificateFingerprint {
    /// Wrap raw fingerprint bytes.
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Derive a fingerprint from public-key (or certificate) bytes via Blake3.
    pub fn from_public_key_bytes(public_key: &[u8]) -> Self {
        Self(blake3::hash(public_key).as_bytes().to_vec())
    }

    /// The raw fingerprint bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for CertificateFingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Render short hex for readability; fingerprints are not secret.
        let hex: String = self.0.iter().take(8).map(|b| format!("{b:02x}")).collect();
        write!(f, "CertificateFingerprint({hex}…)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_ids_are_version_7_and_unique() {
        let inst = CertificateFingerprint::from_public_key_bytes(b"institution-A");
        let a = new_event_id(&inst);
        let b = new_event_id(&inst);
        assert_ne!(a, b);
        assert_eq!(a.get_version_num(), 7);
        assert_eq!(b.get_version_num(), 7);
    }

    #[test]
    fn event_ids_are_time_ordered() {
        let inst = CertificateFingerprint::from_public_key_bytes(b"institution-A");
        let a = new_event_id(&inst);
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = new_event_id(&inst);
        // UUIDv7 sorts lexicographically in creation-time order.
        assert!(a < b, "expected earlier id to sort before later id");
    }

    #[test]
    fn fingerprint_is_stable() {
        let a = CertificateFingerprint::from_public_key_bytes(b"key");
        let b = CertificateFingerprint::from_public_key_bytes(b"key");
        assert_eq!(a, b);
    }
}
