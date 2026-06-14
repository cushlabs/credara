//! Canonical CBOR round-trip and determinism — spec §5.1.1.
//!
//! "The same logical event always produces the same byte sequence" is the property signature
//! verification depends on. These tests cover every event type.

mod common;

use creda_events::canonical;
use creda_events::IdentityEventNode;

#[test]
fn every_event_type_round_trips_through_canonical_cbor() {
    let key = common::ed_key();
    for node in common::one_of_each_event(&key) {
        let bytes = canonical::to_vec(&node).expect("serialize");
        let decoded: IdentityEventNode = canonical::from_slice(&bytes).expect("deserialize");
        assert_eq!(
            node, decoded,
            "round-trip changed the {:?} node",
            node.event_type
        );

        // Re-serializing the decoded node must yield byte-identical output.
        let bytes2 = canonical::to_vec(&decoded).expect("re-serialize");
        assert_eq!(
            bytes, bytes2,
            "canonical encoding not stable for {:?}",
            node.event_type
        );
    }
}

#[test]
fn contest_reason_canonical_bytes_match_cross_language_golden() {
    // Pins the exact canonical bytes of `EventPayload::Contest { target_link_id, reason }` so the
    // Rust (ciborium), Python cbor2 oracle, and Kotlin bridge encoder cannot drift apart. The same
    // hex is asserted in the bridge's ContestPayloadCborTest. ContestReason is the struct
    // {code, detail?} (§3.4.3) — code is kebab, detail is omitted when None.
    use creda_events::payload::ContestReasonCode;
    use creda_events::{ContestReason, EventId, EventPayload};

    let payload = EventPayload::Contest {
        target_link_id: EventId::from_bytes([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]),
        reason: ContestReason {
            code: ContestReasonCode::DistinctPatients,
            detail: Some("different humans".to_string()),
        },
    };
    let hex: String = canonical::to_vec(&payload)
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        hex, "a167436f6e74657374a266726561736f6ea264636f64657164697374696e63742d70617469656e74736664657461696c70646966666572656e742068756d616e736e7461726765745f6c696e6b5f696450000102030405060708090a0b0c0d0e0f",
        "Contest canonical CBOR drifted from the cross-language golden"
    );
}

#[test]
fn identical_logical_events_produce_identical_bytes() {
    // Two clones of the same node are the same logical event and must encode identically.
    let key = common::ed_key();
    let node = common::build_assert(&key);
    let clone = node.clone();
    assert_eq!(
        canonical::to_vec(&node).unwrap(),
        canonical::to_vec(&clone).unwrap()
    );
}

#[test]
fn content_hash_matches_payload_then_voids() {
    let key = common::ed_key();
    let mut node = common::build_assert(&key);
    assert_eq!(node.verify_content_hash(), Some(true));

    node.void_content_hash();
    assert!(node.content_hash.is_none());
    assert!(node.content_hash_voided);
    assert_eq!(
        node.verify_content_hash(),
        None,
        "voided hash should report None"
    );
}
