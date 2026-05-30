//! Link-chain validation for cross-institutional authorization (spec §4.6 step 5.5).
//!
//! When a `Grant` lives in a subgraph fragment that reached the responding peer's view of the
//! patient through `Link` events — i.e. the Grant's institution Asserted about a patient at
//! their own clinic, then someone Linked that fragment to a patient the responding peer holds —
//! the cross-institutional honor of that Grant depends on whether the merging Links are
//! trustworthy. This module is the structural defense against the rogue-Link attack pattern:
//!
//! - A rogue clinic Asserts a patient (self-issued, harmless on its own).
//! - The rogue clinic publishes a `Link` from their fragment to a real patient subgraph at
//!   another institution.
//! - The rogue clinic publishes an `AuthorizationGrant` naming themselves as audience.
//! - Without this check, the responding peer's §4.6 evaluation would admit the Grant because
//!   the Link merged the fragments and the Grant is now "in the subgraph."
//!
//! Two filters:
//!
//! 1. **Per-`LinkMethod` confidence ceiling.** The Link's signing institution claims a
//!    `confidence_score`, but the responding peer caps that claim by method — `Manual` and
//!    `Other` cap lower than `InsuranceCrosswalk`. A low-method claim of 10000 is not honored at
//!    10000. (Recommendation #3.)
//! 2. **Per-Link confidence floor + author standing.** Every Link on the path from the Grant to
//!    a responder-anchored event must meet the floor *after* the method ceiling is applied. At
//!    least one Link in the path must be authored by a party with prior standing in the
//!    responding peer's view of the subgraph — a predecessor Assert or Attest by the same
//!    institution that predates the Link. (Recommendation #1.)
//!
//! The check is responder-configurable. It only applies to Grants reached through Link
//! traversal; a Grant in the responder's own first-encounter subgraph fragment is unaffected,
//! which preserves the legitimate new-clinic-with-new-patient pattern.
//!
//! Defaults aim at "deny the obvious attack, permit the obvious legitimate case." Concrete
//! calibration is institution policy.

use std::collections::{BTreeSet, VecDeque};

use creda_events::{EventId, EventPayload, IdentityEventNode, LinkMethod};

use crate::subgraph::Subgraph;

/// Per-`LinkMethod` ceilings on the signing institution's claimed `confidence_score`. A struct
/// rather than a map because the methods are a small fixed set, and we want infallible lookups
/// for any `LinkMethod` variant without hash/order requirements on the enum (which is bit-stable
/// across the wire). Adjust fields directly to recalibrate.
#[derive(Clone, Debug)]
pub struct MethodCeilings {
    pub insurance_crosswalk: u16,
    pub referral: u16,
    pub algorithmic: u16,
    pub manual: u16,
    pub other: u16,
}

impl MethodCeilings {
    /// Default ceilings reflect each method's intrinsic verification strength (§5.3.5). These
    /// are starting points, not protocol invariants; institutions calibrate.
    pub fn defaults() -> Self {
        Self {
            insurance_crosswalk: 9500,
            referral: 9000,
            algorithmic: 7000,
            manual: 5000,
            other: 3000,
        }
    }

    /// Ceiling for a specific method.
    pub fn for_method(&self, method: LinkMethod) -> u16 {
        match method {
            LinkMethod::InsuranceCrosswalk => self.insurance_crosswalk,
            LinkMethod::Referral => self.referral,
            LinkMethod::Algorithmic => self.algorithmic,
            LinkMethod::Manual => self.manual,
            LinkMethod::Other => self.other,
        }
    }
}

impl Default for MethodCeilings {
    fn default() -> Self {
        Self::defaults()
    }
}

