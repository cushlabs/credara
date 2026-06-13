//! Verifier (M6 relying side): the three-part check — authorization, identity continuity,
//! provenance integrity — plus stale-state reporting, all against a local store (§4.5.2, §10.3).

use creda_events::ids::new_event_id;
use creda_events::{
    AttestPurpose, AuthorizationScope, CertificateFingerprint, Demographics, EventId, EventPayload,
    GrantAudience, GrantPurpose, IdentityEventNode, SignatureAlgorithm, SigningKey, TokenizedDate,
    UseMode, VerificationMethod,
};
use creda_graph::{AuthorizationQuery, RequesterContext};
use creda_store::{MemoryStore, Store};
use creda_verifier::{Verifier, VerifyRequest};

const NOW: i64 = 1_800_000_000;

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

fn mk_grant(
    k: &SigningKey,
    parent: EventId,
    requester: &CertificateFingerprint,
) -> IdentityEventNode {
    IdentityEventNode::create(
        EventPayload::AuthorizationGrant {
            scope: AuthorizationScope::default(),
            audience: GrantAudience::InstitutionId(requester.clone()),
            purpose: GrantPurpose::Treatment,
            expiration: None,
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
        EventPayload::AuthorizationRevocation {
            target_grant_id: grant_id,
        },
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
fn valid_use_verifies() {
    let inst = key();
    let requester = CertificateFingerprint::from_public_key_bytes(b"requester");
    let assert = mk_assert(&inst);
    let grant = mk_grant(&inst, assert.id, &requester);
    let store = store_of(&[assert.clone(), grant.clone()]);

    let verifier = Verifier::new(3600);
    let req = VerifyRequest {
        entry_points: vec![assert.id],
        governing_grant_id: grant.id,
        query: query(requester),
    };
    // Fresh sync (last_sync == now).
    let report = verifier.verify(&store, &req, NOW, NOW).unwrap();
    assert!(
        report.is_valid(),
        "valid use should verify: {}",
        report.reason
    );
    assert!(report.authorized && report.identity_continuity && report.provenance_intact);
    assert!(!report.stale);
}

#[test]
fn revoked_grant_is_not_authorized() {
    let inst = key();
    let requester = CertificateFingerprint::from_public_key_bytes(b"requester");
    let assert = mk_assert(&inst);
    let grant = mk_grant(&inst, assert.id, &requester);
    let revoke = mk_revoke(&inst, grant.id);
    let store = store_of(&[assert.clone(), grant.clone(), revoke]);

    let verifier = Verifier::new(3600);
    let req = VerifyRequest {
        entry_points: vec![assert.id],
        governing_grant_id: grant.id,
        query: query(requester),
    };
    let report = verifier.verify(&store, &req, NOW, NOW).unwrap();
    assert!(!report.authorized);
    assert!(!report.is_valid());
}

#[test]
fn missing_parent_breaks_provenance() {
    let inst = key();
    let requester = CertificateFingerprint::from_public_key_bytes(b"requester");
    let assert = mk_assert(&inst);
    let grant = mk_grant(&inst, assert.id, &requester);

    // An Attest connected to the subgraph (parent = assert) but also referencing a parent that
    // is NOT in the store — a dangling reference.
    let missing = new_event_id(&CertificateFingerprint::from_public_key_bytes(b"ghost"));
    let attest = IdentityEventNode::create(
        EventPayload::Attest {
            target_event_ids: vec![assert.id],
            purpose: AttestPurpose::Treatment,
        },
        vec![assert.id, missing], // `missing` is never stored
        &inst,
        4,
        "2026-05-20T00:00:00Z",
        None,
    )
    .unwrap();
    let store = store_of(&[assert.clone(), grant.clone(), attest]);

    let verifier = Verifier::new(3600);
    let req = VerifyRequest {
        entry_points: vec![assert.id],
        governing_grant_id: grant.id,
        query: query(requester),
    };
    let report = verifier.verify(&store, &req, NOW, NOW).unwrap();
    assert!(
        !report.provenance_intact,
        "a missing parent must break provenance integrity"
    );
    assert!(!report.is_valid());
}

#[test]
fn stale_view_is_flagged_but_still_valid() {
    let inst = key();
    let requester = CertificateFingerprint::from_public_key_bytes(b"requester");
    let assert = mk_assert(&inst);
    let grant = mk_grant(&inst, assert.id, &requester);
    let store = store_of(&[assert.clone(), grant.clone()]);

    let verifier = Verifier::new(3600); // 1h threshold
    let req = VerifyRequest {
        entry_points: vec![assert.id],
        governing_grant_id: grant.id,
        query: query(requester),
    };
    // Last sync a day ago.
    let report = verifier.verify(&store, &req, NOW, NOW - 86_400).unwrap();
    assert!(report.stale, "a day-old view should be flagged stale");
    assert!(report.dag_age_secs >= 86_400);
    // Staleness is advisory: the substantive checks still pass.
    assert!(report.is_valid());
}
