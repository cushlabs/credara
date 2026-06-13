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
