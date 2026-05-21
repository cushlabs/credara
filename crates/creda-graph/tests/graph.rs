//! M3 done-criteria: effective-identity projection and authorization decisions match
//! hand-computed expectations across amendments, contestations, tombstones, disagreement,
//! agreement, decay, and the full authorization matrix (revoked / expired / volume / audience /
//! use-mode / default postures). Confidence is deterministic and monotonic.

use std::collections::HashMap;

use creda_events::payload::ContestReasonCode;
use creda_events::{
    AttestPurpose, AuthorizationScope, CertificateFingerprint, ContestReason, Demographics,
    EventId, EventPayload, GrantAudience, GrantPurpose, IdentityEventNode, LinkMethod,
    RedistributionPolicy, SignatureAlgorithm, SigningKey, StructuredAddress, TokenizedDate,
    TokenizedString, TombstoneBasis, UseMode, VerificationMethod, VolumeConstraints,
};
use creda_graph::{
    evaluate, project, responder_may_serve, AuthorizationQuery, ConfidenceConfig, DefaultPosture,
    FieldKey, RequesterContext, Subgraph,
};
use creda_store::{MemoryStore, Store};

const WALL: &str = "2026-05-20T12:00:00Z";

// ---- helpers -------------------------------------------------------------------------------

fn key() -> SigningKey {
    SigningKey::generate(SignatureAlgorithm::Ed25519).unwrap()
}

fn fp_of(k: &SigningKey) -> CertificateFingerprint {
    CertificateFingerprint::new(k.verifying_key().fingerprint())
}

fn secs(ts: &str) -> i64 {
    time::OffsetDateTime::parse(ts, &time::format_description::well_known::Rfc3339)
        .unwrap()
        .unix_timestamp()
}

fn demo_dob(dob: &str) -> Demographics {
    Demographics {
        date_of_birth: Some(TokenizedDate(dob.into())),
        ..Default::default()
    }
}

fn demo_family(name: &str) -> Demographics {
    Demographics {
        name_family: Some(vec![TokenizedString::from(name)]),
        ..Default::default()
    }
}