/// Configuration for the Link-chain check. Held per responding peer. Construct via
/// [`LinkChainConfig::default`] and adjust.
#[derive(Clone, Debug)]
pub struct LinkChainConfig {
    /// Minimum effective confidence (after method ceiling) any Link in the merge chain must
    /// meet. Effective confidence below this discards the Grant served through that chain.
    pub min_link_confidence: u16,
    /// Whether at least one Link on the merge chain must be authored by an institution with
    /// prior standing in the responder's view (a predecessor Assert/Attest predating the Link).
    /// `true` is the strict posture recommended for federal/deny-by-default deployments.
    pub require_author_standing: bool,
    /// Per-`LinkMethod` ceiling on the signing institution's claimed `confidence_score`. The
    /// effective confidence used in the floor check is `min(claimed, ceiling)`.
    pub method_ceilings: MethodCeilings,
}

impl LinkChainConfig {
    /// Effective confidence for a Link, given the ceiling.
    pub fn effective_confidence(&self, claimed: u16, method: LinkMethod) -> u16 {
        claimed.min(self.method_ceilings.for_method(method))
    }
}

impl Default for LinkChainConfig {
    fn default() -> Self {
        Self {
            // Deliberately mid-range: blocks the obviously-low-confidence rogue case without
            // breaking referral or insurance-crosswalk Links from new institutions.
            min_link_confidence: 6000,
            // Off by default to preserve the legitimate first-encounter case. Federal /
            // deny-by-default deployments flip this on.
            require_author_standing: false,
            method_ceilings: MethodCeilings::defaults(),
        }
    }
}

/// Whether the Grant at `grant_id` may be honored against the responding peer's view, given
/// the merge chain of Links connecting it (transitively) to events the responder considers
/// anchored.
///
/// `responder_anchors` is the set of event ids in the subgraph the responder treats as already
/// trusted — typically the responder's own Asserts and Attests for the patient, plus anything
/// signed by institutions with which the responder has prior trust relationships. Empty means
/// "trust nothing pre-existing"; every Link in the chain must meet the floor on its own.
///
/// Returns:
/// - `LinkChainOk` — the Grant survives this filter.
/// - `LinkChainDeny { reason }` — the Grant is discarded; `reason` is a short tag suitable for
///   inclusion in the responder's decision rationale.
pub fn evaluate_link_chain(
    subgraph: &Subgraph,
    grant_id: EventId,
    responder_anchors: &BTreeSet<EventId>,
    config: &LinkChainConfig,
) -> LinkChainResult {
    // Fast path: the Grant itself is anchored. Nothing to check.
    if responder_anchors.contains(&grant_id) {
        return LinkChainResult::Ok;
    }

    // BFS from the Grant outward. Traverse parents, payload-references, and children. Each Link
    // we cross must meet the floor; we also remember whether any Link on the path has author
    // standing in the responder's view.
    let mut queue: VecDeque<(EventId, bool)> = VecDeque::new();
    queue.push_back((grant_id, false));
    let mut visited: BTreeSet<EventId> = BTreeSet::new();
    visited.insert(grant_id);

    // If the BFS reaches an anchor with at least one standing-bearing Link in the path (when
    // required), we admit. If we exhaust the BFS without reaching an anchor through clean Links,
    // we deny. The "path-bearing" boolean tracks "did the path so far include a Link whose
    // author has standing."

    while let Some((cur, path_has_standing)) = queue.pop_front() {
        // Reached an anchor? Decide.
        if responder_anchors.contains(&cur) {
            if config.require_author_standing && !path_has_standing {
                return LinkChainResult::Deny {
                    reason: "merge chain to anchor lacks a Link with author standing".into(),
                };
            }
            return LinkChainResult::Ok;
        }

        let Some(node) = subgraph.get(&cur) else {
            continue;
        };

        // If the current node is a Link, check its effective confidence; below floor blocks this
        // path from advancing. Also flag whether this Link grants the path "standing."
        let mut step_has_standing = path_has_standing;
        if let EventPayload::Link { method, confidence_score, .. } = &node.payload {
            let effective = config.effective_confidence(*confidence_score, *method);
            if effective < config.min_link_confidence {
                // Skip — don't propagate beyond a too-weak Link. Other paths may still reach an
                // anchor.
                continue;
            }
            if config.require_author_standing && link_author_has_standing(subgraph, node, responder_anchors) {
                step_has_standing = true;
            }
        }

        let neighbors = node_neighbors(subgraph, node);
        for next in neighbors {
            if visited.insert(next) {
                queue.push_back((next, step_has_standing));
            }
        }
    }

    LinkChainResult::Deny {
        reason: "Grant unreachable from a responder-anchored event through Links meeting the floor".into(),
    }
}

