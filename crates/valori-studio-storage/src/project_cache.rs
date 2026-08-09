// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! The disposable, never-authoritative project display cache.
//!
//! # This table must never become a source of truth
//!
//! `project_cache` exists for exactly one reason: let Studio paint a
//! project list/switcher before the daemon's `/v1/projects` round-trip
//! resolves (today done with a raw `localStorage` blob under
//! `valori:projects-list` — see
//! `docs/architecture/studio-storage-audit.md` §4/§5). It intentionally
//! stores only a handful of small, display-only fields — never a complete
//! `Project`/`ManifestProject` object, and never anything [`crate::project`]
//! already owns (id, favorite, registered_at are that table's job).
//!
//! Deleting every row in this table must have **zero** effect on
//! [`crate::project::ProjectRegistry`] — the two tables are independent,
//! and [`ProjectCacheStore::clear`] never touches `projects`. If a UI ever
//! needs "does project X exist", it must ask the registry or the daemon,
//! never this cache.

use redb::Database;
use serde::{Deserialize, Serialize};
use valori_domain::ProjectId;

use crate::error::StudioStorageResult;
use crate::schema::{self, PROJECT_CACHE};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StudioProjectCacheEntry {
    pub id: ProjectId,
    #[serde(default)]
    pub display_name: Option<String>,
    /// Last status the daemon reported for this project (e.g. `"running"`,
    /// `"stopped"`) — a snapshot, not polled or kept fresh by this crate.
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub record_count: Option<u64>,
    /// When this cache entry was last written. Callers can use this to
    /// decide a cached entry is stale enough to ignore before the daemon
    /// round-trip resolves.
    pub refreshed_at: i64,
}

pub struct ProjectCacheStore<'a> {
    db: &'a Database,
}

impl<'a> ProjectCacheStore<'a> {
    pub(crate) fn new(db: &'a Database) -> Self {
        Self { db }
    }

    fn key(id: ProjectId) -> String {
        id.to_string()
    }

    pub fn get(&self, id: ProjectId) -> StudioStorageResult<Option<StudioProjectCacheEntry>> {
        schema::get_json(self.db, PROJECT_CACHE, &Self::key(id))
    }

    pub fn list(&self) -> StudioStorageResult<Vec<StudioProjectCacheEntry>> {
        schema::list_json(self.db, PROJECT_CACHE)
    }

    /// Full replace of the cached entry for `entry.id`.
    pub fn put(&self, entry: &StudioProjectCacheEntry) -> StudioStorageResult<()> {
        schema::put_json(self.db, PROJECT_CACHE, &Self::key(entry.id), entry)
    }

    pub fn delete(&self, id: ProjectId) -> StudioStorageResult<bool> {
        schema::delete_key(self.db, PROJECT_CACHE, &Self::key(id))
    }

    /// Drops every cached entry. Safe to call at any time — see module
    /// docs; this must never be able to destroy a project or a
    /// [`crate::project::ProjectRegistry`] record.
    pub fn clear(&self) -> StudioStorageResult<usize> {
        let mut removed = 0;
        for entry in self.list()? {
            if self.delete(entry.id)? {
                removed += 1;
            }
        }
        Ok(removed)
    }
}
