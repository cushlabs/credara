//! Conformance suite (spec §11.4): run synthetic, test-data-tagged data through the store and
//! graph and assert the system's contracts — provenance preservation, authorization + revocation
//! enforcement, disagreement surfacing, data-category handling, and test-data filtering.
//!
//! The deployment / multi-peer parts (helm install on kind/k3d, gossip convergence, anti-entropy
//! repair, partition/rejoin, Bound-1 revocation latency §4.7) require real peers and a network and
//! live in the test bed (DQ-3); they run once the libp2p transport + gRPC serve path are wired.

use std::collections::HashMap;

use creda_conformance::generator::conformance_requester;
use creda_conformance::{clinical_view, operator_view, Generator, Scenario};
use creda_events::{
    Demographics, EventPayload, IdentityEventNode, IdentityEventType, SignatureAlgorithm,
    SigningKey, TestDataTag, VerificationMethod,
};
use creda_graph::{evaluate, project, AuthorizationQuery, DefaultPosture, FieldKey, RequesterContext, Subgraph};
use creda_store::{MemoryStore, Store};

const NOW: i64 = 1_800_000_000;

fn store_of(events: &[IdentityEventNode]) -> MemoryStore {
    let s = MemoryStore::new();
    Generator::populate(&s, events).unwrap();
    s
}

fn demographics_of(node: &IdentityEventNode) -> &Demographics {
    match &node.payload {
        EventPayload::Assert { demographics, .. } => demographics,
        _ => panic!("expected an Assert"),
    }
}

#[test]
fn provenance_is_preserved() {
    let mut gen = Generator::new(7, "conformance/provenance");
    let events = gen.generate(5, Scenario::Authorized);
    let store = store_of(&events);

    // Every event round-trips, and every event's parents resolve locally (no dangling refs).
    for e in &events {
        assert_eq!(store.get_event(&e.id).unwrap().as_ref(), Some(e));
        for parent in &e.parent_ids {
            assert!(store.has_event(parent).unwrap(), "parent {parent} missing");
        }
    }
}

#[test]
fn authorization_then_revocation_is_enforced() {
    let mut gen = Generator::new(11, "conformance/auth");
    let events = gen.patient(Scenario::Authorized); // [assert, grant, attest]
    let assert = &events[0];
    let grant = events
        .iter()
        .find(|e| e.event_type == IdentityEventType::AuthorizationGrant)
        .expect("grant present");

    let query = AuthorizationQuery {
        requester: RequesterContext::new(conformance_requester()),
        purpose: creda_events::GrantPurpose::Treatment,
        use_mode: creda_events::UseMode::ReadOnly,
        requested_event_types: vec![],
        requested_segments: vec![],
        requested_data_categories: vec![],
    };

    // With the grant, the requester is authorized.
    let store = store_of(&events);
    let sg = Subgraph::materialize(&store, &[assert.id]).unwrap();
    assert!(evaluate(&sg, &query, DefaultPosture::DenyByDefault, NOW, &HashMap::new()).authorized);

    // A validated revocation of that grant denies it.
    let revoker = SigningKey::generate(SignatureAlgorithm::Ed25519).unwrap();
    let revocation = IdentityEventNode::create_test_data(
        EventPayload::AuthorizationRevocation { target_grant_id: grant.id },
        vec![grant.id],
        &revoker,
        100,
        "2026-01-01T00:00:00Z",
        None,
        TestDataTag { purpose: "integration-testing".into(), originating_test: "conformance/auth".into(), expiration_time: None },
    )
    .unwrap();
    let mut with_revoke = events.clone();
    with_revoke.push(revocation);
    let store2 = store_of(&with_revoke);
    let sg2 = Subgraph::materialize(&store2, &[assert.id]).unwrap();
    assert!(!evaluate(&sg2, &query, DefaultPosture::DenyByDefault, NOW, &HashMap::new()).authorized);
}

