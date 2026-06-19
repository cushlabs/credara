//! Right-to-be-forgotten (§3.4.6) is a real scrub, not a projection trick. These tests assert the
//! integrity-critical properties: applying a `Tombstone` physically reduces its targets to husks in
//! the store, the scrubbed value can no longer be located by its demographic token, a re-received
//! original can never resurrect the PII, an out-of-order (tombstone-before-target) delivery still
//! scrubs on arrival, and `open()` self-heals a store that crashed before the scrub ran.

use creda_core::{CredaConfig, CredaCore, InMemorySigner, Ingest, KeyRegistry, PostureSetting};
use creda_events::{
    Demographics, EventId, EventPayload, TokenizedDate, TokenizedString, TombstoneBasis,
    VerificationMethod, VerifyingKey,
};
use creda_store::{MemoryStore, Store};

fn assert_payload() -> EventPayload {
    EventPayload::Assert {
        demographics: Demographics {
            name_family: Some(vec![TokenizedString::from("tok-smith")]),
            date_of_birth: Some(TokenizedDate("tok-1980".into())),
            ..Default::default()
        },
        verification_method: VerificationMethod::GovernmentPhotoId,
    }
}

fn tombstone_of(target: EventId) -> EventPayload {
    EventPayload::Tombstone {
        target_event_ids: vec![target],
        legal_basis: TombstoneBasis::RightToBeForgotten,
    }
}

fn core_with_signer() -> (CredaCore, VerifyingKey) {
    let signer = InMemorySigner::generate().unwrap();
    let vk = signer.verifying_key();
    let config = CredaConfig {
        default_posture: PostureSetting::TreatmentPresumed,
        ..Default::default()
    };
    let core = CredaCore::new(Box::new(MemoryStore::new()), Box::new(signer), config);
    (core, vk)
}

#[test]
fn tombstone_scrubs_stored_pii_and_unindexes_the_token() {
    let (core, _vk) = core_with_signer();
    let assert = core.create_event(assert_payload(), vec![]).unwrap();

    // Before: the demographic token resolves to the event.
    let before = core.match_by_tokens(&["tok-smith".to_string()]).unwrap();
    assert!(before.contains(&assert.id));

    core.create_event(tombstone_of(assert.id), vec![assert.id])
        .unwrap();

    // After: the stored event is a husk — PII gone, content hash voided, envelope intact.
    let husk = core
        .get_event(&assert.id)
        .unwrap()
        .expect("husk is retained");
    assert!(husk.content_hash_voided, "tombstoned target must be a husk");
    match husk.payload {
        EventPayload::Assert { demographics, .. } => assert!(demographics.is_empty()),
        _ => panic!("a husk stays an Assert structurally"),
    }

    // And the token can no longer locate it (the index was rebuilt over the scrubbed store).
    let after = core.match_by_tokens(&["tok-smith".to_string()]).unwrap();
    assert!(
        after.is_empty(),
        "a tombstoned value must not be findable by token"
    );
}

#[test]
fn a_re_received_original_cannot_resurrect_a_husk() {
    let (core, vk) = core_with_signer();
    let assert = core.create_event(assert_payload(), vec![]).unwrap();
    core.create_event(tombstone_of(assert.id), vec![assert.id])
        .unwrap();
    assert!(
        core.get_event(&assert.id)
            .unwrap()
            .unwrap()
            .content_hash_voided
    );

    // Re-delivery of the pristine original is an idempotent no-op (we already hold the husk).
    let reg = KeyRegistry::from_keys([vk]);
    let outcome = core.ingest_event(assert.clone(), &reg).unwrap();
    assert_eq!(outcome, Ingest::AlreadyHave);

    let still = core.get_event(&assert.id).unwrap().unwrap();
    assert!(still.content_hash_voided, "the husk must stay scrubbed");
}

#[test]
fn tombstone_arriving_before_its_target_scrubs_on_arrival() {
    let (origin, origin_vk) = core_with_signer();
    let assert = origin.create_event(assert_payload(), vec![]).unwrap();
    let tombstone = origin
        .create_event(tombstone_of(assert.id), vec![assert.id])
        .unwrap();

    let (local, _vk) = core_with_signer();
    let reg = KeyRegistry::from_keys([origin_vk]);

    // The tombstone is replicated BEFORE its target is held locally.
    assert_eq!(
        local.ingest_event(tombstone, &reg).unwrap(),
        Ingest::Accepted
    );
    // The target arrives afterward and must land as a husk, never as recoverable PII.
    assert_eq!(
        local.ingest_event(assert.clone(), &reg).unwrap(),
        Ingest::Accepted
    );

    let stored = local.get_event(&assert.id).unwrap().unwrap();
    assert!(
        stored.content_hash_voided,
        "an out-of-order target must be husked on arrival"
    );
    let hits = local.match_by_tokens(&["tok-smith".to_string()]).unwrap();
    assert!(hits.is_empty());
}

#[test]
fn open_recovers_and_self_heals_tombstones_from_the_store() {
    // Mint signed originals with a throwaway core; the returned nodes are the pristine originals.
    let (minting, _vk) = core_with_signer();
    let assert = minting.create_event(assert_payload(), vec![]).unwrap();
    let tombstone = minting
        .create_event(tombstone_of(assert.id), vec![assert.id])
        .unwrap();

    // Simulate a store that persisted both events but crashed before the scrub ran.
    let crashed = MemoryStore::new();
    crashed.put_event(&assert).unwrap();
    crashed.put_event(&tombstone).unwrap();
    let pre = crashed.get_event(&assert.id).unwrap().unwrap();
    assert!(
        !pre.content_hash_voided,
        "precondition: target is un-scrubbed"
    );

    // open() must re-apply every stored tombstone: the target is husked and unindexed on boot.
    let config = CredaConfig {
        default_posture: PostureSetting::TreatmentPresumed,
        ..Default::default()
    };
    let recovered = CredaCore::open(
        Box::new(crashed),
        Box::new(InMemorySigner::generate().unwrap()),
        config,
    )
    .unwrap();

    let healed = recovered.get_event(&assert.id).unwrap().unwrap();
    assert!(
        healed.content_hash_voided,
        "open() must self-heal an un-scrubbed target"
    );
    let hits = recovered
        .match_by_tokens(&["tok-smith".to_string()])
        .unwrap();
    assert!(hits.is_empty());
}
