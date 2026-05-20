//! Shared test helpers for the creda-events integration tests.
// Each integration-test binary compiles this module fresh and uses only some helpers, so
// suppress dead-code warnings for the ones a given binary doesn't touch.
#![allow(dead_code)]

use std::collections::BTreeMap;

use creda_events::ids::new_event_id;
use creda_events::{
    AttestPurpose, AuthorizationScope, CertificateFingerprint, ContestReason, Demographics,
    EventId, EventPayload, GrantAudience, GrantPurpose, IdentityEventNode, LinkMethod,
    SignatureAlgorithm, SigningKey, TombstoneBasis, TokenizedString, UseMode, VerificationMethod,
};
use creda_events::payload::ContestReasonCode;

pub const WALL_CLOCK: &str = "2026-05-20T12:00:00Z";

/// A fresh Ed25519 signing key (always available, no `pqc` feature needed).
pub fn ed_key() -> SigningKey {
    SigningKey::generate(SignatureAlgorithm::Ed25519).unwrap()
}

/// A throwaway event id, for use as a parent/target reference in tests.
pub fn some_id() -> EventId {
    let inst = CertificateFingerprint::from_public_key_bytes(b"test-institution");
    new_event_id(&inst)
}

pub fn sample_demographics() -> Demographics {
    let mut extensions = BTreeMap::new();
    extensions.insert("us-va:veteran-id".to_string(), TokenizedString::from("tok-veteran"));
    Demographics {
        name_family: Some(vec![TokenizedString::from("tok-family")]),
        name_given: Some(vec![TokenizedString::from("tok-given")]),
        date_of_birth: Some(creda_events::TokenizedDate("tok-dob".into())),
        sex: Some(creda_events::AdministrativeGender::Female),
        ssn_last_four: Some(TokenizedString::from("tok-1234")),
        extensions,
        ..Default::default()
    }
}

/// Build a root `Assert` (no parents).
pub fn build_assert(key: &SigningKey) -> IdentityEventNode {
    IdentityEventNode::create(
        EventPayload::Assert {
            demographics: sample_demographics(),
            verification_method: VerificationMethod::GovernmentPhotoId,
        },
        vec![],
        key,
        1,
        WALL_CLOCK,
        None,
    )
    .unwrap()
}

/// One valid node of every event type, for round-trip coverage. Parents/targets are synthetic
/// ids — these tests exercise the *event model*, not graph traversal.
pub fn one_of_each_event(key: &SigningKey) -> Vec<IdentityEventNode> {
    let p1 = some_id();
    let p2 = some_id();
    let mk = |payload: EventPayload, parents: Vec<EventId>| {
        IdentityEventNode::create(payload, parents, key, 7, WALL_CLOCK, None).unwrap()
    };

    vec![
        build_assert(key),
        mk(
            EventPayload::Link {
                target_subgraph_heads: (p1, p2),
                confidence_score: 9500,
                method: LinkMethod::Algorithmic,
            },
            vec![p1, p2],
        ),
        mk(
            EventPayload::Contest {
                target_link_id: p1,
                reason: ContestReason {
                    code: ContestReasonCode::DistinctPatients,
                    detail: Some("manual review".into()),
                },
            },
            vec![p1],
        ),
        mk(
            EventPayload::Attest {
                target_event_ids: vec![p1],
                purpose: AttestPurpose::Treatment,
            },
            vec![p1],
        ),
        mk(
            EventPayload::Amend {
                target_event_id: p1,
                updated_demographics: sample_demographics(),
                amendment_reason: "corrected spelling".into(),
            },
            vec![p1],
        ),
        mk(
            EventPayload::Tombstone {
                target_event_ids: vec![p1],
                legal_basis: TombstoneBasis::RightToBeForgotten,
            },
            vec![p1],
        ),
        mk(
            EventPayload::DeceasedDeclaration {
                date_of_death: "2026-04-01".into(),
                certifier_id: CertificateFingerprint::from_public_key_bytes(b"vital-records"),
                cause_of_death_present: false,
            },
            vec![p1],
        ),
        mk(
            EventPayload::AuthorizationGrant {
                scope: AuthorizationScope::default(),
                audience: GrantAudience::InstitutionClass("any-tefca-qhin".into()),
                purpose: GrantPurpose::Treatment,
                expiration: Some("2027-01-01T00:00:00Z".into()),
                volume_constraints: None,
                use_mode: UseMode::ReadAndRely,
            },
            vec![p1],
        ),
        mk(
            EventPayload::AuthorizationRevocation { target_grant_id: p1 },
            vec![p1],
        ),
        mk(
            EventPayload::ExportReceipt {
                governing_grant_id: p1,
                requesting_institution: CertificateFingerprint::from_public_key_bytes(b"req-inst"),
                released_scope: AuthorizationScope::default(),
            },
            vec![p1],
        ),
    ]
}
