//! The seven-step authorization evaluation algorithm (spec §4.6).
//!
//! Given a materialized subgraph and a request, decide whether a covering `AuthorizationGrant`
//! authorizes it. The algorithm is local — no network, no callback, no central consent service
//! — and is the reference logic for both the responding peer and the Verifier (M6).
//!
//! This implements steps 1–5 and 7. Step 6 (cross-institutional redistribution honoring) is a
//! *per-event* filter applied when events are actually served; it is provided here as
//! [`responder_may_serve`] for the caller to apply per event.
//!
//! Note on "validated" revocations (§4.6 step 2): a revocation counts only if its parent
//! references are resolved locally. Signature verification happens at ingest (creda-core /
//! the Store layer), so here "validated" means structurally resolved (parents present).

use std::collections::{BTreeSet, HashMap};

use creda_events::{
    AuthorizationScope, CertificateFingerprint, EventId, EventPayload, GrantAudience, GrantPurpose,
    IdentityEventNode, IdentityEventType, RedistributionPolicy, UseMode,
};

use crate::link_chain::{evaluate_link_chain, LinkChainConfig, LinkChainResult};
use crate::subgraph::Subgraph;

/// The responding peer's posture when no Grant covers a request (spec §9.3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultPosture {
    /// Nothing is served without an explicit covering Grant.
    DenyByDefault,
    /// Treatment/Payment/Operations are presumed authorized (HIPAA TPO); everything else
    /// still needs a Grant.
    TreatmentPresumed,
}

/// What the requesting institution is and which audiences it satisfies. Class/wildcard
/// membership normally comes from the Participant Registry (not available at this layer), so
/// it is supplied explicitly, keeping evaluation pure and testable.
#[derive(Clone, Debug)]
pub struct RequesterContext {
    pub fingerprint: CertificateFingerprint,
    /// Institutional classes this requester belongs to (e.g. "any-tefca-qhin").
    pub classes: Vec<String>,
    /// Constrained wildcards this requester satisfies (e.g. "active-baa").
    pub wildcards: Vec<String>,
}

impl RequesterContext {
    /// A requester identified only by fingerprint (no class/wildcard memberships).
    pub fn new(fingerprint: CertificateFingerprint) -> Self {
        Self {
            fingerprint,
            classes: Vec::new(),
            wildcards: Vec::new(),
        }
    }
}

/// A request to evaluate.
#[derive(Clone, Debug)]
pub struct AuthorizationQuery {
    pub requester: RequesterContext,
    pub purpose: GrantPurpose,
    pub use_mode: UseMode,
    /// Requested event types; empty = "any" (covered only by an unrestricted Grant).
    pub requested_event_types: Vec<IdentityEventType>,
    /// Requested subgraph segments; empty = "any".
    pub requested_segments: Vec<EventId>,
    /// Requested data categories; empty = "any".
    pub requested_data_categories: Vec<String>,
}

/// The decision: whether the request is authorized, which Grants cover it, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizationDecision {
    pub authorized: bool,
    /// Grants that survived all steps (empty when authorized only by default posture).
    pub covering_grants: Vec<EventId>,
    pub reason: String,
}

/// Evaluate a request against the patient's subgraph (steps 1–5 and 7).
///
/// `utilization` maps a Grant's id to the number of requests already served under it, for the
/// volume check (§4.6 step 5); pass an empty map if not tracked.
///
/// This variant does **not** apply the Link-chain cross-institutional check (§4.6 step 5.5);
/// callers that need it use [`evaluate_with_link_chain`].
pub fn evaluate(
    subgraph: &Subgraph,
    query: &AuthorizationQuery,
    posture: DefaultPosture,
    now_unix_secs: i64,
    utilization: &HashMap<EventId, u64>,
) -> AuthorizationDecision {
    evaluate_inner(subgraph, query, posture, now_unix_secs, utilization, None)
}