#[test]
fn conflicting_demographics_are_flagged() {
    let mut gen = Generator::new(13, "conformance/dispute");
    let events = gen.patient(Scenario::Disagreement); // two asserts, conflicting DOB
    let entries: Vec<_> = events.iter().map(|e| e.id).collect();
    let store = store_of(&events);
    let sg = Subgraph::materialize(&store, &entries).unwrap();
    let ei = project(&sg, &entries, &creda_graph::ConfidenceConfig::default(), NOW);
    let dob = ei.field(&FieldKey::DateOfBirth).unwrap();
    assert!(dob.disputed, "conflicting DOBs should be flagged disputed");
    assert_eq!(dob.values.len(), 2);
}

#[test]
fn data_categories_are_respected() {
    let mut gen = Generator::new(17, "conformance/data-category");
    let events = gen.generate(3, Scenario::Simple);

    // Identity assertions are tokenized (§9.2): every demographic value is an opaque token.
    for e in &events {
        let d = demographics_of(e);
        for part in d.name_family.iter().chain(d.name_given.iter()).flatten() {
            assert!(part.0.starts_with("tok:"), "demographic must be tokenized, got {:?}", part.0);
        }
        assert!(d.date_of_birth.as_ref().unwrap().0.starts_with("tok:"));
    }

    // Clinical data never enters the trust graph: a DeceasedDeclaration carries only a
    // cause-present *flag*, never the cause itself (§3.4.7).
    let key = SigningKey::generate(SignatureAlgorithm::Ed25519).unwrap();
    let assert = &events[0];
    let deceased = IdentityEventNode::create(
        EventPayload::DeceasedDeclaration {
            date_of_death: "2026-02-01".into(),
            certifier_id: creda_events::CertificateFingerprint::from_public_key_bytes(b"vital-records"),
            cause_of_death_present: true,
        },
        vec![assert.id],
        &key,
        50,
        "2026-01-01T00:00:00Z",
        None,
    )
    .unwrap();
    match &deceased.payload {
        EventPayload::DeceasedDeclaration { cause_of_death_present, .. } => {
            // The flag exists; there is structurally no field to carry the actual cause.
            assert!(*cause_of_death_present);
        }
        _ => unreachable!(),
    }
}

#[test]
fn test_data_is_filtered_from_clinical_but_visible_to_operator() {
    let mut gen = Generator::new(23, "conformance/test-data");
    let synthetic = gen.generate(2, Scenario::Simple); // tagged test-data

    // A "real" (untagged) event.
    let key = SigningKey::generate(SignatureAlgorithm::Ed25519).unwrap();
    let real = IdentityEventNode::create(
        EventPayload::Assert {
            demographics: Demographics {
                date_of_birth: Some(creda_events::TokenizedDate("tok:real".into())),
                ..Default::default()
            },
            verification_method: VerificationMethod::GovernmentPhotoId,
        },
        vec![],
        &key,
        1,
        "2026-01-01T00:00:00Z",
        None,
    )
    .unwrap();

    let mut all = synthetic.clone();
    all.push(real.clone());

    // Synthetic events propagate/replicate normally (all present in the store).
    let store = store_of(&all);
    for e in &all {
        assert!(store.has_event(&e.id).unwrap());
    }

    // Clinical view: only the real event. Operator view: everything.
    let clinical = clinical_view(&all);
    assert_eq!(clinical.len(), 1);
    assert_eq!(clinical[0].id, real.id);
    assert!(clinical.iter().all(|e| !e.is_test_data()));

    let operator = operator_view(&all);
    assert_eq!(operator.len(), all.len());
    assert!(synthetic.iter().all(|e| e.is_test_data()));
}

#[test]
fn scale_is_configurable() {
    let mut gen = Generator::new(29, "conformance/scale");
    // A single Simple patient = one Assert; N patients = N events. The same call with a much
    // larger N is the load-test path (§11.4.2).
    let events = gen.generate(100, Scenario::Simple);
    assert_eq!(events.len(), 100);
    assert!(events.iter().all(|e| e.is_test_data()));
}

#[test]
fn synthetic_content_is_deterministic_for_a_seed() {
    let mut g1 = Generator::new(99, "t");
    let mut g2 = Generator::new(99, "t");
    // Same seed -> same demographic content (UUIDs/keys differ, but content is reproducible).
    let p1 = g1.patient(Scenario::Simple);
    let p2 = g2.patient(Scenario::Simple);
    assert_eq!(demographics_of(&p1[0]), demographics_of(&p2[0]));
}
