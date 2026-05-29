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

/// SHA-512 of `data` as a fixed 64-byte array. Used by DHT key derivation (§6.1.6) and other
/// network-wide routing primitives that need FIPS validation and a 256-bit post-quantum margin.
/// Reuses creda-events' `sha512_bytes` so the whole system shares one SHA-512 implementation
/// (and one place to swap to the OpenSSL FIPS provider when a FIPS build is selected).
pub(crate) fn sha512_64(data: &[u8]) -> [u8; 64] {
    creda_events::sha512_bytes(data)
}
