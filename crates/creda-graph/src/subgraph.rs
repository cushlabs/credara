//! Subgraph materialization and structural queries (spec §5.2.1–§5.2.3).
//!
//! A patient subgraph is **not stored** — it is the transitive closure of events reachable
//! from a set of entry points by following parent references and payload-referenced ids, plus
//! forward children (§5.2.1). [`Subgraph::materialize`] computes it on demand from a
//! [`Store`]; the result is an in-memory view that the projection (§5.2.4) and authorization
//! (§4.6) algorithms operate over.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use creda_events::{EventId, EventPayload, IdentityEventNode, IdentityEventType};
use creda_store::Store;

use crate::error::Result;

/// A materialized patient subgraph: the events plus a within-set forward (parent→children)
/// index. Construct with [`Subgraph::materialize`].
#[derive(Clone, Debug, Default)]
pub struct Subgraph {
    nodes: BTreeMap<EventId, IdentityEventNode>,
    children: BTreeMap<EventId, BTreeSet<EventId>>,
}

impl Subgraph {
    /// Materialize the connected subgraph reachable from `entry_points`, pulling events from
    /// `store`. Traverses parent references, payload-referenced ids, and forward children so
    /// the whole connected component is captured. Events not present locally are simply
    /// absent (the store reflects what this peer has replicated, §5.2.4).
    pub fn materialize(store: &dyn Store, entry_points: &[EventId]) -> Result<Self> {
        let mut nodes: BTreeMap<EventId, IdentityEventNode> = BTreeMap::new();
        let mut queue: VecDeque<EventId> = entry_points.iter().copied().collect();
        let mut seen: BTreeSet<EventId> = entry_points.iter().copied().collect();

        while let Some(id) = queue.pop_front() {
            let Some(node) = store.get_event(&id)? else {
                continue; // not replicated locally
            };

            let enqueue = |next: EventId, seen: &mut BTreeSet<EventId>, q: &mut VecDeque<EventId>| {
                if seen.insert(next) {
                    q.push_back(next);
                }
            };

            for parent in &node.parent_ids {
                enqueue(*parent, &mut seen, &mut queue);
            }
            for referenced in referenced_ids(&node) {
                enqueue(referenced, &mut seen, &mut queue);
            }
            for child in store.children_of(&id)? {
                enqueue(child, &mut seen, &mut queue);
            }

            nodes.insert(id, node);
        }

        // Build the within-set forward index from parent edges.
        let mut children: BTreeMap<EventId, BTreeSet<EventId>> = BTreeMap::new();
        for node in nodes.values() {
            for parent in &node.parent_ids {
                if nodes.contains_key(parent) {
                    children.entry(*parent).or_default().insert(node.id);
                }
            }
        }

        Ok(Self { nodes, children })
    }

    /// Build a subgraph directly from a set of nodes (used in tests and by callers that have
    /// already gathered events). Equivalent to materializing over those exact nodes.
    pub fn from_nodes(nodes: impl IntoIterator<Item = IdentityEventNode>) -> Self {
        let nodes: BTreeMap<EventId, IdentityEventNode> = nodes.into_iter().map(|n| (n.id, n)).collect();
        let mut children: BTreeMap<EventId, BTreeSet<EventId>> = BTreeMap::new();
        for node in nodes.values() {
            for parent in &node.parent_ids {
                if nodes.contains_key(parent) {
                    children.entry(*parent).or_default().insert(node.id);
                }
            }
        }
        Self { nodes, children }
    }

    /// Number of events in the subgraph.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the subgraph is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Whether the given event is in the subgraph.
    pub fn contains(&self, id: &EventId) -> bool {
        self.nodes.contains_key(id)
    }

    /// Get an event by id.
    pub fn get(&self, id: &EventId) -> Option<&IdentityEventNode> {
        self.nodes.get(id)
    }

    /// Iterate all events (sorted by id).
    pub fn nodes(&self) -> impl Iterator<Item = &IdentityEventNode> {
        self.nodes.values()
    }

    /// Iterate events of a given type (sorted by id).
    pub fn nodes_of_type(
        &self,
        ty: IdentityEventType,
    ) -> impl Iterator<Item = &IdentityEventNode> {
        self.nodes.values().filter(move |n| n.event_type == ty)
    }

    /// Root events — those with no parents (§5.2.2). Multiple roots are normal.
    pub fn roots(&self) -> Vec<EventId> {
        self.nodes
            .values()
            .filter(|n| n.parent_ids.is_empty())
            .map(|n| n.id)
            .collect()
    }

    /// Leaf events — those with no children within the subgraph. The projection starts here
    /// (§5.2.4 step 1).
    pub fn leaves(&self) -> Vec<EventId> {
        self.nodes
            .keys()
            .filter(|id| self.children.get(id).map(|c| c.is_empty()).unwrap_or(true))
            .copied()
            .collect()
    }

    /// The children of `id` within the subgraph (forward traversal index, §5.2.5).
    pub fn children_of(&self, id: &EventId) -> BTreeSet<EventId> {
        self.children.get(id).cloned().unwrap_or_default()
    }
}

/// Event ids referenced by a node's payload (not counting `parent_ids`). Used to widen
/// materialization so that link/contest/amend/tombstone/authorization targets are pulled in.
pub fn referenced_ids(node: &IdentityEventNode) -> Vec<EventId> {
    match &node.payload {
        EventPayload::Link {
            target_subgraph_heads,
            ..
        } => vec![target_subgraph_heads.0, target_subgraph_heads.1],
        EventPayload::Contest { target_link_id, .. } => vec![*target_link_id],
        EventPayload::Attest {
            target_event_ids, ..
        } => target_event_ids.clone(),
        EventPayload::Amend { target_event_id, .. } => vec![*target_event_id],
        EventPayload::Tombstone {
            target_event_ids, ..
        } => target_event_ids.clone(),
        EventPayload::AuthorizationRevocation { target_grant_id } => vec![*target_grant_id],
        EventPayload::ExportReceipt {
            governing_grant_id, ..
        } => vec![*governing_grant_id],
        EventPayload::AuthorizationGrant { scope, .. } => scope.subgraph_segments.clone(),
        EventPayload::Assert { .. } | EventPayload::DeceasedDeclaration { .. } => Vec::new(),
    }
}
