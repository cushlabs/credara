//! Backend-agnostic contract for the `Store` trait (spec §5.2.5).
//!
//! The same `contract` function runs against every backend, so MemoryStore and RocksdbStore
//! are proven to behave identically — which is the whole point of the trait (§7.4.1: swapping
//! backends touches no other crate).

use creda_events::ids::new_event_id;
use creda_events::{
    AttestPurpose, Demographics, EventId, EventPayload, IdentityEventNode, SignatureAlgorithm,
    SigningKey, TokenizedDate, TokenizedString, VerificationMethod,
};
use creda_store::{MemoryStore, Store};

fn key() -> SigningKey {
    SigningKey::generate(SignatureAlgorithm::Ed25519).unwrap()
}

fn assert_event(signer: &SigningKey, family_token: &str) -> IdentityEventNode {
    let demographics = Demographics {
        name_family: Some(vec![TokenizedString::from(family_token)]),
        date_of_birth: Some(TokenizedDate("tok-dob".into())),
        ..Default::default()
    };
    IdentityEventNode::create(
        EventPayload::Assert {
            demographics,
            verification_method: VerificationMethod::SelfReport,
        },
        vec![],
        signer,
        1,
        "2026-05-20T00:00:00Z",
        None,
    )
    .unwrap()
}

fn attest_child(signer: &SigningKey, parent: EventId) -> IdentityEventNode {
    IdentityEventNode::create(
        EventPayload::Attest {
            target_event_ids: vec![parent],
            purpose: AttestPurpose::Treatment,
        },
        vec![parent],
        signer,
        2,
        "2026-05-20T00:00:00Z",
        None,
    )
    .unwrap()
}

/// The full contract every backend must satisfy.
fn contract(store: &dyn Store) {
    // Empty store.
    assert!(store.all_event_ids().unwrap().is_empty());

    // Persist an Assert and read it back by UUID (primary index).
    let ka = key();
    let a1 = assert_event(&ka, "tok-smith");

    // A never-stored id is absent.
    let unknown = new_event_id(&a1.institution_id);
    assert!(store.get_event(&unknown).unwrap().is_none());
    assert!(!store.has_event(&unknown).unwrap());

    store.put_event(&a1).unwrap();
    assert_eq!(store.get_event(&a1.id).unwrap().as_ref(), Some(&a1));
    assert!(store.has_event(&a1.id).unwrap());
    assert!(store.all_event_ids().unwrap().contains(&a1.id));

    // Index 2: institution -> events.
    assert!(store
        .events_by_institution(&a1.institution_id)
        .unwrap()
        .contains(&a1.id));

    // Index 1: demographic token -> entry points.
    assert!(store.entry_points_by_token("tok-smith").unwrap().contains(&a1.id));
    assert!(store.entry_points_by_token("tok-dob").unwrap().contains(&a1.id));
    assert!(store.entry_points_by_token("absent-token").unwrap().is_empty());

    // Index 4: parent -> children.
    let child = attest_child(&ka, a1.id);
    store.put_event(&child).unwrap();
    assert_eq!(store.children_of(&a1.id).unwrap(), vec![child.id]);
    assert!(store.children_of(&child.id).unwrap().is_empty());

    // A second institution's events are scoped correctly.
    let kb = key();
    let b1 = assert_event(&kb, "tok-jones");
    store.put_event(&b1).unwrap();
    let a_events = store.events_by_institution(&a1.institution_id).unwrap();
    assert!(a_events.contains(&a1.id) && a_events.contains(&child.id));
    assert!(!a_events.contains(&b1.id));
    assert_eq!(store.events_by_institution(&b1.institution_id).unwrap(), vec![b1.id]);

    // put_event is idempotent.
    store.put_event(&a1).unwrap();
    assert_eq!(store.children_of(&a1.id).unwrap(), vec![child.id]);

    // Rebuilding indexes from the event store reproduces every index exactly.
    let tokens_before = store.entry_points_by_token("tok-smith").unwrap();
    let children_before = store.children_of(&a1.id).unwrap();
    let inst_before = store.events_by_institution(&a1.institution_id).unwrap();
    store.rebuild_indexes().unwrap();
    assert_eq!(store.entry_points_by_token("tok-smith").unwrap(), tokens_before);
    assert_eq!(store.children_of(&a1.id).unwrap(), children_before);
    assert_eq!(store.events_by_institution(&a1.institution_id).unwrap(), inst_before);

    // Results are returned in sorted (UUIDv7 creation-time) order.
    let ids = store.all_event_ids().unwrap();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    assert_eq!(ids.len(), 3);
}

#[test]
fn memory_store_satisfies_contract() {
    contract(&MemoryStore::new());
}

#[cfg(feature = "rocksdb")]
#[test]
fn rocksdb_store_satisfies_contract() {
    let dir = tempfile::tempdir().unwrap();
    let store = creda_store::RocksdbStore::open(dir.path()).unwrap();
    contract(&store);
}

#[cfg(feature = "rocksdb")]
#[test]
fn rocksdb_persists_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let id;
    {
        let store = creda_store::RocksdbStore::open(dir.path()).unwrap();
        let a = assert_event(&key(), "tok-persist");
        id = a.id;
        store.put_event(&a).unwrap();
    } // drop closes the database

    let store = creda_store::RocksdbStore::open(dir.path()).unwrap();
    assert!(store.get_event(&id).unwrap().is_some());
    assert!(store.entry_points_by_token("tok-persist").unwrap().contains(&id));
    // Indexes rebuilt from the on-disk event store still resolve.
    store.rebuild_indexes().unwrap();
    assert!(store.entry_points_by_token("tok-persist").unwrap().contains(&id));
}