/// Evaluate with the Link-chain check (§4.6 step 5.5). Each Grant must be reachable from a
/// responder-anchored event through Links that meet the configured per-method ceilings and the
/// floor. The check defends against rogue-Link attacks where an admitted-but-bad-actor
/// institution merges a fabricated fragment into a real patient's subgraph.
///
/// `responder_anchors` is the set of event ids the responder considers already-trusted in the
/// subgraph — typically its own Asserts/Attests for the patient. A Grant in an anchor's own
/// fragment is always admitted (the legitimate first-encounter case).
pub fn evaluate_with_link_chain(
    subgraph: &Subgraph,
    query: &AuthorizationQuery,
    posture: DefaultPosture,
    now_unix_secs: i64,
    utilization: &HashMap<EventId, u64>,
    responder_anchors: &BTreeSet<EventId>,
    link_chain_config: &LinkChainConfig,
) -> AuthorizationDecision {
    evaluate_inner(
        subgraph,
        query,
        posture,
        now_unix_secs,
        utilization,
        Some((responder_anchors, link_chain_config)),
    )
}

fn evaluate_inner(
    subgraph: &Subgraph,
    query: &AuthorizationQuery,
    posture: DefaultPosture,
    now_unix_secs: i64,
    utilization: &HashMap<EventId, u64>,
    link_chain: Option<(&BTreeSet<EventId>, &LinkChainConfig)>,
) -> AuthorizationDecision {
    let mut covering = Vec::new();
    let mut link_chain_denials: Vec<String> = Vec::new();

    // Step 1: collect AuthorizationGrants (sorted by id, deterministic).
    for grant in subgraph.nodes_of_type(IdentityEventType::AuthorizationGrant) {
        let EventPayload::AuthorizationGrant {
            scope,
            audience,
            purpose,
            expiration,
            volume_constraints,
            use_mode,
        } = &grant.payload
        else {
            continue;
        };

        // Step 2: subtract revoked Grants (validated revocation present).
        if grant_revoked(subgraph, grant.id) {
            continue;
        }
        // Step 3: requesting institution must match the Grant's audience.
        if !audience_matches(audience, &query.requester) {
            continue;
        }
        // Step 4: scope, purpose, use-mode.
        if *purpose != query.purpose
            || !use_mode_permits(*use_mode, query.use_mode)
            || !scope_covers(scope, query)
        {
            continue;
        }
        // Step 5: expiration and volume.
        if let Some(exp) = expiration {
            if let Some(exp_secs) = parse_rfc3339_unix(exp) {
                if exp_secs < now_unix_secs {
                    continue; // expired
                }
            }
        }
        if let Some(vc) = volume_constraints {
            if let Some(max) = vc.max_requests {
                if utilization.get(&grant.id).copied().unwrap_or(0) >= max {
                    continue; // volume exhausted
                }
            }
        }

        // Step 5.5: Link-chain check. Only applies when the caller asked for it via
        // `evaluate_with_link_chain`. Defends against rogue-Link cross-institutional attacks
        // (see [`crate::link_chain`]).
        if let Some((anchors, cfg)) = link_chain {
            match evaluate_link_chain(subgraph, grant.id, anchors, cfg) {
                LinkChainResult::Ok => {}
                LinkChainResult::Deny { reason } => {
                    link_chain_denials.push(format!("grant {}: {reason}", grant.id));
                    continue;
                }
            }
        }

        covering.push(grant.id);
    }

    // Step 7: determine outcome.
    if !covering.is_empty() {
        return AuthorizationDecision {
            authorized: true,
            covering_grants: covering,
            reason: "covered by an active AuthorizationGrant (§4.6 steps 1–5)".into(),
        };
    }

    // If Link-chain denial happened for at least one Grant, surface it so the responder logs
    // why apparently-covering Grants were filtered out. This is what makes a rogue-Link attack
    // visible to operators instead of silently denied.
    if !link_chain_denials.is_empty() {
        return AuthorizationDecision {
            authorized: false,
            covering_grants: Vec::new(),
            reason: format!(
                "no covering Grant after §4.6 step 5.5 Link-chain check; {} candidate Grant(s) rejected: {}",
                link_chain_denials.len(),
                link_chain_denials.join("; ")
            ),
        };
    }

    // No covering Grant. Research/AI/federal always require an explicit Grant.
    if requires_explicit_grant(query.purpose) {
        return AuthorizationDecision {
            authorized: false,
            covering_grants: Vec::new(),
            reason: "research/AI/federal purpose requires an explicit Grant (§4.6 step 7)".into(),
        };
    }

    match posture {
        DefaultPosture::DenyByDefault => AuthorizationDecision {
            authorized: false,
            covering_grants: Vec::new(),
            reason: "no covering Grant; deny-by-default posture".into(),
        },
        DefaultPosture::TreatmentPresumed if is_tpo(query.purpose) => AuthorizationDecision {
            authorized: true,
            covering_grants: Vec::new(),
            reason: "treatment-presumed authorization (HIPAA TPO)".into(),
        },
        DefaultPosture::TreatmentPresumed => AuthorizationDecision {
            authorized: false,
            covering_grants: Vec::new(),
            reason: "no covering Grant; purpose outside TPO under treatment-presumed posture"
                .into(),
        },
    }
}

