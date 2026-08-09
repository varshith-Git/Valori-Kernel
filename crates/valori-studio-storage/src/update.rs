// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Studio auto-updater state — **storage layer only**.
//!
//! This does not check for updates, download anything, or install
//! anything. `desktop/src-tauri`'s existing updater (background check +
//! `install_update` command, per `ui/src/lib/native.ts`'s
//! `installUpdate`) is untouched in S1. This is a small typed place a
//! future phase can persist what it already knows — when it last checked,
//! what version it found, whether it downloaded it — instead of that
//! state living only in memory for the current process lifetime.

use redb::Database;
use serde::{Deserialize, Serialize};

use crate::error::StudioStorageResult;
use crate::schema::{self, SINGLETON_KEY, UPDATE_STATE};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct StudioUpdateState {
    #[serde(default)]
    pub last_checked: Option<i64>,
    #[serde(default)]
    pub available_version: Option<String>,
    #[serde(default)]
    pub downloaded: bool,
    #[serde(default)]
    pub downloaded_at: Option<i64>,
}

pub struct UpdateStateStore<'a> {
    db: &'a Database,
}

impl<'a> UpdateStateStore<'a> {
    pub(crate) fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// Never errors on a missing record — returns
    /// [`StudioUpdateState::default`] if no check has ever been recorded.
    pub fn get(&self) -> StudioStorageResult<StudioUpdateState> {
        Ok(schema::get_json(self.db, UPDATE_STATE, SINGLETON_KEY)?.unwrap_or_default())
    }

    pub fn set(&self, state: &StudioUpdateState) -> StudioStorageResult<()> {
        schema::put_json(self.db, UPDATE_STATE, SINGLETON_KEY, state)
    }

    pub fn update(
        &self,
        f: impl FnOnce(&mut StudioUpdateState),
    ) -> StudioStorageResult<StudioUpdateState> {
        schema::update_json(
            self.db,
            UPDATE_STATE,
            SINGLETON_KEY,
            StudioUpdateState::default,
            f,
        )
    }
}
