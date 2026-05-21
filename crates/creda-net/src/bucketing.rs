//! Topic bucketing and DHT key derivation (spec §6.1.6, §6.2.4).
//!
//! A naive design would use one gossipsub topic per patient subgraph, but topic cardinality in
//! the millions stresses gossipsub's mesh management. Creda instead hashes each subgraph's DHT
//! key into one of **1,024 stable topic buckets** (§6.2.4); peers subscribe to the buckets
//! covering their patient population and filter locally. This trades a little unnecessary
//! received traffic for dramatically lower topic cardinality.
//!
//! All functions here are pure and deterministic — the same demographics always map to the same
//! key, bucket, and topic on every peer, which is what makes the DHT and gossip routing work.

use creda_events::Demographics;

use crate::util::blake3_32;

/// Number of topic buckets (§6.2.4). A protocol-level constant — changing it requires a
/// coordinated network upgrade. Tunable per the spec's discussion, but fixed for the wire.
pub const BUCKET_COUNT: u64 = 1024;

/// Topic id prefix (§6.2.4): `creda/v1/subgraph/{bucket}`.
pub const TOPIC_PREFIX: &str = "creda/v1/subgraph/";

/// A subgraph's DHT key (§6.1.6): `Blake3(tokenize(family) || tokenize(dob) || tokenize(sex))`.
pub type DhtKey = [u8; 32];

const SEP: u8 = 0x1f; // unit separator between token fields

/// Derive the primary DHT key from the three core demographic fields (family name, date of
/// birth, sex), per §6.1.6. Returns `None` if any of the three is absent — the primary key
/// cannot be formed (institutions may derive secondary keys from other field combinations).
///
/// Inputs are already-tokenized opaque strings; this crate never sees raw PII.
pub fn dht_key(family_token: &str, dob_token: &str, sex_token: &str) -> DhtKey {
    let mut buf = Vec::new();
    buf.extend_from_slice(family_token.as_bytes());
    buf.push(SEP);
    buf.extend_from_slice(dob_token.as_bytes());
    buf.push(SEP);
    buf.extend_from_slice(sex_token.as_bytes());
    blake3_32(&buf)
}

/// Derive the primary DHT key from a demographics record, if family name, DOB, and sex are all
/// present. Family-name tokens are joined deterministically.
pub fn dht_key_from_demographics(d: &Demographics) -> Option<DhtKey> {
    let family = d.name_family.as_ref()?;
    let dob = d.date_of_birth.as_ref()?;
    let sex = d.sex?;
    let family_joined = family
        .iter()
        .map(|t| t.0.as_str())
        .collect::<Vec<_>>()
        .join("\u{1f}");
    Some(dht_key(&family_joined, &dob.0, gender_token(sex)))
}

/// The topic bucket for a DHT key: `Blake3(dht_key) mod 1024` (§6.2.4).
///
/// `mod 1024` over the big-endian hash integer is its low 10 bits, i.e. the last two bytes
/// masked to 10 bits.
pub fn bucket_of(key: &DhtKey) -> u64 {
    let h = blake3_32(key);
    let low = ((h[30] as u64) << 8) | (h[31] as u64);
    low & (BUCKET_COUNT - 1)
}

/// The gossipsub topic id for a bucket.
pub fn topic_for_bucket(bucket: u64) -> String {
    format!("{TOPIC_PREFIX}{bucket}")
}

/// The gossipsub topic id for a DHT key (its bucket's topic).
pub fn topic_for_key(key: &DhtKey) -> String {
    topic_for_bucket(bucket_of(key))
}

fn gender_token(g: creda_events::AdministrativeGender) -> &'static str {
    use creda_events::AdministrativeGender::*;
    match g {
        Male => "male",
        Female => "female",
        Other => "other",
        Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_and_bucket_are_deterministic() {
        let k1 = dht_key("smith", "1980-01-01", "female");
        let k2 = dht_key("smith", "1980-01-01", "female");
        assert_eq!(k1, k2);
        assert_eq!(bucket_of(&k1), bucket_of(&k2));
        assert_eq!(topic_for_key(&k1), topic_for_key(&k2));
    }

    #[test]
    fn different_demographics_usually_differ() {
        let a = dht_key("smith", "1980-01-01", "female");
        let b = dht_key("jones", "1990-02-02", "male");
        assert_ne!(a, b);
    }

    #[test]
    fn buckets_are_in_range() {
        for i in 0..5000u64 {
            let k = dht_key(&format!("fam{i}"), "1980-01-01", "male");
            assert!(bucket_of(&k) < BUCKET_COUNT);
        }
    }

    #[test]
    fn topic_format() {
        assert_eq!(topic_for_bucket(7), "creda/v1/subgraph/7");
    }
}