/// Step 6 (per-event): whether a peer with fingerprint `responder` may serve `event` onward,
/// honoring the event's originating-institution redistribution policy. The most restrictive
/// of the patient Grant (from [`evaluate`]), this policy, and the responder's own posture
/// governs; an unrecognized `Custom` policy is conservatively denied.
pub fn responder_may_serve(event: &IdentityEventNode, responder: &CertificateFingerprint) -> bool {
    match &event.redistribution_policy {
        None | Some(RedistributionPolicy::Open) => true,
        Some(RedistributionPolicy::NoRedistribution)
        | Some(RedistributionPolicy::OriginatingInstitutionOnly) => {
            &event.institution_id == responder
        }
        Some(RedistributionPolicy::Custom(_)) => false,
    }
}

fn grant_revoked(subgraph: &Subgraph, grant_id: EventId) -> bool {
    subgraph
        .nodes_of_type(IdentityEventType::AuthorizationRevocation)
        .any(|rev| match &rev.payload {
            EventPayload::AuthorizationRevocation { target_grant_id } => {
                *target_grant_id == grant_id && revocation_validated(subgraph, rev)
            }
            _ => false,
        })
}

/// A revocation is "validated" for enforcement only once its parent references are resolved
/// locally (§4.6 step 2). Signature verification is performed upstream at ingest.
fn revocation_validated(subgraph: &Subgraph, revocation: &IdentityEventNode) -> bool {
    revocation.parent_ids.iter().all(|p| subgraph.contains(p))
}

fn audience_matches(audience: &GrantAudience, requester: &RequesterContext) -> bool {
    match audience {
        GrantAudience::InstitutionId(fp) => *fp == requester.fingerprint,
        GrantAudience::InstitutionClass(class) => requester.classes.iter().any(|c| c == class),
        GrantAudience::ConstrainedWildcard(w) => requester.wildcards.iter().any(|x| x == w),
    }
}

fn scope_covers(scope: &AuthorizationScope, query: &AuthorizationQuery) -> bool {
    let types_ok = scope.event_types.is_empty()
        || (!query.requested_event_types.is_empty()
            && query
                .requested_event_types
                .iter()
                .all(|t| scope.event_types.contains(t)));
    let segments_ok = scope.subgraph_segments.is_empty()
        || (!query.requested_segments.is_empty()
            && query
                .requested_segments
                .iter()
                .all(|s| scope.subgraph_segments.contains(s)));
    let categories_ok = scope.data_categories.is_empty()
        || (!query.requested_data_categories.is_empty()
            && query
                .requested_data_categories
                .iter()
                .all(|c| scope.data_categories.contains(c)));
    types_ok && segments_ok && categories_ok
}

/// A Grant with use-mode `granted` permits a request for `requested` if the request is no more
/// permissive: ReadOnly < ReadAndRely < ReadAndExport.
fn use_mode_permits(granted: UseMode, requested: UseMode) -> bool {
    use_mode_rank(requested) <= use_mode_rank(granted)
}

fn use_mode_rank(m: UseMode) -> u8 {
    match m {
        UseMode::ReadOnly => 0,
        UseMode::ReadAndRely => 1,
        UseMode::ReadAndExport => 2,
    }
}

fn requires_explicit_grant(purpose: GrantPurpose) -> bool {
    matches!(
        purpose,
        GrantPurpose::Research
            | GrantPurpose::AiTraining
            | GrantPurpose::AiInference
            | GrantPurpose::FederalProgram
    )
}

fn is_tpo(purpose: GrantPurpose) -> bool {
    matches!(
        purpose,
        GrantPurpose::Treatment | GrantPurpose::Payment | GrantPurpose::Operations
    )
}

fn parse_rfc3339_unix(rfc3339: &str) -> Option<i64> {
    time::OffsetDateTime::parse(rfc3339, &time::format_description::well_known::Rfc3339)
        .ok()
        .map(|t| t.unix_timestamp())
}
