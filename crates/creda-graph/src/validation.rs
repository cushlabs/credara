//! Graph-dependent event invariants — the rules deferred from `creda-events` (M1) because
//! they need traversal context.
//!
//! - **Contest party-of-the-subgraph** (§3.4.3): a `Contest` may only be created by an
//!   institution party to the linked subgraphs — the institution that created the `Link`, or
//!   any institution that created an `Assert`/`Attest`/`Amend` within either linked subgraph.
//! - **Amend originating-institution** (§3.4.5): an `Amend` must be signed by the same
//!   institution that signed its target. (Successor keys via a rotation chain are valid too;
//!   key rotation is not modeled until the trust layer, so this checks fingerprint equality
//!   and leaves a note for the rotation case.)

use std::collections::{HashSet, VecDeque};

use creda_events::{CertificateFingerprint, EventId, EventPayload, IdentityEventType};
use creda_events::IdentityEventNode;

use crate::error::{Error, Result};
use crate::subgraph::Subgraph;

/// Validate a node's graph-dependent invariants given its subgraph context. No-op for event
/// types that have none.
pub fn validate_node(subgraph: &Subgraph, node: &IdentityEventNode) -> Result<()> {
    match node.event_type {
        IdentityEventType::Contest => validate_contest(subgraph, node),
        IdentityEventType::Amend => validate_amend(subgraph, node),
        _ => Ok(()),
    }
}

/// Whether a Contest passes the party-of-the-subgraph rule (convenience wrapper).
pub fn contest_is_valid(subgraph: &Subgraph, contest: &IdentityEventNode) -> bool {
    validate_contest(subgraph, contest).is_ok()
}

/// Enforce the Contest party-of-the-subgraph rule (§3.4.3).
pub fn validate_contest(subgraph: &Subgraph, contest: &IdentityEventNode) -> Result<()> {
    let EventPayload::Contest { target_link_id, .. } = &contest.payload else {
        return Err(Error::Invariant("validate_contest called on a non-Contest event".into()));
    };

    let link = subgraph.get(target_link_id).ok_or_else(|| {
        Error::Inconsistent("contest target Link is not present in the subgraph".into())
    })?;
    let EventPayload::Link { target_subgraph_heads, .. } = &link.payload else {
        return Err(Error::Invariant("contest target is not a Link event".into()));
    };

    // The party set: the Link's creator, plus every institution that created an
    // Assert/Attest/Amend in either linked subgraph (the component on each side of the Link,
    // not crossing the Link itself).
    let mut party: HashSet<CertificateFingerprint> = HashSet::new();
    party.insert(link.institution_id.clone());

    for head in [target_subgraph_heads.0, target_subgraph_heads.1] {
        for member in connected_component(subgraph, head, Some(*target_link_id)) {
            if let Some(n) = subgraph.get(&member) {
                if matches!(
                    n.event_type,
                    IdentityEventType::Assert | IdentityEventType::Attest | IdentityEventType::Amend
                ) {
                    party.insert(n.institution_id.clone());
                }
            }
        }
    }

    if party.contains(&contest.institution_id) {
        Ok(())
    } else {
        Err(Error::Invariant(
            "Contest must be created by a party to the linked subgraphs (§3.4.3)".into(),
        ))
    }
}

/// Enforce the Amend originating-institution rule (§3.4.5).
pub fn validate_amend(subgraph: &Subgraph, amend: &IdentityEventNode) -> Result<()> {
    let EventPayload::Amend { target_event_id, .. } = &amend.payload else {
        return Err(Error::Invariant("validate_amend called on a non-Amend event".into()));
    };

    let target = subgraph.get(target_event_id).ok_or_else(|| {
        Error::Inconsistent("amend target is not present in the subgraph".into())
    })?;

    // TODO(trust-layer): also accept a successor key with a valid rotation chain (§3.6); key
    // rotation is not modeled until the identity/trust layer, so for now require the same
    // institutional fingerprint.
    if amend.institution_id == target.institution_id {
        Ok(())
    } else {
        Err(Error::Invariant(
            "Amend must be signed by the originating institution of its target (§3.4.5)".into(),
        ))
    }
}

/// The connected component of `start` within `subgraph`, traversing parent edges in both
/// directions (undirected) but never through `exclude` (used to keep the two sides of a
/// contested Link separate).
fn connected_component(
    subgraph: &Subgraph,
    start: EventId,
    exclude: Option<EventId>,
) -> HashSet<EventId> {
    let mut seen: HashSet<EventId> = HashSet::new();
    if Some(start) == exclude || !subgraph.contains(&start) {
        return seen;
    }
    let mut queue: VecDeque<EventId> = VecDeque::new();
    seen.insert(start);
    queue.push_back(start);

    while let Some(id) = queue.pop_front() {
        let Some(node) = subgraph.get(&id) else { continue };
        let mut neighbors: Vec<EventId> = node.parent_ids.clone();
        neighbors.extend(subgraph.children_of(&id));
        for n in neighbors {
            if Some(n) == exclude {
                continue;
            }
            if subgraph.contains(&n) && seen.insert(n) {
                queue.push_back(n);
            }
        }
    }
    seen
}
