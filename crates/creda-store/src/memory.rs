//! In-memory [`Store`] backend — always available.
//!
//! Used by the crate's contract tests and by downstream crates (e.g. creda-graph, M3) that
//! want the trait without the RocksDB compile. Backed by a `Mutex` so the trait's `&self`
//! methods can mutate; not optimized for concurrency, but the semantics match the persistent
//! backends exactly, which is what the contract tests verify.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Mutex;

use creda_events::{CertificateFingerprint, EventId, IdentityEventNode};

use crate::error::{Error, Result};
use crate::store::Store;
use crate::tokens::demographic_tokens;

#[derive(Default)]
struct Inner {
    events: BTreeMap<EventId, IdentityEventNode>,
    parent_to_children: HashMap<EventId, BTreeSet<EventId>>,
    institution_to_events: HashMap<CertificateFingerprint, BTreeSet<EventId>>,
    token_to_events: HashMap<String, BTreeSet<EventId>>,
}

impl Inner {
    fn index(&mut self, node: &IdentityEventNode) {
        let id = node.id;
        self.institution_to_events
            .entry(node.institution_id.clone())
            .or_default()
            .insert(id);
        for parent in &node.parent_ids {
            self.parent_to_children.entry(*parent).or_default().insert(id);
        }
        for token in demographic_tokens(node) {
            self.token_to_events.entry(token).or_default().insert(id);
        }
    }
}

/// An in-memory event store.
#[derive(Default)]
pub struct MemoryStore(Mutex<Inner>);

impl MemoryStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Inner>> {
        self.0
            .lock()
            .map_err(|_| Error::Backend("memory store mutex poisoned".into()))
    }
}

impl Store for MemoryStore {
    fn put_event(&self, node: &IdentityEventNode) -> Result<()> {
        let mut inner = self.lock()?;
        inner.events.insert(node.id, node.clone());
        inner.index(node);
        Ok(())
    }

    fn get_event(&self, id: &EventId) -> Result<Option<IdentityEventNode>> {
        Ok(self.lock()?.events.get(id).cloned())
    }

    fn all_event_ids(&self) -> Result<Vec<EventId>> {
        Ok(self.lock()?.events.keys().copied().collect())
    }

    fn children_of(&self, parent: &EventId) -> Result<Vec<EventId>> {
        Ok(self
            .lock()?
            .parent_to_children
            .get(parent)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default())
    }

    fn events_by_institution(&self, institution: &CertificateFingerprint) -> Result<Vec<EventId>> {
        Ok(self
            .lock()?
            .institution_to_events
            .get(institution)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default())
    }

    fn entry_points_by_token(&self, token: &str) -> Result<Vec<EventId>> {
        Ok(self
            .lock()?
            .token_to_events
            .get(token)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default())
    }

    fn rebuild_indexes(&self) -> Result<()> {
        let mut inner = self.lock()?;
        inner.parent_to_children.clear();
        inner.institution_to_events.clear();
        inner.token_to_events.clear();
        // Re-derive from the primary event store.
        let nodes: Vec<IdentityEventNode> = inner.events.values().cloned().collect();
        for node in &nodes {
            inner.index(node);
        }
        Ok(())
    }
}
