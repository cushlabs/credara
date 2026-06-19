//! Tombstoned-husk reduction (§3.4.6): scrubbing a node strips its demographic PII while keeping
//! the structural envelope (id, type, parents, institution), and the result is treated as "no
//! content hash" rather than a hash mismatch — so the husk survives reads but discloses nothing.

mod common;

use creda_events::{Demographics, EventPayload};

#[test]
fn husk_strips_demographics_and_voids_the_hash() {
    let key = common::ed_key();
    let assert = common::build_assert(&key);

    // Precondition: a real Assert carries (tokenized) demographics and a valid content hash.
    assert!(assert.carries_demographics());
    assert_eq!(assert.verify_content_hash(), Some(true));
    match &assert.payload {
        EventPayload::Assert { demographics, .. } => assert!(!demographics.is_empty()),
        _ => panic!("expected an Assert"),
    }

    let id = assert.id;
    let parents = assert.parent_ids.clone();
    let institution = assert.institution_id.clone();
    let event_type = assert.event_type;

    let husk = assert.into_tombstoned_husk();

    // The envelope is preserved — references and audit structure are intact.
    assert_eq!(husk.id, id);
    assert_eq!(husk.parent_ids, parents);
    assert_eq!(husk.institution_id, institution);
    assert_eq!(husk.event_type, event_type);

    // The PII is gone and the hash is voided: None (no usable hash), never Some(false).
    assert!(husk.content_hash_voided);
    assert_eq!(husk.verify_content_hash(), None);
    match &husk.payload {
        EventPayload::Assert { demographics, .. } => {
            assert_eq!(*demographics, Demographics::default())
        }
        _ => panic!("a husk must remain an Assert structurally"),
    }
}

#[test]
fn husk_is_a_noop_for_events_without_demographics() {
    let key = common::ed_key();
    // The Tombstone event itself carries no PII; scrubbing it must not void its content hash.
    let tombstone = common::one_of_each_event(&key)
        .into_iter()
        .find(|e| matches!(e.payload, EventPayload::Tombstone { .. }))
        .expect("one_of_each_event includes a Tombstone");

    assert!(!tombstone.carries_demographics());
    let before = tombstone.verify_content_hash();
    let husk = tombstone.into_tombstoned_husk();

    assert!(
        !husk.content_hash_voided,
        "a non-demographic event is not voided"
    );
    assert_eq!(husk.verify_content_hash(), before);
}
