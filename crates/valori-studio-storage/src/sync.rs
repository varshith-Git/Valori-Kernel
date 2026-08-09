// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Studio-side synchronization bookkeeping — **not** the sync engine.
//!
//! This table records, per project, what Studio last observed about
//! syncing that project's state with Valori Cloud: when it last
//! successfully synced, what remote version/ETag it last saw, and whether
//! local state is known to be `dirty` (changed since last sync) or in
//! `conflict`. It contains no logic for actually performing a sync —
//! there is no sync engine in this crate or this phase.
//!
//! # Cloud remains authoritative
//!
//! For a Cloud project, Cloud's own row is the source of truth for what
//! state the project is actually in. A `StudioSyncState` row is Studio's
//! local *belief* about sync progress — useful for showing "syncing…" /
//! "synced 2 minutes ago" / "conflict" in the UI without a live
//! round-trip, and for deciding when to attempt the next sync — but it
//! must never be treated as overriding what Cloud reports. If Studio's
//! `last_sync`/`dirty` and Cloud's actual state ever disagree, Cloud wins
//! by definition (see `docs/architecture/studio-storage-audit.md` §7 and
//! §"Separate authoritative state from cache").

use redb::Database;
use serde::{Deserialize, Serialize};
use valori_domain::ProjectId;

use crate::error::StudioStorageResult;
use crate::schema::{self, SYNC_STATE};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StudioSyncState {
    pub project_id: ProjectId,
    #[serde(default)]
    pub last_sync: Option<i64>,
    #[serde(default)]
    pub remote_version: Option<String>,
    #[serde(default)]
    pub dirty: bool,
    #[serde(default)]
    pub conflict: bool,
}

impl StudioSyncState {
    pub fn fresh(project_id: ProjectId) -> Self {
        Self {
            project_id,
            last_sync: None,
            remote_version: None,
            dirty: false,
            conflict: false,
        }
    }
}

pub struct SyncStateStore<'a> {
    db: &'a Database,
}

impl<'a> SyncStateStore<'a> {
    pub(crate) fn new(db: &'a Database) -> Self {
        Self { db }
    }

    fn key(id: ProjectId) -> String {
        id.to_string()
    }

    pub fn get(&self, project_id: ProjectId) -> StudioStorageResult<Option<StudioSyncState>> {
        schema::get_json(self.db, SYNC_STATE, &Self::key(project_id))
    }

    pub fn set(&self, state: &StudioSyncState) -> StudioStorageResult<()> {
        schema::put_json(self.db, SYNC_STATE, &Self::key(state.project_id), state)
    }

    /// Atomic read-modify-write, starting from [`StudioSyncState::fresh`]
    /// if nothing is stored yet for `project_id`.
    pub fn update(
        &self,
        project_id: ProjectId,
        f: impl FnOnce(&mut StudioSyncState),
    ) -> StudioStorageResult<StudioSyncState> {
        schema::update_json(
            self.db,
            SYNC_STATE,
            &Self::key(project_id),
            move || StudioSyncState::fresh(project_id),
            f,
        )
    }

    pub fn delete(&self, project_id: ProjectId) -> StudioStorageResult<bool> {
        schema::delete_key(self.db, SYNC_STATE, &Self::key(project_id))
    }

    pub fn list(&self) -> StudioStorageResult<Vec<StudioSyncState>> {
        schema::list_json(self.db, SYNC_STATE)
    }
}
