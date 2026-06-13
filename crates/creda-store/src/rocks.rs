//! RocksDB-backed [`Store`] — the default embedded backend (spec §7.3.1, §7.4.1).
//!
//! Layout: one column family per index (spec §5.2.5). Index entries use composite keys so
//! lookups are RocksDB prefix scans (no read-modify-write of a serialized set, so writes are
//! idempotent and concurrency-friendly):
//!
//! | Column family    | Key                                   | Value | Lookup |
//! |------------------|---------------------------------------|-------|--------|
//! | `events`         | `uuid` (16)                           | CBOR  | primary get |
//! | `idx_institution`| `blake3(fp)` (32) ‖ `uuid` (16)        | empty | prefix = `blake3(fp)` |
//! | `idx_parent`     | `parent_uuid` (16) ‖ `child_uuid` (16)| empty | prefix = `parent_uuid` |
//! | `idx_token`      | `blake3(token)` (32) ‖ `uuid` (16)    | empty | prefix = `blake3(token)` |
//!
//! Institution and token keys are hashed to a fixed 32-byte prefix so prefix scans are
//! length-safe regardless of the raw value's length; the trailing 16 bytes are always the
//! event UUID. Iteration is in sorted key order, so results come back in UUIDv7 (creation
//! time) order.

use std::path::Path;

use creda_events::{canonical, CertificateFingerprint, ContentHash, EventId, IdentityEventNode};
use rocksdb::{ColumnFamilyDescriptor, Direction, IteratorMode, Options, DB};

use crate::error::{Error, Result};
use crate::store::Store;
use crate::tokens::demographic_tokens;

const CF_EVENTS: &str = "events";
const CF_IDX_INSTITUTION: &str = "idx_institution";
const CF_IDX_PARENT: &str = "idx_parent";
const CF_IDX_TOKEN: &str = "idx_token";

const ALL_CFS: [&str; 4] = [CF_EVENTS, CF_IDX_INSTITUTION, CF_IDX_PARENT, CF_IDX_TOKEN];

/// A RocksDB-backed event store.
pub struct RocksdbStore {
    db: DB,
}

impl RocksdbStore {
    /// Open (creating if absent) a store at `path`, ensuring all column families exist.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let cfs: Vec<ColumnFamilyDescriptor> = ALL_CFS
            .iter()
            .map(|name| ColumnFamilyDescriptor::new(*name, Options::default()))
            .collect();

        let db = DB::open_cf_descriptors(&opts, path, cfs)?;
        Ok(Self { db })
    }

    fn cf(&self, name: &str) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(name)
            .ok_or_else(|| Error::Backend(format!("missing column family {name}")))
    }

    /// Write all secondary-index entries for a node (idempotent).
    fn index_node(&self, node: &IdentityEventNode) -> Result<()> {
        let id = node.id.as_bytes();

        let inst_cf = self.cf(CF_IDX_INSTITUTION)?;
        self.db.put_cf(
            inst_cf,
            prefixed_key(&hash32(node.institution_id.as_bytes()), id),
            b"",
        )?;

        let parent_cf = self.cf(CF_IDX_PARENT)?;
        for parent in &node.parent_ids {
            self.db
                .put_cf(parent_cf, prefixed_key(parent.as_bytes(), id), b"")?;
        }

        let token_cf = self.cf(CF_IDX_TOKEN)?;
        for token in demographic_tokens(node) {
            self.db
                .put_cf(token_cf, prefixed_key(&hash32(token.as_bytes()), id), b"")?;
        }
        Ok(())
    }

    /// Prefix-scan an index column family, returning the trailing-UUID of every matching key.
    fn scan_ids(&self, cf_name: &str, prefix: &[u8]) -> Result<Vec<EventId>> {
        let cf = self.cf(cf_name)?;
        let mut out = Vec::new();
        let iter = self
            .db
            .iterator_cf(cf, IteratorMode::From(prefix, Direction::Forward));
        for item in iter {
            let (key, _) = item?;
            if !key.starts_with(prefix) {
                break;
            }
            out.push(uuid_from_tail(&key)?);
        }
        Ok(out)
    }

    fn clear_cf(&self, cf_name: &str) -> Result<()> {
        let cf = self.cf(cf_name)?;
        let keys: Vec<Box<[u8]>> = self
            .db
            .iterator_cf(cf, IteratorMode::Start)
            .map(|item| item.map(|(k, _)| k))
            .collect::<std::result::Result<_, _>>()?;
        for key in keys {
            self.db.delete_cf(cf, key)?;
        }
        Ok(())
    }
}

