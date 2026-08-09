// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! The Studio-local project registry: local projects, cloud project
//! references, favorites, and recency.
//!
//! # `StudioProjectRecord` is not `Project`
//!
//! `valori_domain::Project` (or `ProjectManifest`, or Cloud's row) already
//! owns what a project *is* — see `docs/architecture/ownership.md`.
//! `StudioProjectRecord` is deliberately a thinner, Studio-specific
//! persistence record: how the desktop app remembers a project locally
//! (which one was opened last, which are favorited), not a second
//! authoritative copy of project meaning. It is built *around*
//! [`valori_domain::ProjectId`], never a replacement for it.
//!
//! # Identity discipline (CLAUDE.md item 8)
//!
//! `ProjectId` is the only key. Every mutating method here takes an
//! existing `ProjectId` and updates fields *on that record* — none of them
//! mint a new id, derive one from a path or name, or silently replace one
//! record with another under a different key. Re-registering a project
//! that is already present (same id) merges: identity, `favorite`, and
//! `registered_at` survive; only the fields the call actually describes
//! (display name, path) change. See `Self::register_local`.
//!
//! # Authoritative for Studio's own bookkeeping only
//!
//! `favorite`, `last_opened_at`, and "this project is registered with
//! Studio at all" are genuine Studio-local facts with no other source of
//! truth. `display_name` and `local_path`/cloud reference fields are
//! **not** authoritative — the daemon's `project.json` (local) or Cloud's
//! `projects` table (cloud) own those; this registry's copies exist so
//! Studio can render a project list/switcher without a live round-trip,
//! and are expected to be refreshed from those sources, not treated as the
//! last word on what a project is named or where it lives.

use std::path::{Path, PathBuf};

use redb::Database;
use serde::{Deserialize, Serialize};
use valori_domain::ProjectId;