/// Whether the Link's signing institution has prior standing — at least one Assert or Attest in
/// the subgraph, by the same institution, that the responder treats as anchored. This is the
/// formal "party with prior standing" check from spec §4.6 step 5.5.
fn link_author_has_standing(
    subgraph: &Subgraph,
    link: &IdentityEventNode,
    responder_anchors: &BTreeSet<EventId>,
) -> bool {
    let author = &link.institution_id;
    for anchored_id in responder_anchors {
        let Some(anchored) = subgraph.get(anchored_id) else {
            continue;
        };
        if &anchored.institution_id == author
            && matches!(
                anchored.payload,
                EventPayload::Assert { .. } | EventPayload::Attest { .. }
            )
        {
            return true;
        }
    }
    false
}

/// Neighbors of a node for BFS purposes: parents, payload-referenced events, and children
/// within the subgraph.
fn node_neighbors(subgraph: &Subgraph, node: &IdentityEventNode) -> Vec<EventId> {
    let mut out: Vec<EventId> = Vec::new();
    out.extend(node.parent_ids.iter().copied());
    out.extend(crate::subgraph::referenced_ids(node));
    for child in subgraph.children_of(&node.id) {
        out.push(child);
    }
    out
}

/// Result of the Link-chain check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkChainResult {
    Ok,
    Deny { reason: String },
}