fn demo_address(line: &str) -> Demographics {
    Demographics {
        address: Some(StructuredAddress {
            line1: Some(TokenizedString::from(line)),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn assert_ev(s: &SigningKey, d: Demographics, m: VerificationMethod, wall: &str) -> IdentityEventNode {
    IdentityEventNode::create(
        EventPayload::Assert {
            demographics: d,
            verification_method: m,
        },
        vec![],
        s,
        1,
        wall,
        None,
    )
    .unwrap()
}

fn amend_ev(s: &SigningKey, target: EventId, d: Demographics, clock: u64) -> IdentityEventNode {
    IdentityEventNode::create(
        EventPayload::Amend {
            target_event_id: target,
            updated_demographics: d,
            amendment_reason: "correction".into(),
        },
        vec![target],
        s,
        clock,
        WALL,
        None,
    )
    .unwrap()
}

fn link_ev(s: &SigningKey, a: EventId, b: EventId) -> IdentityEventNode {
    IdentityEventNode::create(
        EventPayload::Link {
            target_subgraph_heads: (a, b),
            confidence_score: 9000,
            method: LinkMethod::Algorithmic,
        },
        vec![a, b],
        s,
        2,
        WALL,
        None,
    )
    .unwrap()
}

fn contest_ev(s: &SigningKey, link_id: EventId) -> IdentityEventNode {
    IdentityEventNode::create(
        EventPayload::Contest {
            target_link_id: link_id,
            reason: ContestReason {
                code: ContestReasonCode::DistinctPatients,
                detail: None,
            },
        },
        vec![link_id],
        s,
        3,
        WALL,
        None,
    )
    .unwrap()
}

fn attest_ev(s: &SigningKey, target: EventId) -> IdentityEventNode {
    IdentityEventNode::create(
        EventPayload::Attest {
            target_event_ids: vec![target],
            purpose: AttestPurpose::Treatment,
        },
        vec![target],
        s,
        2,
        WALL,
        None,
    )
    .unwrap()
}

fn tombstone_ev(s: &SigningKey, target: EventId) -> IdentityEventNode {
    IdentityEventNode::create(
        EventPayload::Tombstone {
            target_event_ids: vec![target],
            legal_basis: TombstoneBasis::RightToBeForgotten,
        },
        vec![target],
        s,
        2,
        WALL,
        None,
    )
    .unwrap()
}

#[allow(clippy::too_many_arguments)]
fn grant_ev(
    s: &SigningKey,
    parent: EventId,
    audience: GrantAudience,
    purpose: GrantPurpose,
    use_mode: UseMode,
    expiration: Option<String>,
    volume: Option<VolumeConstraints>,
) -> IdentityEventNode {
    IdentityEventNode::create(
        EventPayload::AuthorizationGrant {
            scope: AuthorizationScope::default(),
            audience,
            purpose,
            expiration,
            volume_constraints: volume,
            use_mode,
        },
        vec![parent],
        s,
        2,
        WALL,
        None,
    )
    .unwrap()
}

fn revoke_ev(s: &SigningKey, grant_id: EventId) -> IdentityEventNode {
    IdentityEventNode::create(
        EventPayload::AuthorizationRevocation {
            target_grant_id: grant_id,
        },
        vec![grant_id],
        s,
        3,
        WALL,
        None,
    )
    .unwrap()
}

fn store_with(events: &[IdentityEventNode]) -> MemoryStore {
    let store = MemoryStore::new();
    for e in events {
        store.put_event(e).unwrap();
    }
    store
}

fn project_from(events: &[IdentityEventNode], entries: &[EventId], now: i64) -> creda_graph::EffectiveIdentity {
    let store = store_with(events);
    let sg = Subgraph::materialize(&store, entries).unwrap();
    project(&sg, entries, &ConfidenceConfig::default(), now)
}

fn query(req: CertificateFingerprint, purpose: GrantPurpose, use_mode: UseMode) -> AuthorizationQuery {
    AuthorizationQuery {
        requester: RequesterContext::new(req),
        purpose,
        use_mode,
        requested_event_types: vec![],
        requested_segments: vec![],
        requested_data_categories: vec![],
    }
}

fn decide(
    events: &[IdentityEventNode],
    entries: &[EventId],
    q: &AuthorizationQuery,
    posture: DefaultPosture,
    util: &HashMap<EventId, u64>,
) -> creda_graph::AuthorizationDecision {
    let store = store_with(events);
    let sg = Subgraph::materialize(&store, entries).unwrap();
    evaluate(&sg, q, posture, secs(WALL), util)
}

// ---- effective-identity projection ---------------------------------------------------------

#[test]
fn single_assert_projects_field() {
    let k = key();
    let a = assert_ev(&k, demo_dob("dob-1980"), VerificationMethod::GovernmentPhotoId, WALL);
    let ei = project_from(&[a.clone()], &[a.id], secs(WALL));
    let e = ei.field(&FieldKey::DateOfBirth).unwrap();
    assert!(!e.disputed);
    assert_eq!(e.values.len(), 1);
    assert_eq!(e.values[0].value, "dob-1980");
    assert!(e.values[0].confidence > 0);
}

#[test]
fn amend_supersedes_original() {
    let k = key();
    let a = assert_ev(&k, demo_dob("dob-1980"), VerificationMethod::GovernmentPhotoId, WALL);
    let am = amend_ev(&k, a.id, demo_dob("dob-1981"), 5);
    let ei = project_from(&[a.clone(), am], &[a.id], secs(WALL));
    let e = ei.field(&FieldKey::DateOfBirth).unwrap();
    assert!(!e.disputed);
    assert_eq!(e.values.len(), 1);
    assert_eq!(e.values[0].value, "dob-1981");
}

#[test]
fn conflicting_asserts_are_disputed() {
    let a = assert_ev(&key(), demo_dob("dob-1980"), VerificationMethod::GovernmentPhotoId, WALL);
    let b = assert_ev(&key(), demo_dob("dob-1990"), VerificationMethod::GovernmentPhotoId, WALL);
    let ei = project_from(&[a.clone(), b.clone()], &[a.id, b.id], secs(WALL));
    let e = ei.field(&FieldKey::DateOfBirth).unwrap();
    assert!(e.disputed);
    assert_eq!(e.values.len(), 2);
}

#[test]
fn independent_agreement_raises_confidence() {
    let dob = "dob-1980";
    let a = assert_ev(&key(), demo_dob(dob), VerificationMethod::GovernmentPhotoId, WALL);
    let single = project_from(&[a.clone()], &[a.id], secs(WALL));
    let c_single = single.field(&FieldKey::DateOfBirth).unwrap().values[0].confidence;

    let b = assert_ev(&key(), demo_dob(dob), VerificationMethod::GovernmentPhotoId, WALL);
    let c = assert_ev(&key(), demo_dob(dob), VerificationMethod::GovernmentPhotoId, WALL);
    let three = project_from(&[a.clone(), b.clone(), c.clone()], &[a.id, b.id, c.id], secs(WALL));
    let e3 = three.field(&FieldKey::DateOfBirth).unwrap();
    assert!(!e3.disputed);
    assert_eq!(e3.values.len(), 1);
    assert!(
        e3.values[0].confidence > c_single,
        "independent agreement should raise confidence: {} vs {}",
        e3.values[0].confidence,
        c_single
    );
}

#[test]
fn government_id_beats_self_report() {
    let g = assert_ev(&key(), demo_dob("d"), VerificationMethod::GovernmentPhotoId, WALL);
    let cg = project_from(&[g.clone()], &[g.id], secs(WALL))
        .field(&FieldKey::DateOfBirth)
        .unwrap()
        .values[0]
        .confidence;
    let sr = assert_ev(&key(), demo_dob("d"), VerificationMethod::SelfReport, WALL);
    let csr = project_from(&[sr.clone()], &[sr.id], secs(WALL))
        .field(&FieldKey::DateOfBirth)
        .unwrap()
        .values[0]
        .confidence;
    assert!(cg > csr, "gov-id {} should beat self-report {}", cg, csr);
}

#[test]
fn attestation_raises_confidence() {
    let k1 = key();
    let a = assert_ev(&k1, demo_dob("d"), VerificationMethod::SelfReport, WALL);
    let baseline = project_from(&[a.clone()], &[a.id], secs(WALL))
        .field(&FieldKey::DateOfBirth)
        .unwrap()
        .values[0]
        .confidence;
    let at = attest_ev(&key(), a.id);
    let amplified = project_from(&[a.clone(), at], &[a.id], secs(WALL))
        .field(&FieldKey::DateOfBirth)
        .unwrap()
        .values[0]
        .confidence;
    assert!(amplified > baseline, "attestation should raise confidence: {} vs {}", amplified, baseline);
}

#[test]
fn tombstone_removes_demographics() {
    let k = key();
    let a = assert_ev(&k, demo_family("smith"), VerificationMethod::GovernmentPhotoId, WALL);
    let t = tombstone_ev(&k, a.id);
    let ei = project_from(&[a.clone(), t], &[a.id], secs(WALL));
    assert!(
        ei.field(&FieldKey::NameFamily).is_none(),
        "a tombstoned assert must contribute no demographics"
    );
}

#[test]
fn valid_contest_severs_link() {
    let ka = key();
    let kb = key();
    let a = assert_ev(&ka, demo_family("smith"), VerificationMethod::GovernmentPhotoId, WALL);
    let b = assert_ev(&kb, demo_family("jones"), VerificationMethod::GovernmentPhotoId, WALL);
    let l = link_ev(&ka, a.id, b.id); // linker ka is a party

    // Without a contest: the link merges both sides, so family is disputed.
    let merged = project_from(&[a.clone(), b.clone(), l.clone()], &[a.id], secs(WALL));
    let fam = merged.field(&FieldKey::NameFamily).unwrap();
    assert!(fam.disputed && fam.values.len() == 2);

    // A valid contest (by the link creator, a party) severs the link.
    let c = contest_ev(&ka, l.id);
    let split = project_from(&[a.clone(), b.clone(), l.clone(), c], &[a.id], secs(WALL));
    let fam2 = split.field(&FieldKey::NameFamily).unwrap();
    assert!(!fam2.disputed && fam2.values.len() == 1 && fam2.values[0].value == "smith");
}

#[test]
fn invalid_contest_does_not_sever_link() {
    let ka = key();
    let kb = key();
    let kc = key(); // unrelated — not a party
    let a = assert_ev(&ka, demo_family("smith"), VerificationMethod::GovernmentPhotoId, WALL);
    let b = assert_ev(&kb, demo_family("jones"), VerificationMethod::GovernmentPhotoId, WALL);
    let l = link_ev(&ka, a.id, b.id);
    let c = contest_ev(&kc, l.id); // not a party -> invalid
    let ei = project_from(&[a.clone(), b.clone(), l.clone(), c], &[a.id], secs(WALL));
    let fam = ei.field(&FieldKey::NameFamily).unwrap();
    assert!(fam.disputed && fam.values.len() == 2, "an invalid contest must not sever the link");
}

#[test]
fn amend_by_wrong_institution_is_ignored() {
    let ka = key();
    let kb = key();
    let a = assert_ev(&ka, demo_dob("dob-1980"), VerificationMethod::GovernmentPhotoId, WALL);
    let bad = amend_ev(&kb, a.id, demo_dob("dob-1999"), 5); // wrong institution
    let ei = project_from(&[a.clone(), bad], &[a.id], secs(WALL));
    let e = ei.field(&FieldKey::DateOfBirth).unwrap();
    assert!(!e.disputed);
    assert_eq!(e.values[0].value, "dob-1980", "an invalid amend must be ignored");
}

#[test]
fn projection_is_deterministic() {
    let k = key();
    let a = assert_ev(&k, demo_dob("d"), VerificationMethod::GovernmentPhotoId, WALL);
    let store = store_with(&[a.clone()]);
    let sg = Subgraph::materialize(&store, &[a.id]).unwrap();
    let e1 = project(&sg, &[a.id], &ConfidenceConfig::default(), secs(WALL));
    let e2 = project(&sg, &[a.id], &ConfidenceConfig::default(), secs(WALL));
    assert_eq!(e1, e2);
}

#[test]
fn fast_field_decays_with_age() {
    let now = secs(WALL);
    let fresh = assert_ev(&key(), demo_address("addr"), VerificationMethod::GovernmentPhotoId, WALL);
    let cf = project_from(&[fresh.clone()], &[fresh.id], now)
        .field(&FieldKey::Address)
        .unwrap()
        .values[0]
        .confidence;
    let old = assert_ev(
        &key(),
        demo_address("addr"),
        VerificationMethod::GovernmentPhotoId,
        "2010-01-01T00:00:00Z",
    );
    let co = project_from(&[old.clone()], &[old.id], now)
        .field(&FieldKey::Address)
        .unwrap()
        .values[0]
        .confidence;
    assert!(cf > co, "an older fast-decaying field should score lower: fresh {} old {}", cf, co);
}

// ---- authorization evaluation --------------------------------------------------------------

#[test]
fn deny_by_default_without_grant() {
    let a = assert_ev(&key(), demo_dob("d"), VerificationMethod::GovernmentPhotoId, WALL);
    let q = query(fp_of(&key()), GrantPurpose::Treatment, UseMode::ReadOnly);
    let d = decide(&[a.clone()], &[a.id], &q, DefaultPosture::DenyByDefault, &HashMap::new());
    assert!(!d.authorized);
}

#[test]
fn treatment_presumed_authorizes_tpo_but_not_research() {
    let a = assert_ev(&key(), demo_dob("d"), VerificationMethod::GovernmentPhotoId, WALL);
    let treat = query(fp_of(&key()), GrantPurpose::Treatment, UseMode::ReadOnly);
    let d = decide(&[a.clone()], &[a.id], &treat, DefaultPosture::TreatmentPresumed, &HashMap::new());
    assert!(d.authorized && d.covering_grants.is_empty());

    let research = query(fp_of(&key()), GrantPurpose::Research, UseMode::ReadOnly);
    let d2 = decide(&[a.clone()], &[a.id], &research, DefaultPosture::TreatmentPresumed, &HashMap::new());
    assert!(!d2.authorized, "research always needs an explicit grant");
}

#[test]
fn covering_grant_authorizes() {
    let kp = key();
    let req = fp_of(&key());
    let a = assert_ev(&kp, demo_dob("d"), VerificationMethod::GovernmentPhotoId, WALL);
    let g = grant_ev(
        &kp,
        a.id,
        GrantAudience::InstitutionId(req.clone()),
        GrantPurpose::Treatment,
        UseMode::ReadAndRely,
        None,
        None,
    );
    let q = query(req, GrantPurpose::Treatment, UseMode::ReadOnly);
    let d = decide(&[a.clone(), g.clone()], &[a.id], &q, DefaultPosture::DenyByDefault, &HashMap::new());
    assert!(d.authorized);
    assert_eq!(d.covering_grants, vec![g.id]);
}

#[test]
fn audience_mismatch_denied() {
    let kp = key();
    let a = assert_ev(&kp, demo_dob("d"), VerificationMethod::GovernmentPhotoId, WALL);
    let g = grant_ev(
        &kp,
        a.id,
        GrantAudience::InstitutionId(fp_of(&key())), // some other institution
        GrantPurpose::Treatment,
        UseMode::ReadAndRely,
        None,
        None,
    );
    let q = query(fp_of(&key()), GrantPurpose::Treatment, UseMode::ReadOnly);
    let d = decide(&[a.clone(), g], &[a.id], &q, DefaultPosture::DenyByDefault, &HashMap::new());
    assert!(!d.authorized);
}

#[test]
fn revoked_grant_denied() {
    let kp = key();
    let req = fp_of(&key());
    let a = assert_ev(&kp, demo_dob("d"), VerificationMethod::GovernmentPhotoId, WALL);
    let g = grant_ev(
        &kp,
        a.id,
        GrantAudience::InstitutionId(req.clone()),
        GrantPurpose::Treatment,
        UseMode::ReadAndRely,
        None,
        None,
    );
    let r = revoke_ev(&kp, g.id);
    let q = query(req, GrantPurpose::Treatment, UseMode::ReadOnly);
    let d = decide(&[a.clone(), g, r], &[a.id], &q, DefaultPosture::DenyByDefault, &HashMap::new());
    assert!(!d.authorized, "a validated revocation must withdraw the grant");
}

#[test]
fn expired_grant_denied() {
    let kp = key();
    let req = fp_of(&key());
    let a = assert_ev(&kp, demo_dob("d"), VerificationMethod::GovernmentPhotoId, WALL);
    let g = grant_ev(
        &kp,
        a.id,
        GrantAudience::InstitutionId(req.clone()),
        GrantPurpose::Treatment,
        UseMode::ReadAndRely,
        Some("2000-01-01T00:00:00Z".into()),
        None,
    );
    let q = query(req, GrantPurpose::Treatment, UseMode::ReadOnly);
    let d = decide(&[a.clone(), g], &[a.id], &q, DefaultPosture::DenyByDefault, &HashMap::new());
    assert!(!d.authorized);
}

#[test]
fn volume_exhausted_denied() {
    let kp = key();
    let req = fp_of(&key());
    let a = assert_ev(&kp, demo_dob("d"), VerificationMethod::GovernmentPhotoId, WALL);
    let g = grant_ev(
        &kp,
        a.id,
        GrantAudience::InstitutionId(req.clone()),
        GrantPurpose::Treatment,
        UseMode::ReadAndRely,
        None,
        Some(VolumeConstraints {
            max_requests: Some(2),
            ..Default::default()
        }),
    );
    let mut util = HashMap::new();
    util.insert(g.id, 2u64);
    let q = query(req, GrantPurpose::Treatment, UseMode::ReadOnly);
    let d = decide(&[a.clone(), g], &[a.id], &q, DefaultPosture::DenyByDefault, &util);
    assert!(!d.authorized, "an exhausted volume limit must deny");
}

#[test]
fn use_mode_must_not_exceed_grant() {
    let kp = key();
    let req = fp_of(&key());
    let a = assert_ev(&kp, demo_dob("d"), VerificationMethod::GovernmentPhotoId, WALL);

    // ReadOnly grant cannot satisfy a ReadAndExport request.
    let ro = grant_ev(&kp, a.id, GrantAudience::InstitutionId(req.clone()), GrantPurpose::Treatment, UseMode::ReadOnly, None, None);
    let q_export = query(req.clone(), GrantPurpose::Treatment, UseMode::ReadAndExport);
    assert!(!decide(&[a.clone(), ro], &[a.id], &q_export, DefaultPosture::DenyByDefault, &HashMap::new()).authorized);

    // ReadAndExport grant satisfies a ReadOnly request.
    let rx = grant_ev(&kp, a.id, GrantAudience::InstitutionId(req.clone()), GrantPurpose::Treatment, UseMode::ReadAndExport, None, None);
    let q_read = query(req, GrantPurpose::Treatment, UseMode::ReadOnly);
    assert!(decide(&[a.clone(), rx], &[a.id], &q_read, DefaultPosture::DenyByDefault, &HashMap::new()).authorized);
}

#[test]
fn institution_class_audience_matches() {
    let kp = key();
    let req = fp_of(&key());
    let a = assert_ev(&kp, demo_dob("d"), VerificationMethod::GovernmentPhotoId, WALL);
    let g = grant_ev(
        &kp,
        a.id,
        GrantAudience::InstitutionClass("any-tefca-qhin".into()),
        GrantPurpose::Treatment,
        UseMode::ReadAndRely,
        None,
        None,
    );
    let q = AuthorizationQuery {
        requester: RequesterContext {
            fingerprint: req,
            classes: vec!["any-tefca-qhin".into()],
            wildcards: vec![],
        },
        purpose: GrantPurpose::Treatment,
        use_mode: UseMode::ReadOnly,
        requested_event_types: vec![],
        requested_segments: vec![],
        requested_data_categories: vec![],
    };
    let store = store_with(&[a.clone(), g]);
    let sg = Subgraph::materialize(&store, &[a.id]).unwrap();
    let d = evaluate(&sg, &q, DefaultPosture::DenyByDefault, secs(WALL), &HashMap::new());
    assert!(d.authorized, "requester in the granted class should be authorized");
}

#[test]
fn redistribution_policy_is_honored() {
    let ko = key();
    let originator = fp_of(&ko);
    let other = fp_of(&key());

    let no_redist = IdentityEventNode::create(
        EventPayload::Assert {
            demographics: demo_dob("d"),
            verification_method: VerificationMethod::GovernmentPhotoId,
        },
        vec![],
        &ko,
        1,
        WALL,
        Some(RedistributionPolicy::NoRedistribution),
    )
    .unwrap();
    assert!(responder_may_serve(&no_redist, &originator), "originator may serve its own event");
    assert!(!responder_may_serve(&no_redist, &other), "a recipient must not redistribute");

    let open = IdentityEventNode::create(
        EventPayload::Assert {
            demographics: demo_dob("d"),
            verification_method: VerificationMethod::GovernmentPhotoId,
        },
        vec![],
        &ko,
        1,
        WALL,
        Some(RedistributionPolicy::Open),
    )
    .unwrap();
    assert!(responder_may_serve(&open, &other), "Open policy lets any peer serve");

    let custom = IdentityEventNode::create(
        EventPayload::Assert {
            demographics: demo_dob("d"),
            verification_method: VerificationMethod::GovernmentPhotoId,
        },
        vec![],
        &ko,
        1,
        WALL,
        Some(RedistributionPolicy::Custom("by-agreement".into())),
    )
    .unwrap();
    assert!(!responder_may_serve(&custom, &other), "unknown Custom policy is conservatively denied");
}