use crate::error::StudioStorageResult;
use crate::schema::{self, PROJECTS};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectKind {
    /// A project whose data lives on this machine, under the daemon's
    /// project directory. `path` is a cache of where it currently is —
    /// authoritative location is the daemon's `project.json`.
    Local { path: PathBuf },
    /// A reference to a project whose data is authoritative in Valori
    /// Cloud. Deliberately thin — see module docs and
    /// `docs/architecture/studio-storage.md` §"Cloud projects": no secret,
    /// no full Cloud row, just enough to render an offline-aware list and
    /// resolve back to Cloud.
    Cloud {
        /// Opaque reference string, not a typed `OrganizationId` — that
        /// type belongs to the private Cloud control plane and must not
        /// be defined or depended on here (dependency_direction.rs).
        #[serde(default)]
        organization_id: Option<String>,
        cloud_endpoint: String,
        #[serde(default)]
        region: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StudioProjectRecord {
    pub id: ProjectId,
    pub display_name: String,
    pub kind: ProjectKind,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub last_opened_at: Option<i64>,
    /// When this record was first registered with Studio. Never changes on
    /// re-registration — see module docs.
    pub registered_at: i64,
}

pub struct ProjectRegistry<'a> {
    db: &'a Database,
}

impl<'a> ProjectRegistry<'a> {
    pub(crate) fn new(db: &'a Database) -> Self {
        Self { db }
    }

    fn key(id: ProjectId) -> String {
        id.to_string()
    }

    pub fn get(&self, id: ProjectId) -> StudioStorageResult<Option<StudioProjectRecord>> {
        schema::get_json(self.db, PROJECTS, &Self::key(id))
    }

    pub fn list(&self) -> StudioStorageResult<Vec<StudioProjectRecord>> {
        schema::list_json(self.db, PROJECTS)
    }

    /// Projects with `favorite == true`, in registry order (no separate
    /// favorites list is kept — see module docs: favorite is a field on
    /// the one authoritative record, not a second list that could drift
    /// out of sync with it).
    pub fn favorites(&self) -> StudioStorageResult<Vec<StudioProjectRecord>> {
        Ok(self.list()?.into_iter().filter(|p| p.favorite).collect())
    }

    /// Registered projects sorted by `last_opened_at` descending (never
    /// opened = last), truncated to `limit`. Derived from the one
    /// authoritative table on every call — never a separately maintained
    /// list that could fall out of sync with it.
    pub fn recent(&self, limit: usize) -> StudioStorageResult<Vec<StudioProjectRecord>> {
        let mut all = self.list()?;
        all.sort_by(|a, b| b.last_opened_at.cmp(&a.last_opened_at));
        all.truncate(limit);
        Ok(all)
    }

    /// Registers a local project, or updates the existing record for `id`
    /// if one is already registered. Preserves `favorite`,
    /// `last_opened_at`, and `registered_at` across re-registration —
    /// only `display_name`, `kind`, are set from the arguments. This is
    /// what makes "restart", "rename", "path change", and
    /// "re-registration" all preserve identity: the row keyed by `id`
    /// never moves, and calling this again with a new `display_name` or
    /// `path` updates fields on the same row rather than creating a
    /// second one.
    pub fn register_local(
        &self,
        id: ProjectId,
        display_name: &str,
        path: &Path,
        now: i64,
    ) -> StudioStorageResult<StudioProjectRecord> {
        self.upsert(
            id,
            display_name,
            ProjectKind::Local {
                path: path.to_path_buf(),
            },
            now,
        )
    }

    /// Registers (or updates) a cloud project reference. Same
    /// identity-preserving upsert semantics as [`Self::register_local`].
    /// Never accepts credentials — see module docs and
    /// `docs/architecture/studio-storage.md` §"Security".
    pub fn register_cloud_ref(
        &self,
        id: ProjectId,
        display_name: &str,
        organization_id: Option<String>,
        cloud_endpoint: &str,
        region: Option<String>,
        now: i64,
    ) -> StudioStorageResult<StudioProjectRecord> {
        self.upsert(
            id,
            display_name,
            ProjectKind::Cloud {
                organization_id,
                cloud_endpoint: cloud_endpoint.to_string(),
                region,
            },
            now,
        )
    }

    fn upsert(
        &self,
        id: ProjectId,
        display_name: &str,
        kind: ProjectKind,
        now: i64,
    ) -> StudioStorageResult<StudioProjectRecord> {
        let key = Self::key(id);
        let existing = schema::get_json::<StudioProjectRecord>(self.db, PROJECTS, &key)?;
        let record = StudioProjectRecord {
            id,
            display_name: display_name.to_string(),
            kind,
            favorite: existing.as_ref().map(|r| r.favorite).unwrap_or(false),
            last_opened_at: existing.as_ref().and_then(|r| r.last_opened_at),
            registered_at: existing.as_ref().map(|r| r.registered_at).unwrap_or(now),
        };
        schema::put_json(self.db, PROJECTS, &key, &record)?;
        Ok(record)
    }

    /// Renames the record in place (same id, same everything else).
    /// No-op error (`NotFound`) if `id` is not registered — callers must
    /// register before renaming, never silently create-on-rename (that
    /// would let a typo mint a phantom project record).
    pub fn rename(
        &self,
        id: ProjectId,
        new_display_name: &str,
    ) -> StudioStorageResult<StudioProjectRecord> {
        self.mutate(id, |r| r.display_name = new_display_name.to_string())
    }

    /// Updates a local project's cached path (the project moved on disk).
    /// Only valid on a `ProjectKind::Local` record; a cloud reference's
    /// `kind` is untouched by this — call [`Self::register_cloud_ref`]
    /// again to update cloud fields.
    pub fn set_local_path(
        &self,
        id: ProjectId,
        new_path: &Path,
    ) -> StudioStorageResult<StudioProjectRecord> {
        self.mutate(id, |r| {
            if let ProjectKind::Local { path } = &mut r.kind {
                *path = new_path.to_path_buf();
            }
        })
    }

    pub fn set_favorite(
        &self,
        id: ProjectId,
        favorite: bool,
    ) -> StudioStorageResult<StudioProjectRecord> {
        self.mutate(id, |r| r.favorite = favorite)
    }

    pub fn touch_last_opened(
        &self,
        id: ProjectId,
        at: i64,
    ) -> StudioStorageResult<StudioProjectRecord> {
        self.mutate(id, |r| r.last_opened_at = Some(at))
    }

    fn mutate(
        &self,
        id: ProjectId,
        f: impl FnOnce(&mut StudioProjectRecord),
    ) -> StudioStorageResult<StudioProjectRecord> {
        let key = Self::key(id);
        let mut record: StudioProjectRecord = schema::get_json(self.db, PROJECTS, &key)?
            .ok_or_else(|| crate::error::StudioStorageError::NotFound(format!("project {id}")))?;
        f(&mut record);
        schema::put_json(self.db, PROJECTS, &key, &record)?;
        Ok(record)
    }

    /// Removes the registry entry entirely — the user deleted or
    /// unregistered the project. Does not touch the project's actual data
    /// (that is the daemon's/Cloud's responsibility, never this crate's —
    /// see module docs) and does not touch `project_cache`'s entry for the
    /// same id (that store is independently disposable — see
    /// `crate::project_cache`).
    pub fn unregister(&self, id: ProjectId) -> StudioStorageResult<bool> {
        schema::delete_key(self.db, PROJECTS, &Self::key(id))
    }
}
