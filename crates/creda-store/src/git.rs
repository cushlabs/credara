//! libgit2-backed [`Store`] — **scaffold only** (feature `libgit2`).
//!
//! `TODO(open-question-13.1)`: the storage-substrate trade study (libgit2 vs. RocksDB) is
//! unresolved (spec §13.1, Appendix C.1). Git's data model is itself a signed DAG with parent
//! references, so a libgit2 backend could store the event DAG natively (one repo per
//! institution, patient subgraphs as refs) and get anti-entropy via Git's pack protocol "for
//! free". This module establishes the backend behind the same [`Store`] trait so that
//! adopting it later touches no other crate; the methods are intentionally unimplemented.
//!
//! This file compiles only with the `libgit2` feature enabled.

use std::path::Path;

use creda_events::{CertificateFingerprint, EventId, IdentityEventNode};

use crate::error::{Error, Result};
use crate::store::Store;

/// A libgit2-backed event store (scaffold). See module docs and open question 13.1.
pub struct GitStore {
    repo: git2::Repository,
}

impl GitStore {
    /// Open an existing repository at `path`, or initialize one if absent.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let repo = git2::Repository::open(path)
            .or_else(|_| git2::Repository::init(path))
            .map_err(|e| Error::Backend(format!("git2: {e}")))?;
        Ok(Self { repo })
    }

    /// Access the underlying repository (for the eventual implementation).
    pub fn repository(&self) -> &git2::Repository {
        &self.repo
    }
}

const TODO: &str = "GitStore — TODO(open-question-13.1): libgit2 storage substrate unresolved";

impl Store for GitStore {
    fn put_event(&self, _node: &IdentityEventNode) -> Result<()> {
        Err(Error::Unimplemented(TODO))
    }

    fn get_event(&self, _id: &EventId) -> Result<Option<IdentityEventNode>> {
        Err(Error::Unimplemented(TODO))
    }

    fn all_event_ids(&self) -> Result<Vec<EventId>> {
        Err(Error::Unimplemented(TODO))
    }

    fn children_of(&self, _parent: &EventId) -> Result<Vec<EventId>> {
        Err(Error::Unimplemented(TODO))
    }

    fn events_by_institution(&self, _institution: &CertificateFingerprint) -> Result<Vec<EventId>> {
        Err(Error::Unimplemented(TODO))
    }

    fn entry_points_by_token(&self, _token: &str) -> Result<Vec<EventId>> {
        Err(Error::Unimplemented(TODO))
    }

    fn rebuild_indexes(&self) -> Result<()> {
        Err(Error::Unimplemented(TODO))
    }
}
