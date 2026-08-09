// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Typed session service and Tauri command handlers for `studio.redb`.
//!
//! # Architecture (S2b-2c)
//!
//! ```text
//! Desktop App Startup / Exit (Rust/Tauri lifecycle)
//!        │
//!        ▼
//! `SessionService`
//!        │
//!        ▼
//! `Arc<StudioDatabase>`
//!        │
//!        ▼
//! `studio.redb` (`sessions` table)
//! ```
//!
//! # Separation of Responsibilities
//!
//! - `studio.redb`'s `sessions` table captures Studio desktop application process runs
//!   (startup, clean shutdown, duration, and crash identification).
//! - Session identity is strictly [`valori_domain::SessionId`].
//! - Telemetry queue and uploader remain independent (S2b-2d); telemetry events may reference
//!   the same `SessionId` without duplicating session databases.
//! - One application process run corresponds to exactly one durable session record.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::Manager;
use valori_domain::{InstallationId, SessionId};
use valori_studio_storage::{
    session::{SessionPruneStats, SessionRetentionPolicy, StudioSessionRecord},
    StudioDatabase, StudioStorageResult,
};

/// Public DTO representing a Studio session record.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSessionDto {
    pub id: String,
    pub installation_id: Option<String>,
    pub app_version: String,
    pub platform: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub crashed: bool,
    pub duration_secs: Option<u64>,
}

impl From<StudioSessionRecord> for StudioSessionDto {
    fn from(r: StudioSessionRecord) -> Self {
        let duration_secs = r.ended_at.map(|e| {
            if e >= r.started_at {
                ((e - r.started_at) / 1000) as u64
            } else {
                0
            }
        });
        Self {
            id: r.id.to_string(),
            installation_id: r.installation_id.map(|i| i.to_string()),
            app_version: r.app_version,
            platform: r.platform,
            started_at: r.started_at,
            ended_at: r.ended_at,
            crashed: r.crashed,
            duration_secs,
        }
    }
}

/// Typed service wrapping session store operations on `StudioDatabase`.
#[derive(Clone)]
pub struct SessionService {
    db: Arc<StudioDatabase>,
}

impl SessionService {
    pub fn new(db: Arc<StudioDatabase>) -> Self {
        Self { db }
    }

    /// Records the start of a session. Idempotent if called multiple times with the same ID.
    pub fn start_session(
        &self,
        id: SessionId,
        installation_id: Option<InstallationId>,
        app_version: &str,
        platform: &str,
        started_at: i64,
    ) -> StudioStorageResult<StudioSessionDto> {
        let record =
            self.db
                .sessions()
                .start(id, installation_id, app_version, platform, started_at)?;
        Ok(StudioSessionDto::from(record))
    }

    /// Marks a session ended. Idempotent.
    pub fn end_session(
        &self,
        id: SessionId,
        ended_at: i64,
        crashed: bool,
    ) -> StudioStorageResult<StudioSessionDto> {
        let record = self.db.sessions().end(id, ended_at, crashed)?;
        Ok(StudioSessionDto::from(record))
    }

    /// Looks up a session by its `SessionId`.
    pub fn get_session(&self, id: SessionId) -> StudioStorageResult<Option<StudioSessionDto>> {
        let record = self.db.sessions().get(id)?;
        Ok(record.map(StudioSessionDto::from))
    }

    /// Lists recent sessions sorted by `started_at` descending.
    pub fn recent_sessions(&self, limit: usize) -> StudioStorageResult<Vec<StudioSessionDto>> {
        let records = self.db.sessions().recent(limit)?;
        Ok(records.into_iter().map(StudioSessionDto::from).collect())
    }

    /// Scans for any open sessions from previous runs (`id != current_session_id`) and marks them crashed.
    pub fn reconcile_crashed_sessions(
        &self,
        current_session_id: SessionId,
        now: i64,
    ) -> StudioStorageResult<usize> {
        self.db
            .sessions()
            .reconcile_crashed(current_session_id, now)
    }

    /// Prunes session history per `policy` (S5 — session retention). See
    /// `valori_studio_storage::session::SessionRetentionPolicy`'s doc
    /// comment for the exact rule. `current_session_id` is never touched.
    pub fn prune_sessions(
        &self,
        current_session_id: SessionId,
        policy: &SessionRetentionPolicy,
        now: i64,
    ) -> StudioStorageResult<SessionPruneStats> {
        self.db.sessions().prune(current_session_id, policy, now)
    }
}

