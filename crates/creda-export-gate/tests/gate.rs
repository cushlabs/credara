//! Export Gate (M6 source side): permit a valid release (emitting an ExportReceipt) and refuse
//! invalid / expired / revoked / audience-mismatched / ungranted ones (§4.5.1, §10.2).

use creda_events::{
    AuthorizationScope, CertificateFingerprint, Demographics, EventId, EventPayload, GrantAudience,
    GrantPurpose, IdentityEventNode, IdentityEventType, SignatureAlgorithm, SigningKey,
    TokenizedDate, UseMode, VerificationMethod,
};
use creda_export_gate::{ExportGate, ExportRequest};
use creda_graph::{AuthorizationQuery, RequesterContext};
use creda_store::{MemoryStore, Store};

const NOW: i64 = 1_800_000_000; // ~2027

fn key() -> SigningKey {
    SigningKey::generate(SignatureAlgorithm::Ed25519).unwrap()
}

fn mk_assert(k: &SigningKey) -> IdentityEventNode {
    IdentityEventNode::create(
        EventPayload::Assert {
            demographics: Demographics {
                date_of_birth: Some(TokenizedDate("dob".into())),
                ..Default::default()
            },
            verification_method: VerificationMethod::GovernmentPhotoId,
        },
        vec![],
        k,
        1,
        "2026-05-20T00:00:00Z",
        None,
    )
    .unwrap()
}

#[allow(clippy::too_many_arguments)]
fn mk_grant(
    k: &SigningKey,
    parent: EventId,
    audience: GrantAudience,
    expiration: Option<String>,
) -> IdentityEventNode {
    IdentityEventNode::create(
        EventPayload::AuthorizationGrant {
            scope: AuthorizationScope::default(),
            audience,
            purpose: GrantPurpose::Treatment,
            expiration,
            volume_constraints: None,
            use_mode: UseMode::ReadAndRely,
        },
        vec![parent],
        k,
        2,
        "2026-05-20T00:00:00Z",
        None,
    )
    .unwrap()
}

fn mk_revoke(k: &SigningKey, grant_id: EventId) -> IdentityEventNode {
    IdentityEventNode::create(
        EventPayload::AuthorizationRevocation { target_grant_id: grant_id },
        vec![grant_id],
        k,
        3,
        "2026-05-20T00:00:00Z",
        None,
    )
    .unwrap()
}

fn query(requester: CertificateFingerprint) -> AuthorizationQuery {
    AuthorizationQuery {
        requester: RequesterContext::new(requester),
        purpose: GrantPurpose::Treatment,
        use_mode: UseMode::ReadOnly,
        requested_event_types: vec![],
        requested_segments: vec![],
        requested_data_categories: vec![],
    }
}

fn store_of(events: &[IdentityEventNode]) -> MemoryStore {
    let s = MemoryStore::new();
    for e in events {
        s.put_event(e).unwrap();
    }
    s
}

#[test]
fn permits_valid_export_and_emits_receipt() {
    let inst = key();
    let requester = CertificateFingerprint::from_public_key_bytes(b"requester");
    let assert = mk_assert(&inst);
    let grant = mk_grant(&inst, assert.id, GrantAudience::InstitutionId(requester.clone()), None);
    let store = store_of(&[assert.clone(), grant.clone()]);

    let gate = ExportGate::new(inst);
    let req = ExportRequest { entry_points: vec![assert.id], query: query(requester) };
    let outcome = gate.authorize_export(&store, &req, NOW, 10).unwrap();

    assert!(outcome.is_permitted(), "valid artifact should permit export");
    let receipt = outcome.receipt().unwrap();
    assert_eq!(receipt.event_type, IdentityEventType::ExportReceipt);
    match &receipt.payload {
        EventPayload::ExportReceipt { governing_grant_id, .. } => {
            assert_eq!(*governing_grant_id, grant.id)
        }
        _ => panic!("expected ExportReceipt payload"),
    }
}

#[test]
fn refuses_expired_grant() {
    let inst = key();
    let requester = CertificateFingerprint::from_public_key_bytes(b"requester");
    let assert = mk_assert(&inst);
    let grant = mk_grant(
        &inst,
        assert.id,
        GrantAudience::InstitutionId(requester.clone()),
        Some("2000-01-01T00:00:00Z".into()),
    );
    let store = store_of(&[assert.clone(), grant]);
    let gate = ExportGate::new(inst);
    let req = ExportRequest { entry_points: vec![assert.id], query: query(requester) };
    assert!(!gate.authorize_export(&store, &req, NOW, 10).unwrap().is_permitted());
}

#[test]
fn refuses_revoked_grant() {
    let inst = key();
    let requester = CertificateFingerprint::from_public_key_bytes(b"requester");
    let assert = mk_assert(&inst);
    let grant = mk_grant(&inst, assert.id, GrantAudience::InstitutionId(requester.clone()), None);
    let revoke = mk_revoke(&inst, grant.id);
    let store = store_of(&[assert.clone(), grant, revoke]);
    let gate = ExportGate::new(inst);
    let req = ExportRequest { entry_points: vec![assert.id], query: query(requester) };
    assert!(!gate.authorize_export(&store, &req, NOW, 10).unwrap().is_permitted());
}

#[test]
fn refuses_audience_mismatch_and_no_grant() {
    // Grant addressed to a different institution.
    let inst = key();
    let assert = mk_assert(&inst);
    let other = CertificateFingerprint::from_public_key_bytes(b"someone-else");
    let grant = mk_grant(&inst, assert.id, GrantAudience::InstitutionId(other), None);
    let requester = CertificateFingerprint::from_public_key_bytes(b"requester");
    let store = store_of(&[assert.clone(), grant]);
    let gate = ExportGate::new(inst);
    let req = ExportRequest { entry_points: vec![assert.id], query: query(requester.clone()) };
    assert!(!gate.authorize_export(&store, &req, NOW, 10).unwrap().is_permitted());

    // No grant at all.
    let inst2 = key();
    let assert2 = mk_assert(&inst2);
    let store2 = store_of(&[assert2.clone()]);
    let gate2 = ExportGate::new(inst2);
    let req2 = ExportRequest { entry_points: vec![assert2.id], query: query(requester) };
    assert!(!gate2.authorize_export(&store2, &req2, NOW, 10).unwrap().is_permitted());
}
