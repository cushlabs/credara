//! Snapshot format: round-trip, integrity verification, and Store load/unload (spec §6.2.5,
//! §6.3.2). (Bucketing, anti-entropy, and gossip dedup are covered by unit tests inside their
//! modules.)

use creda_events::{
    Demographics, EventPayload, IdentityEventNode, SignatureAlgorithm, SigningKey, TokenizedDate,
    VerificationMethod,
};
use creda_net::Snapshot;
use creda_store::{MemoryStore, Store};

fn assert_event(dob: &str) -> IdentityEventNode {
    let key = SigningKey::generate(SignatureAlgorithm::Ed25519).unwrap();
    IdentityEventNode::create(
        EventPayload::Assert {
            demographics: Demographics {
                date_of_birth: Some(TokenizedDate(dob.into())),
                ..Default::default()
            },
            verification_method: VerificationMethod::GovernmentPhotoId,
        },
        vec![],
        &key,
        1,
        "2026-05-20T00:00:00Z",
        None,
    )
    .unwrap()
}

#[test]
fn snapshot_round_trips_and_verifies() {
    let events = vec![assert_event("a"), assert_event("b"), assert_event("c")];
    let snap = Snapshot::build(events.clone(), 1_700_000_000).unwrap();
    assert_eq!(snap.manifest.event_count, 3);
    snap.verify().unwrap();

    let bytes = snap.to_bytes().unwrap();
    let decoded = Snapshot::from_bytes(&bytes).unwrap(); // from_bytes verifies integrity
    assert_eq!(snap, decoded);
}

#[test]
fn snapshot_is_sorted_by_id() {
    let events = vec![assert_event("a"), assert_event("b"), assert_event("c")];
    let snap = Snapshot::build(events, 1).unwrap();
    let ids: Vec<_> = snap.events.iter().map(|e| e.id).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
}

#[test]
fn tampered_snapshot_fails_verification() {
    let events = vec![assert_event("a"), assert_event("b")];
    let mut snap = Snapshot::build(events, 1).unwrap();
    // Mutate the events without updating the manifest hash/count.
    snap.events.pop();
    assert!(snap.verify().is_err(), "event-count mismatch must fail verification");

    // Hash mismatch with matching count.
    let mut snap2 = Snapshot::build(vec![assert_event("x"), assert_event("y")], 1).unwrap();
    snap2.events[0] = assert_event("z"); // count unchanged, content changed
    assert!(snap2.verify().is_err(), "content-hash mismatch must fail verification");
}

#[test]
fn snapshot_moves_events_between_stores() {
    let source = MemoryStore::new();
    let events = vec![assert_event("a"), assert_event("b"), assert_event("c")];
    for e in &events {
        source.put_event(e).unwrap();
    }

    let snap = Snapshot::from_store(&source, 1_700_000_000).unwrap();
    assert_eq!(snap.manifest.event_count, 3);

    // Load into a fresh store and confirm every event arrived.
    let dest = MemoryStore::new();
    let loaded = snap.load_into_store(&dest).unwrap();
    assert_eq!(loaded, 3);
    for e in &events {
        assert!(dest.has_event(&e.id).unwrap(), "event {} should be present after load", e.id);
    }
    assert_eq!(dest.all_event_ids().unwrap().len(), 3);
}