impl Store for RocksdbStore {
    fn put_event(&self, node: &IdentityEventNode) -> Result<()> {
        let events_cf = self.cf(CF_EVENTS)?;
        let bytes = canonical::to_vec(node)?;
        self.db.put_cf(events_cf, node.id.as_bytes(), bytes)?;
        self.index_node(node)
    }

    fn get_event(&self, id: &EventId) -> Result<Option<IdentityEventNode>> {
        let events_cf = self.cf(CF_EVENTS)?;
        match self.db.get_cf(events_cf, id.as_bytes())? {
            Some(bytes) => Ok(Some(canonical::from_slice(&bytes)?)),
            None => Ok(None),
        }
    }

    fn all_event_ids(&self) -> Result<Vec<EventId>> {
        let events_cf = self.cf(CF_EVENTS)?;
        let mut out = Vec::new();
        for item in self.db.iterator_cf(events_cf, IteratorMode::Start) {
            let (key, _) = item?;
            out.push(
                EventId::from_slice(&key)
                    .map_err(|e| Error::Corrupt(format!("bad event-store key: {e}")))?,
            );
        }
        Ok(out)
    }

    fn children_of(&self, parent: &EventId) -> Result<Vec<EventId>> {
        self.scan_ids(CF_IDX_PARENT, parent.as_bytes())
    }

    fn events_by_institution(&self, institution: &CertificateFingerprint) -> Result<Vec<EventId>> {
        self.scan_ids(CF_IDX_INSTITUTION, &hash32(institution.as_bytes()))
    }

    fn entry_points_by_token(&self, token: &str) -> Result<Vec<EventId>> {
        self.scan_ids(CF_IDX_TOKEN, &hash32(token.as_bytes()))
    }

    fn rebuild_indexes(&self) -> Result<()> {
        self.clear_cf(CF_IDX_INSTITUTION)?;
        self.clear_cf(CF_IDX_PARENT)?;
        self.clear_cf(CF_IDX_TOKEN)?;
        for id in self.all_event_ids()? {
            if let Some(node) = self.get_event(&id)? {
                self.index_node(&node)?;
            }
        }
        Ok(())
    }
}

/// Blake3 of `data` as a fixed 32-byte array (reuses creda-events' hash to avoid a second
/// blake3 dependency).
fn hash32(data: &[u8]) -> [u8; 32] {
    let digest = ContentHash::blake3(data).digest;
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest[..32]);
    out
}

/// Build a composite index key: `prefix` followed by the 16-byte event UUID.
fn prefixed_key(prefix: &[u8], uuid_bytes: &[u8; 16]) -> Vec<u8> {
    let mut key = Vec::with_capacity(prefix.len() + 16);
    key.extend_from_slice(prefix);
    key.extend_from_slice(uuid_bytes);
    key
}

/// Parse the event UUID from the trailing 16 bytes of a composite index key.
fn uuid_from_tail(key: &[u8]) -> Result<EventId> {
    if key.len() < 16 {
        return Err(Error::Corrupt(format!(
            "index key too short ({} bytes)",
            key.len()
        )));
    }
    let tail = &key[key.len() - 16..];
    EventId::from_slice(tail).map_err(|e| Error::Corrupt(format!("bad uuid in index key: {e}")))
}
