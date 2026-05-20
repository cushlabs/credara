//! Structural validation invariants checkable from a single event — spec §3.4, §5.1.3.
//!
//! Graph-dependent invariants (Contest party-of-subgraph §3.4.3; Amend originating-institution
//! §3.4.5) are enforced in creda-graph (M3) and are intentionally not exercised here.

mod common;

use creda_events::{
    AttestPurpose, ContestReason, EventPayload, IdentityEventNode, IdentityEventType, LinkMethod,
    TombstoneBasis,
};
use creda_events::payload::ContestReasonCode;

fn create(payload: EventPayload, parents: Vec<creda_events::EventId>) -> creda_events::Result<IdentityEventNode> {
    let key = common::ed_key();
    IdentityEventNode::create(payload, parents, &key, 1, common::WALL_CLOCK, None)
}

#[test]
fn assert_may_be_root() {
    let key = common::ed_key();
    let node = common::build_assert(&key);
    assert!(node.parent_ids.is_empty());
    node.validate_structure().unwrap();
}

#[test]
fn non_assert_events_may_not_be_roots() {
    // An Attest with no parents violates the root rule.
    let res = create(
        EventPayload::Attest {
            target_event_ids: vec![common::some_id()],
            purpose: AttestPurpose::Treatment,
        },
        vec![],
    );
    assert!(res.is_err(), "non-Assert root must be rejected");
}

#[test]
fn link_confidence_must_be_within_basis_points() {
    let a = common::some_id();
    let b = common::some_id();
    let res = create(
        EventPayload::Link {
            target_subgraph_heads: (a, b),
            confidence_score: 10_001, // > 100.00%
            method: LinkMethod::Manual,
        },
        vec![a, b],
    );
    assert!(res.is_err(), "confidence over 10000 bp must be rejected");
}

#[test]
fn link_heads_must_be_distinct_and_referenced_as_parents() {
    let a = common::some_id();
    let b = common::some_id();

    // Identical heads.
    assert!(create(
        EventPayload::Link {
            target_subgraph_heads: (a, a),
            confidence_score: 5000,
            method: LinkMethod::Manual,
        },
        vec![a],
    )
    .is_err());

    // Heads not present in parent_ids.
    assert!(create(
        EventPayload::Link {
            target_subgraph_heads: (a, b),
            confidence_score: 5000,
            method: LinkMethod::Manual,
        },
        vec![common::some_id()],
    )
    .is_err());

    // Valid case.
    assert!(create(
        EventPayload::Link {
            target_subgraph_heads: (a, b),
            confidence_score: 5000,
            method: LinkMethod::Manual,
        },
        vec![a, b],
    )
    .is_ok());
}

#[test]
fn attest_and_tombstone_require_targets() {
    let p = common::some_id();
    assert!(create(
        EventPayload::Attest {
            target_event_ids: vec![],
            purpose: AttestPurpose::Treatment,
        },
        vec![p],
    )
    .is_err());

    assert!(create(
        EventPayload::Tombstone {
            target_event_ids: vec![],
            legal_basis: TombstoneBasis::RightToBeForgotten,
        },
        vec![p],
    )
    .is_err());
}

#[test]
fn amend_requires_a_reason() {
    let p = common::some_id();
    assert!(create(
        EventPayload::Amend {
            target_event_id: p,
            updated_demographics: common::sample_demographics(),
            amendment_reason: "   ".into(),
        },
        vec![p],
    )
    .is_err());
}

#[test]
fn event_type_is_derived_from_payload() {
    let p = common::some_id();
    let node = create(
        EventPayload::Contest {
            target_link_id: p,
            reason: ContestReason {
                code: ContestReasonCode::DuplicateRecord,
                detail: None,
            },
        },
        vec![p],
    )
    .unwrap();
    assert_eq!(node.event_type, IdentityEventType::Contest);
}

#[test]
fn revocation_and_tombstone_are_distinct_event_types() {
    // Guards against the sibling-spec name-collision bug: these must never be the same type.
    assert_ne!(
        IdentityEventType::AuthorizationRevocation,
        IdentityEventType::Tombstone
    );
    assert!(IdentityEventType::AuthorizationRevocation.is_authorization());
    assert!(!IdentityEventType::Tombstone.is_authorization());
}
