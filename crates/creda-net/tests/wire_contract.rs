//! Cross-peer wire/routing contract — exact-value golden vectors (spec §6.1.6, §6.2.2, §6.2.4).
//!
//! The existing module unit tests prove these functions are *deterministic* (same input → same
//! output within one build). These tests pin the *actual values* so the contract can't silently
//! drift across versions or implementations: every peer must derive an identical DHT key, bucket,
//! gossip topic, and batch envelope, or the network fragments (peers stop finding each other —
//! a silent identity-continuity failure). A change to the SHA-512 input framing (field order or
//! the 0x1f separator), the `mod 1024` derivation, the topic prefix, or the `GossipBatch` serde
//! shape breaks these here before it breaks two real peers in the field.
//!
//! Golden values produced by an independent Python oracle (hashlib SHA-512 + cbor2 canonical=True),
//! mirroring the ciborium 0.2.2 rules documented in the bridge CBOR tests.

use creda_net::bucketing::dht_key;
use creda_net::{bucket_of, topic_for_key, GossipBatch};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn dht_key_pins_input_framing_and_sha512() {
    // SHA-512("smith" 0x1f "1980-01-01" 0x1f "female").
    let k = dht_key("smith", "1980-01-01", "female");
    assert_eq!(
        hex(&k),
        "78f13cead63d96450f18d87c9593a0adc75901f3bf00b210d477635bcab0bab9a977a50a7689acdd6b6494f73b0782236ce3ae0b4b8e3c92648fc7d1e3c0be45",
        "DHT key derivation drifted — every peer must compute this identically (§6.1.6)"
    );
}

#[test]
fn bucket_and_topic_are_pinned() {
    let k = dht_key("smith", "1980-01-01", "female");
    assert_eq!(bucket_of(&k), 926, "bucket derivation (SHA-512(key) mod 1024) drifted (§6.2.4)");
    assert_eq!(topic_for_key(&k), "creda/v1/subgraph/926", "gossip topic id drifted (§6.2.4)");
}

#[test]
fn gossip_batch_envelope_matches_golden() {
    // The bytes peers actually exchange. Empty events isolate the envelope framing (sender is a
    // Vec<u8> → CBOR array of ints, not a bstr; canonical key order events < sender < sequence).
    // Individual event canonicality is pinned separately in creda-events.
    let batch = GossipBatch::new(vec![1, 2, 3], 7, vec![]);
    assert_eq!(
        hex(&batch.to_bytes().unwrap()),
        "a3666576656e7473806673656e646572830102036873657175656e636507",
        "GossipBatch wire envelope drifted (§6.2.2)"
    );
    // Round-trips back to an identical batch.
    let decoded = GossipBatch::from_bytes(&batch.to_bytes().unwrap()).unwrap();
    assert_eq!(decoded, batch);
}