// ── Tauri Commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn session_get_current(app: tauri::AppHandle) -> Result<Option<StudioSessionDto>, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase not initialized".to_string())?;
    let session_id_str = crate::telemetry::get_session_id();
    let session_id = session_id_str
        .parse::<SessionId>()
        .map_err(|e| e.to_string())?;
    let service = SessionService::new(db.inner().clone());
    service.get_session(session_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn session_list_recent(
    app: tauri::AppHandle,
    limit: Option<usize>,
) -> Result<Vec<StudioSessionDto>, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase not initialized".to_string())?;
    let service = SessionService::new(db.inner().clone());
    service
        .recent_sessions(limit.unwrap_or(10))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn session_end_current(app: tauri::AppHandle) -> Result<Option<StudioSessionDto>, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase not initialized".to_string())?;
    let session_id_str = crate::telemetry::get_session_id();
    let session_id = session_id_str
        .parse::<SessionId>()
        .map_err(|e| e.to_string())?;
    let service = SessionService::new(db.inner().clone());
    let now = chrono::Utc::now().timestamp_millis();
    let record = service
        .end_session(session_id, now, false)
        .map_err(|e| e.to_string())?;
    Ok(Some(record))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_session_service_lifecycle_and_invariants() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("studio.redb");
        let db = Arc::new(StudioDatabase::open(&db_path).unwrap());
        let service = SessionService::new(db.clone());

        let session1 = SessionId::new();
        let install_id = InstallationId::new();

        // 1. Start session
        let started = service
            .start_session(session1, Some(install_id), "0.2.4", "macos", 1000)
            .unwrap();
        assert_eq!(started.id, session1.to_string());
        assert_eq!(started.installation_id, Some(install_id.to_string()));
        assert_eq!(started.started_at, 1000);
        assert_eq!(started.ended_at, None);
        assert!(!started.crashed);
        assert_eq!(started.duration_secs, None);

        // 2. Start idempotency (no duplicate or overwrite)
        let restarted = service
            .start_session(session1, Some(install_id), "0.2.4", "macos", 9999)
            .unwrap();
        assert_eq!(restarted.started_at, 1000);

        // 3. End session cleanly
        let ended = service.end_session(session1, 4000, false).unwrap();
        assert_eq!(ended.ended_at, Some(4000));
        assert_eq!(ended.duration_secs, Some(3)); // (4000 - 1000) / 1000
        assert!(!ended.crashed);

        // 4. Multiple sessions remain distinct
        let session2 = SessionId::new();
        service
            .start_session(session2, Some(install_id), "0.2.4", "macos", 5000)
            .unwrap();
        let recents = service.recent_sessions(10).unwrap();
        assert_eq!(recents.len(), 2);
        assert_eq!(recents[0].id, session2.to_string()); // 5000 > 1000
        assert_eq!(recents[1].id, session1.to_string());

        // 5. Crash reconciliation marks unended prior session
        let session3 = SessionId::new();
        service
            .start_session(session3, Some(install_id), "0.2.4", "macos", 10000)
            .unwrap();

        let count = service.reconcile_crashed_sessions(session3, 10000).unwrap();
        assert_eq!(
            count, 1,
            "session2 was left open, so it must be marked crashed"
        );

        let s2_after = service.get_session(session2).unwrap().unwrap();
        assert!(s2_after.crashed);
        assert_eq!(s2_after.ended_at, Some(10000));

        let s3_curr = service.get_session(session3).unwrap().unwrap();
        assert!(!s3_curr.crashed);
        assert_eq!(s3_curr.ended_at, None);

        // 6. Persistence across reopen
        drop(service);
        drop(db);

        let db2 = Arc::new(StudioDatabase::open(&db_path).unwrap());
        let service2 = SessionService::new(db2);
        let s1_persisted = service2.get_session(session1).unwrap().unwrap();
        assert_eq!(s1_persisted.ended_at, Some(4000));
        assert!(!s1_persisted.crashed);
    }

    /// S5: `SessionService::prune_sessions` delegates correctly to the
    /// store — full policy-semantics coverage lives in
    /// `valori-studio-storage`'s own `tests/sessions.rs`; this is a thin
    /// sanity check that the desktop-side wrapper passes the same
    /// arguments through unchanged.
    #[test]
    fn prune_sessions_wrapper_delegates_to_the_store() {
        use valori_studio_storage::session::SessionRetentionPolicy;

        let temp = tempdir().unwrap();
        let db = Arc::new(StudioDatabase::open(&temp.path().join("studio.redb")).unwrap());
        let service = SessionService::new(db);

        let current = SessionId::new();
        service
            .start_session(current, None, "0.2.4", "macos", 1_000 * 86_400_000)
            .unwrap();

        let old = SessionId::new();
        service
            .start_session(old, None, "0.2.4", "macos", 1)
            .unwrap();
        service.end_session(old, 1000, false).unwrap();

        let policy = SessionRetentionPolicy {
            max_completed_sessions: 0,
            completed_retention_days: 90,
            crashed_retention_days: 180,
        };
        let stats = service
            .prune_sessions(current, &policy, 1_000 * 86_400_000)
            .unwrap();

        assert_eq!(stats.deleted, 1);
        assert_eq!(stats.protected_current, 1);
        assert!(service.get_session(old).unwrap().is_none());
        assert!(service.get_session(current).unwrap().is_some());
    }
}
