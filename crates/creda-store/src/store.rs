//! The `Store` trait — the backend-agnostic storage interface (spec §7.3.1, §7.4.1).
//!
//! All identity logic lives above this layer; a `Store` only persists and indexes events.
//! The trait is object-safe (`dyn Store`) and synchronous — storage is blocking I/O, and
//! Creda Core (M5) will wrap calls in `spawn_blocking` for its async runtime.
//!
//! The four secondary indexes are exactly those listed in spec §5.2.5:
//! 1. demographic token → subgraph entry points  ([`Store::entry_points_by_token`])
//! 2. institution id → event UUIDs               ([`Store::events_by_institution`])
//! 3. event UUID → event node                    ([`Store::get_event`], the primary index)
//! 4. parent UUID → child UUIDs                   ([`Store::children_of`])

use creda_events::{CertificateFingerprint, EventId, IdentityEventNode};

use crate::error::Result;

/// A backend-agnostic event store with the secondary indexes from spec §5.2.5.
///
/// Methods returning collections of [`EventId`] return them sorted (UUIDv7 byte order, which
/// is creation-time order), so results are deterministic across backends.
pub trait Store: Send + Sync {
    /// Persist an event and update every secondary index. Idempotent: storing the same event
    /// twice is a no-op beyond overwriting identical data. The event is **not** validated or
    /// signature-checked here — callers do that (creda-events / the responding peer); the
    /// store only persists and indexes.
    fn put_event(&self, node: &IdentityEventNode) -> Result<()>;

    /// Retrieve an event by its UUID (primary index). `None` if not present.
    fn get_event(&self, id: &EventId) -> Result<Option<IdentityEventNode>>;

    /// Whether an event with this UUID is present.
    fn has_event(&self, id: &EventId) -> Result<bool> {
        Ok(self.get_event(id)?.is_some())
    }

    /// All event UUIDs in the store, sorted. Used for bootstrap, rebuild, snapshots, and audit.
    fn all_event_ids(&self) -> Result<Vec<EventId>>;

    /// Index 4 — the UUIDs of events that reference `parent` as a parent (forward traversal).
    /// Needed for computing leaf nodes and for propagating tombstone effects forward (§5.2.5).
    fn children_of(&self, parent: &EventId) -> Result<Vec<EventId>>;

    /// Index 2 — all event UUIDs created by the given institution (institutional audit, §5.2.5).
    fn events_by_institution(&self, institution: &CertificateFingerprint) -> Result<Vec<EventId>>;

    /// Index 1 — the UUIDs of events carrying the given demographic token. These are the
    /// subgraph entry points an institution's matching logic looks up at registration (§5.2.5).
    /// The token is an opaque tokenized demographic value (see [`crate::demographic_tokens`]).
    fn entry_points_by_token(&self, token: &str) -> Result<Vec<EventId>>;

    /// Rebuild every secondary index from the primary event store. Indexes are local to a peer
    /// and rebuilt from the event store on bootstrap (§5.2.5); this is that operation, and is
    /// also how a peer recovers from index corruption.
    fn rebuild_indexes(&self) -> Result<()>;
}
