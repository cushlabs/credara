//! M5 engine: the composed peer operations over an in-memory store (no gRPC, no network, no
//! RocksDB). Confirms create→get, effective-identity, token match, authorization, and
//! snapshot round-trip all work end-to-end through `CredaCore`.

use creda_core::{CredaConfig, CredaCore, Ingest, InMemorySigner, KeyRegistry, PostureSetting};
use creda_events::{
    AuthorizationScope, CertificateFingerprint, Demographics, EventPayload, GrantAudience,
    GrantPurpose, TokenizedDate, TokenizedString, UseMode, VerificationMethod,
};
use creda_graph::{AuthorizationQuery, RequesterContext};
use creda_store::MemoryStore;

fn core_with(posture: PostureSetting) -> CredaCore {
    let config = CredaConfig { default_posture: posture, ..Default::default() };
    let store = Box::new(MemoryStore::new());
    let signer = Box::new(InMemorySigner::generate().unwrap());
    CredaCore::new(store, signer, config)
}

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

#[test]
fn create_then_get_round_trips() {
    let core = core_with(PostureSetting::TreatmentPresumed);
    let event = core.create_event(assert_payload(), vec![]).unwrap();
    let fetched = core.get_event(&event.id).unwrap();
    assert_eq!(fetched.as_ref(), Some(&event));
    assert_eq!(core.event_count().unwrap(), 1);
    // Read-your-writes: event is present immediately.
    assert_eq!(core.institution_id(), event.institution_id);
}

#[test]
fn effective_identity_and_token_match() {
    let core = core_with(PostureSetting::TreatmentPresumed);
    let event = core.create_event(assert_payload(), vec![]).unwrap();

    let ei = core.effective_identity(&[event.id]).unwrap();
    let dob = ei.field(&creda_graph::FieldKey::DateOfBirth).unwrap();
    assert_eq!(dob.values[0].value, "tok-1980");

    // MatchByTokens finds the entry point via a demographic token.
    let hits = core.match_by_tokens(&["tok-smith".to_string()]).unwrap();
    assert!(hits.contains(&event.id));
    assert!(core.match_by_tokens(&["nope".to_string()]).unwrap().is_empty());
}

#[test]
fn authorization_grant_authorizes_request() {
    let core = core_with(PostureSetting::DenyByDefault);
    let assert = core.create_event(assert_payload(), vec![]).unwrap();

    let requester = CertificateFingerprint::from_public_key_bytes(b"requesting-institution");
    let grant = EventPayload::AuthorizationGrant {
        scope: AuthorizationScope::default(),
        audience: GrantAudience::InstitutionId(requester.clone()),
        purpose: GrantPurpose::Treatment,
        expiration: None,
        volume_constraints: None,
        use_mode: UseMode::ReadAndRely,
    };
    core.create_event(grant, vec![assert.id]).unwrap();

    let query = AuthorizationQuery {
        requester: RequesterContext::new(requester),
        purpose: GrantPurpose::Treatment,
        use_mode: UseMode::ReadOnly,
        requested_event_types: vec![],
        requested_segments: vec![],
        requested_data_categories: vec![],
    };
    let decision = core.evaluate_authorization(&[assert.id], &query).unwrap();
    assert!(decision.authorized);
    assert_eq!(decision.covering_grants.len(), 1);
}

#[test]
fn deny_by_default_without_grant() {
    let core = core_with(PostureSetting::DenyByDefault);
    let assert = core.create_event(assert_payload(), vec![]).unwrap();
    let query = AuthorizationQuery {
        requester: RequesterContext::new(CertificateFingerprint::from_public_key_bytes(b"x")),
        purpose: GrantPurpose::Treatment,
        use_mode: UseMode::ReadOnly,
        requested_event_types: vec![],
        requested_segments: vec![],
        requested_data_categories: vec![],
    };
    let decision = core.evaluate_authorization(&[assert.id], &query).unwrap();
    assert!(!decision.authorized);
}

#[test]
fn snapshot_round_trips_between_engines() {
    let source = core_with(PostureSetting::TreatmentPresumed);
    let a = source.create_event(assert_payload(), vec![]).unwrap();
    let b = source.create_event(assert_payload(), vec![a.id]).unwrap();
    let bytes = source.snapshot_bytes().unwrap();

    // Load the snapshot into a fresh engine and confirm both events arrived.
    let dest = core_with(PostureSetting::TreatmentPresumed);
    let loaded = dest.load_snapshot(&bytes).unwrap();
    assert_eq!(loaded, 2);
    assert!(dest.get_event(&a.id).unwrap().is_some());
    assert!(dest.get_event(&b.id).unwrap().is_some());
    assert_eq!(dest.event_count().unwrap(), 2);
}

// ---- Synthetic-only guardrail (closed-pilot safety, docs/PILOT.md) -------------------------

fn synthetic_core() -> CredaCore {
    let config = CredaConfig { synthetic_only: true, ..Default::default() };
    CredaCore::new(
        Box::new(MemoryStore::new()),
        Box::new(InMemorySigner::generate().unwrap()),
        config,
    )
}

#[test]
fn synthetic_only_auto_tags_local_creates() {
    // A synthetic-only peer tags everything it creates; a normal peer does not.
    let ev = synthetic_core().create_event(assert_payload(), vec![]).unwrap();
    assert!(ev.is_test_data(), "synthetic_only must auto-tag local events as test_data");

    let ev2 = core_with(PostureSetting::TreatmentPresumed)
        .create_event(assert_payload(), vec![])
        .unwrap();
    assert!(!ev2.is_test_data(), "normal peer must NOT tag events");
}

#[test]
fn synthetic_only_refuses_untagged_ingest_but_accepts_tagged() {
    let local = synthetic_core();

    // A validly-signed, admitted, but UNTAGGED event from a normal peer is refused.
    let remote_signer = InMemorySigner::generate().unwrap();
    let remote_vk = remote_signer.verifying_key();
    let remote = CredaCore::new(
        Box::new(MemoryStore::new()),
        Box::new(remote_signer),
        CredaConfig::default(),
    );
    let untagged = remote.create_event(assert_payload(), vec![]).unwrap();
    assert!(!untagged.is_test_data());
    let reg = KeyRegistry::from_keys([remote_vk]);
    match local.ingest_event(untagged, &reg).unwrap() {
        Ingest::Rejected(msg) => assert!(msg.contains("synthetic-only"), "wrong reason: {msg}"),
        _ => panic!("expected Rejected for an untagged event on a synthetic-only peer"),
    }

    // A tagged event from a synthetic peer is accepted.
    let syn_signer = InMemorySigner::generate().unwrap();
    let syn_vk = syn_signer.verifying_key();
    let syn_remote = CredaCore::new(
        Box::new(MemoryStore::new()),
        Box::new(syn_signer),
        CredaConfig { synthetic_only: true, ..Default::default() },
    );
    let tagged = syn_remote.create_event(assert_payload(), vec![]).unwrap();
    assert!(tagged.is_test_data());
    let reg2 = KeyRegistry::from_keys([syn_vk]);
    assert!(matches!(local.ingest_event(tagged, &reg2).unwrap(), Ingest::Accepted));
}