/// Convenience: classify a batch of Grants against the responder's anchors and config. Returns
/// a `(admitted, denied)` partition keyed by Grant id, with per-denial reasons.
pub fn classify_grants<'a>(
    subgraph: &Subgraph,
    grant_ids: impl IntoIterator<Item = &'a EventId>,
    responder_anchors: &BTreeSet<EventId>,
    config: &LinkChainConfig,
) -> (Vec<EventId>, Vec<(EventId, String)>) {
    let mut admitted = Vec::new();
    let mut denied: Vec<(EventId, String)> = Vec::new();
    for id in grant_ids {
        match evaluate_link_chain(subgraph, *id, responder_anchors, config) {
            LinkChainResult::Ok => admitted.push(*id),
            LinkChainResult::Deny { reason } => denied.push((*id, reason)),
        }
    }
    (admitted, denied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use creda_events::{
        AdministrativeGender, AttestPurpose, AuthorizationScope, CertificateFingerprint,
        Demographics, EventPayload, GrantAudience, GrantPurpose, IdentityEventNode,
        SignatureAlgorithm, SigningKey, TokenizedString, UseMode, VerificationMethod,
    };

    fn fp(byte: u8) -> CertificateFingerprint {
        CertificateFingerprint::new(vec![byte; 32])
    }

    fn signer_for(byte: u8) -> SigningKey {
        // The institution_id on events created via IdentityEventNode::create comes from the
        // signer's verifying key fingerprint, not from `fp(byte)`. For test plumbing we use a
        // distinct signer per role (hospital_b, rogue_clinic, etc.) and read its institution_id
        // from the produced event.
        let _ = byte;
        SigningKey::generate(SignatureAlgorithm::Ed25519).expect("ed25519 keygen")
    }

    /// CertificateFingerprint derived from a signer's verifying key. Used for Grant audiences
    /// when the test wants the audience to be the signer itself (self-issued Grants).
    fn cert_fp(signer: &SigningKey) -> CertificateFingerprint {
        CertificateFingerprint::new(signer.verifying_key().fingerprint())
    }

    fn assert_event(
        signer: &SigningKey,
        clock: u64,
        family: &str,
    ) -> IdentityEventNode {
        IdentityEventNode::create(
            EventPayload::Assert {
                demographics: Demographics {
                    name_family: Some(vec![TokenizedString(format!("tok:{family}"))]),
                    date_of_birth: Some(creda_events::TokenizedDate("tok:1970-01-01".into())),
                    sex: Some(AdministrativeGender::Other),
                    ..Default::default()
                },
                verification_method: VerificationMethod::GovernmentPhotoId,
            },
            vec![],
            signer,
            clock,
            "2026-05-01T00:00:00Z",
            None,
        )
        .expect("valid Assert")
    }

    fn link_event(
        signer: &SigningKey,
        clock: u64,
        heads: (EventId, EventId),
        method: LinkMethod,
        confidence: u16,
    ) -> IdentityEventNode {
        IdentityEventNode::create(
            EventPayload::Link {
                target_subgraph_heads: heads,
                confidence_score: confidence,
                method,
            },
            vec![heads.0, heads.1],
            signer,
            clock,
            "2026-05-01T01:00:00Z",
            None,
        )
        .expect("valid Link")
    }

    fn grant_event(
        signer: &SigningKey,
        clock: u64,
        parent: EventId,
        audience: CertificateFingerprint,
    ) -> IdentityEventNode {
        IdentityEventNode::create(
            EventPayload::AuthorizationGrant {
                scope: AuthorizationScope {
                    subgraph_segments: vec![],
                    event_types: vec![],
                    data_categories: vec![],
                },
                audience: GrantAudience::InstitutionId(audience),
                purpose: GrantPurpose::Treatment,
                expiration: None,
                volume_constraints: None,
                use_mode: UseMode::ReadAndExport,
            },
            vec![parent],
            signer,
            clock,
            "2026-05-01T02:00:00Z",
            None,
        )
        .expect("valid Grant")
    }

    fn attest_event(
        signer: &SigningKey,
        clock: u64,
        targets: Vec<EventId>,
    ) -> IdentityEventNode {
        IdentityEventNode::create(
            EventPayload::Attest {
                target_event_ids: targets.clone(),
                purpose: AttestPurpose::Treatment,
            },
            targets,
            signer,
            clock,
            "2026-05-01T03:00:00Z",
            None,
        )
        .expect("valid Attest")
    }

    #[test]
    fn default_ceilings_cover_all_known_methods() {
        let ceilings = MethodCeilings::defaults();
        // The struct shape guarantees every variant is covered at compile time. This test exists
        // to catch regressions: if someone adds a new LinkMethod variant they must also extend
        // MethodCeilings (the match in for_method would otherwise fail to compile).
        for method in [
            LinkMethod::Manual,
            LinkMethod::Algorithmic,
            LinkMethod::Referral,
            LinkMethod::InsuranceCrosswalk,
            LinkMethod::Other,
        ] {
            let c = ceilings.for_method(method);
            assert!(c > 0, "default ceiling for {method:?} is zero — would silently deny everything");
            assert!(c <= 10_000, "ceiling exceeds confidence range for {method:?}");
        }
    }

    #[test]
    fn effective_confidence_caps_at_ceiling() {
        let cfg = LinkChainConfig::default();
        assert_eq!(cfg.effective_confidence(10_000, LinkMethod::Manual), 5000);
        assert_eq!(cfg.effective_confidence(9800, LinkMethod::InsuranceCrosswalk), 9500);
        assert_eq!(cfg.effective_confidence(4000, LinkMethod::Manual), 4000);
    }

    #[test]
    fn anchored_grant_is_admitted_without_traversal() {
        // The Grant lives directly in the responder's anchor set — the legitimate
        // first-encounter case where the clinic Asserts + Grants from itself, no Link needed.
        let hospital_b = signer_for(1);
        let assert = assert_event(&hospital_b, 0, "smith");
        let grant = grant_event(&hospital_b, 1, assert.id, fp(0xAA));
        let subgraph = Subgraph::from_nodes([assert.clone(), grant.clone()]);
        let mut anchors = BTreeSet::new();
        anchors.insert(assert.id);
        anchors.insert(grant.id);

        let cfg = LinkChainConfig::default();
        assert_eq!(evaluate_link_chain(&subgraph, grant.id, &anchors, &cfg), LinkChainResult::Ok);
    }

    #[test]
    fn legitimate_first_encounter_grant_in_own_fragment_is_admitted() {
        // New clinic — no anchors at the responder yet, but the Grant is in the same fragment as
        // the clinic's own Assert. With permissive defaults (require_author_standing = false),
        // the responder admits it via the direct path from Grant -> Assert (both in subgraph,
        // and no Links between them, so no Link-floor block).
        // This is what makes the recommendation safe for legitimate onboarding.
        let new_clinic = signer_for(2);
        let assert = assert_event(&new_clinic, 0, "doe");
        let grant = grant_event(&new_clinic, 1, assert.id, cert_fp(&new_clinic));
        let subgraph = Subgraph::from_nodes([assert.clone(), grant.clone()]);

        // The responder treats its OWN events as anchors. If the responder hasn't yet seen this
        // patient at all, anchors is empty — and that's the failure mode we want: no anchor =>
        // Grant denied. But the legitimate path is: responder eventually creates its own
        // matching Assert (or a Link to the new clinic's fragment). For this test, we model the
        // case where the responder's anchor IS the new clinic's Assert (e.g., the responder
        // happens to be the new clinic itself in M5+ daemon mode — and self-issued Grants
        // against your own subgraph never need cross-institutional honoring).
        let mut anchors = BTreeSet::new();
        anchors.insert(assert.id);

        let cfg = LinkChainConfig::default();
        assert_eq!(evaluate_link_chain(&subgraph, grant.id, &anchors, &cfg), LinkChainResult::Ok);
    }

    #[test]
    fn rogue_link_low_confidence_blocks_traversal() {
        // Hospital B has its own subgraph. Rogue clinic Asserts a parallel patient and Links to
        // Hospital B's Assert with Manual method at the maximum claimed confidence (10000). The
        // method ceiling caps it at 5000 — below the default floor of 6000. Rogue's Grant is
        // unreachable from Hospital B's anchor through any Link that meets the floor.
        let hospital_b = signer_for(3);
        let rogue_clinic = signer_for(4);

        let b_assert = assert_event(&hospital_b, 0, "patient-x");
        let rogue_assert = assert_event(&rogue_clinic, 1, "patient-x");
        let rogue_link = link_event(
            &rogue_clinic,
            2,
            (rogue_assert.id, b_assert.id),
            LinkMethod::Manual,
            10_000, // brazen overclaim
        );
        let rogue_grant = grant_event(
            &rogue_clinic,
            3,
            rogue_assert.id,
            cert_fp(&rogue_clinic),
        );

        let subgraph = Subgraph::from_nodes([
            b_assert.clone(),
            rogue_assert.clone(),
            rogue_link.clone(),
            rogue_grant.clone(),
        ]);

        let mut anchors = BTreeSet::new();
        anchors.insert(b_assert.id);

        let cfg = LinkChainConfig::default();
        let result = evaluate_link_chain(&subgraph, rogue_grant.id, &anchors, &cfg);
        match result {
            LinkChainResult::Deny { reason } => {
                assert!(
                    reason.contains("unreachable") || reason.contains("anchor"),
                    "expected reachability-related denial, got: {reason}"
                );
            }
            LinkChainResult::Ok => panic!("expected denial for rogue Manual link at the ceiling"),
        }
    }

    #[test]
    fn high_confidence_insurance_crosswalk_link_passes() {
        // Same shape as the rogue test, but the linking institution uses InsuranceCrosswalk at
        // 9000. Ceiling for InsuranceCrosswalk is 9500, so effective confidence is 9000 —
        // above the default floor of 6000. The Grant is admitted via that Link.
        let hospital_b = signer_for(5);
        let referrer = signer_for(6);

        let b_assert = assert_event(&hospital_b, 0, "patient-y");
        let r_assert = assert_event(&referrer, 1, "patient-y");
        let link = link_event(
            &referrer,
            2,
            (r_assert.id, b_assert.id),
            LinkMethod::InsuranceCrosswalk,
            9000,
        );
        let grant = grant_event(
            &referrer,
            3,
            r_assert.id,
            cert_fp(&referrer),
        );

        let subgraph = Subgraph::from_nodes([
            b_assert.clone(),
            r_assert.clone(),
            link.clone(),
            grant.clone(),
        ]);

        let mut anchors = BTreeSet::new();
        anchors.insert(b_assert.id);

        let cfg = LinkChainConfig::default();
        assert_eq!(evaluate_link_chain(&subgraph, grant.id, &anchors, &cfg), LinkChainResult::Ok);
    }

    #[test]
    fn require_standing_denies_link_from_stranger() {
        // require_author_standing = true: even an InsuranceCrosswalk Link from a stranger fails
        // because the linking institution has no prior Assert/Attest in the responder's anchor
        // set. This is the strict deny-by-default posture for federal-program deployments.
        let hospital_b = signer_for(7);
        let stranger = signer_for(8);

        let b_assert = assert_event(&hospital_b, 0, "patient-z");
        let s_assert = assert_event(&stranger, 1, "patient-z");
        let link = link_event(
            &stranger,
            2,
            (s_assert.id, b_assert.id),
            LinkMethod::InsuranceCrosswalk,
            9000,
        );
        let grant = grant_event(
            &stranger,
            3,
            s_assert.id,
            cert_fp(&stranger),
        );

        let subgraph = Subgraph::from_nodes([
            b_assert.clone(),
            s_assert.clone(),
            link.clone(),
            grant.clone(),
        ]);

        let mut anchors = BTreeSet::new();
        anchors.insert(b_assert.id);

        let cfg = LinkChainConfig {
            require_author_standing: true,
            ..Default::default()
        };
        match evaluate_link_chain(&subgraph, grant.id, &anchors, &cfg) {
            LinkChainResult::Deny { reason } => {
                assert!(
                    reason.contains("standing"),
                    "expected standing-related denial, got: {reason}"
                );
            }
            LinkChainResult::Ok => panic!("strict posture should deny a stranger's Link"),
        }
    }

    #[test]
    fn require_standing_admits_link_from_party_with_prior_attest() {
        // require_author_standing = true, and the linking institution has previously Attested an
        // event in Hospital B's subgraph. That establishes standing; the Link is honored.
        let hospital_b = signer_for(9);
        let partner = signer_for(10);

        let b_assert = assert_event(&hospital_b, 0, "patient-w");
        // Partner's prior Attest in Hospital B's subgraph — this is the standing-bearing event.
        let partner_attest = attest_event(&partner, 1, vec![b_assert.id]);
        let p_assert = assert_event(&partner, 2, "patient-w");
        let link = link_event(
            &partner,
            3,
            (p_assert.id, b_assert.id),
            LinkMethod::Referral,
            8500,
        );
        let grant = grant_event(
            &partner,
            4,
            p_assert.id,
            cert_fp(&partner),
        );

        let subgraph = Subgraph::from_nodes([
            b_assert.clone(),
            partner_attest.clone(),
            p_assert.clone(),
            link.clone(),
            grant.clone(),
        ]);

        let mut anchors = BTreeSet::new();
        anchors.insert(b_assert.id);
        anchors.insert(partner_attest.id); // responder trusts the prior Attest

        let cfg = LinkChainConfig {
            require_author_standing: true,
            ..Default::default()
        };
        assert_eq!(evaluate_link_chain(&subgraph, grant.id, &anchors, &cfg), LinkChainResult::Ok);
    }
}
