//! Small internal helpers.

use creda_events::ContentHash;

/// Blake3 of `data` as a fixed 32-byte array. Reuses creda-events' Blake3 (the protocol hash,
/// §5.1.2) so the whole system shares one hash implementation and there is no second blake3
/// dependency to keep in sync.
pub(crate) fn blake3_32(data: &[u8]) -> [u8; 32] {
    let digest = ContentHash::blake3(data).digest;
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest[..32]);
    out
}
