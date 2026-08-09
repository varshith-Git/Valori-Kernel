// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Studio storage initialization, corruption recovery, and legacy
//! persistence migration integration.
//!
//! # Startup Lifecycle (S2b-1, recovery added in the DR phase)
//!
//! 1. Resolve `studio.redb` / `backups/` / `studio-recovery.jsonl` locations
//!    (`~/.valori/...` or `$VALORI_HOME/...`).
//! 2. Open `studio.redb` **with recovery** (`StudioDatabase::open_default_with_recovery`
//!    / `valori_studio_storage::recovery::open_with_recovery`) — corruption
//!    never fails this step; see that module's docs for the full order
//!    (preserve → try backups → fresh fallback).
//! 3. Resolve the real legacy paths using Tauri's path resolver (`app_config_dir()`).
//! 4. Execute legacy migration via the S2a migration engine (`run_legacy_migration`) if required.
//! 5. Verify migration results and log appropriate diagnostics.
//! 6. Manage `Arc<StudioDatabase>` and the recovery status DTO in Tauri
//!    application state, and emit a `studio-recovery` event so the
//!    frontend can show a non-blocking notice — see
//!    `docs/architecture/studio-storage.md` §"Recovery UI".
//!
//! # Backward Compatibility and Invariants
//!
//! - Legacy files (`preferences.json`, `events.jsonl`) are **NEVER modified, renamed, or deleted**.
//! - Migration is **idempotent**: running on startup #2+ detects `already_migrated` and skips re-import.
//! - Fresh installations with no legacy files detect `source_found: false` and proceed smoothly.
//! - Corruption recovery never touches project data (`$VALORI_HOME/projects/**`) —
//!   structurally impossible, not just avoided; see `valori_studio_storage::recovery`
//!   module docs.
//! - `init_studio_storage` never fails the app's `setup()` — Studio storage
//!   is best-effort local convenience state, never load-bearing for the
//!   rest of the app. See its own doc comment.

use std::sync::Arc;
use tauri::{Emitter, Manager};
use tracing::{debug, error, info, warn};
use valori_studio_storage::{
    db::{LegacyMigrationSummary, LegacyStudioPaths},
    path::{default_backups_dir, default_db_path, default_recovery_log_path},
    recovery::open_with_recovery,
    RecoveryOutcome, StudioDatabase, StudioStorageResult,
};

/// Resolves the actual on-disk paths for legacy Studio persistence files
/// (`preferences.json`, `events.jsonl`) using Tauri's path resolution APIs.
///
/// On macOS: `~/Library/Application Support/com.valori.desktop/`
/// On Windows: `%APPDATA%\com.valori.desktop`
/// On Linux: `~/.config/com.valori.desktop`
pub fn resolve_legacy_paths(app: &tauri::AppHandle) -> LegacyStudioPaths {
    let config_dir = app.path().app_config_dir().ok();
    LegacyStudioPaths {
        preferences_json: config_dir.as_ref().map(|d| d.join("preferences.json")),
        events_jsonl: config_dir.as_ref().map(|d| d.join("events.jsonl")),
    }
}

/// Initializes `StudioDatabase` with explicitly provided paths — recovering
/// from corruption if necessary — executes legacy migration, and returns
/// the database handle, what recovery (if any) had to do, and the
/// migration summary.
pub fn init_studio_storage_with_paths(
    db_path: &std::path::Path,
    backups_dir: &std::path::Path,
    recovery_log_path: &std::path::Path,
    legacy_paths: &LegacyStudioPaths,
) -> StudioStorageResult<(Arc<StudioDatabase>, RecoveryOutcome, LegacyMigrationSummary)> {
    info!("Studio storage initialization started");
    let (db, recovery_outcome) = open_with_recovery(db_path, backups_dir, recovery_log_path)?;
    match &recovery_outcome {
        RecoveryOutcome::Healthy => {
            info!("Studio database opened at {}", db_path.display());
        }
        RecoveryOutcome::RestoredFromBackup {
            backup_generation,
            corrupt_original,
        } => {
            warn!(
                "studio database open failed; restored from backup generation \
                 {backup_generation}. Original preserved at {}",
                corrupt_original.display()
            );
        }
        RecoveryOutcome::FreshDatabaseCreated { corrupt_original } => {
            warn!(
                "studio database open failed and no backup was valid; created a \
                 fresh database. {}",
                corrupt_original
                    .as_ref()
                    .map(|p| format!("Original preserved at {}", p.display()))
                    .unwrap_or_else(|| "No original file existed to preserve.".to_string())
            );
        }
    }

    let now = chrono::Utc::now().timestamp_millis();
    let summary = db.run_legacy_migration(legacy_paths, now);
    log_migration_summary(&summary);

    info!("Studio storage initialization completed");
    Ok((Arc::new(db), recovery_outcome, summary))
}

/// Initializes `StudioDatabase` at the standard platform location
/// (`~/.valori/studio.redb`), recovering from corruption if necessary,
/// resolves legacy paths via Tauri, executes legacy migration if needed,
/// manages the resulting `Arc<StudioDatabase>` and a recovery-status DTO
/// in Tauri state, and emits a `studio-recovery` event for the frontend.
///
/// Returns `None` — never an `Err` — on any failure that recovery itself
/// could not resolve (e.g. even a fresh `studio.redb` could not be
/// created — disk full, an unwritable `~/.valori`, …). `studio.redb` is
/// Studio's own local convenience state (preferences, session history,
/// project registry cache, telemetry queue) — it must never become
/// load-bearing for the rest of the app. Before recovery existed, no
/// single file's corruption could prevent Valori Studio from launching at
/// all; this preserves that property even in the pathological
/// double-failure case. The caller is expected to skip `app.manage(...)`
/// when this returns `None`, which every consumer already treats as
/// "Studio storage unavailable" (`app.try_state::<Arc<StudioDatabase>>()`
/// is `None`) — commands that need it fail individually with a clear
/// message, the rest of the app is unaffected. See
/// `docs/architecture/studio-storage.md` §"Corruption behavior".
pub fn init_studio_storage(app: &tauri::AppHandle) -> Option<Arc<StudioDatabase>> {
    let db_path = default_db_path();
    let backups_dir = default_backups_dir();
    let recovery_log_path = default_recovery_log_path();
    let legacy_paths = resolve_legacy_paths(app);

    match init_studio_storage_with_paths(&db_path, &backups_dir, &recovery_log_path, &legacy_paths)
    {
        Ok((db, recovery_outcome, _)) => {
            let status = RecoveryStatusDto::from_outcome(&recovery_outcome);
            app.manage(status.clone());
            let _ = app.emit("studio-recovery", &status);
            Some(db)
        }
        Err(e) => {
            error!(
                "Studio storage unavailable — continuing without it: {e} \
                 (path: {})",
                db_path.display()
            );
            let status = RecoveryStatusDto::unavailable();
            app.manage(status.clone());
            let _ = app.emit("studio-recovery", &status);
            None
        }
    }
}

/// Logs human-readable diagnostic messages for migration outcome without
/// logging any sensitive user data, API keys, or raw telemetry payloads.
fn log_migration_summary(summary: &LegacyMigrationSummary) {
    match &summary.preferences {
        Ok(report) => {
            if report.already_migrated {
                debug!("Legacy preferences already migrated");
            } else if report.source_found {
                info!(
                    "Legacy preferences imported: {} records ({} skipped)",
                    report.imported,
                    report.skipped.len()
                );
            } else {
                debug!("Legacy preferences file not found; no migration needed");
            }
        }
        Err(e) => {
            error!("Failed to migrate legacy preferences: {e}");
        }
    }

    match &summary.telemetry {
        Ok(report) => {
            if report.already_migrated {
                debug!("Legacy telemetry already migrated");
            } else if report.source_found {
                info!(
                    "Legacy telemetry imported: {} records ({} skipped)",
                    report.imported,
                    report.skipped.len()
                );
            } else {
                debug!("Legacy telemetry queue not found; no migration needed");
            }
        }
        Err(e) => {
            error!("Failed to migrate legacy telemetry: {e}");
        }
    }
}

// ── Recovery status — Tauri state, event payload, and command ────────────

/// JSON-serializable projection of [`RecoveryOutcome`] for the frontend.
/// Deliberately carries only what the Recovery UI spec needs (a
/// non-technical `message`, plus enough structured detail to build the
/// "Open recovery folder" affordance) — never a raw `redb`/IO error
/// string; see `RecoveryOutcome::user_message` for why.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecoveryStatusDto {
    Healthy,
    #[serde(rename_all = "camelCase")]
    RestoredFromBackup {
        message: String,
        backup_generation: u32,
        preserved_original_path: String,
    },
    #[serde(rename_all = "camelCase")]
    FreshDatabaseCreated {
        message: String,
        preserved_original_path: Option<String>,
    },
    /// Recovery itself could not produce a usable database (fresh creation
    /// also failed) — Studio storage is disabled for this run. Distinct
    /// from the two variants above: those always end in a working
    /// database; this one does not, and is the one case a dedicated
    /// recovery screen (rather than a toast) would be warranted for, per
    /// the Recovery UI spec's "manual recovery required" path. In
    /// practice this should be exceedingly rare — it requires even a
    /// brand-new empty `studio.redb` to fail to create.
    Unavailable,
}

impl RecoveryStatusDto {
    fn from_outcome(outcome: &RecoveryOutcome) -> Self {
        match outcome {
            RecoveryOutcome::Healthy => RecoveryStatusDto::Healthy,
            RecoveryOutcome::RestoredFromBackup {
                backup_generation,
                corrupt_original,
            } => RecoveryStatusDto::RestoredFromBackup {
                message: outcome.user_message().unwrap_or_default().to_string(),
                backup_generation: *backup_generation,
                preserved_original_path: corrupt_original.display().to_string(),
            },
            RecoveryOutcome::FreshDatabaseCreated { corrupt_original } => {
                RecoveryStatusDto::FreshDatabaseCreated {
                    message: outcome.user_message().unwrap_or_default().to_string(),
                    preserved_original_path: corrupt_original
                        .as_ref()
                        .map(|p| p.display().to_string()),
                }
            }
        }
    }

    fn unavailable() -> Self {
        RecoveryStatusDto::Unavailable
    }
}

/// Returns the outcome of this session's `studio.redb` open, for a
/// frontend that mounts after the `studio-recovery` event already fired
/// (the event is emitted once, synchronously during `setup()`, before any
/// window is guaranteed to be listening yet).
#[tauri::command]
pub fn get_studio_recovery_status(app: tauri::AppHandle) -> Option<RecoveryStatusDto> {
    app.try_state::<RecoveryStatusDto>()
        .map(|s| s.inner().clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_init_studio_storage_with_paths_fresh_and_existing() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("studio.redb");
        let backups_dir = temp.path().join("backups");
        let recovery_log_path = temp.path().join("studio-recovery.jsonl");
        let pref_file = temp.path().join("preferences.json");
        let events_file = temp.path().join("events.jsonl");

        fs::write(
            &pref_file,
            br#"{
            "onboardingVersion": 3,
            "telemetryConsent": { "analytics": true, "crash": false },
            "installationId": "b6b6a6f0-1a2b-4c3d-8e9f-0123456789ab",
            "recentProjects": ["demo"]
        }"#,
        )
        .unwrap();

        fs::write(
            &events_file,
            br#"{"schema":1,"source":"desktop","event_id":"evt-1","timestamp":"2026-08-08T10:00:00Z","session_id":"a1a1a1a1-1a2b-4c3d-8e9f-0123456789ab","installation_id":"b6b6a6f0-1a2b-4c3d-8e9f-0123456789ab","version":"0.2.0","platform":"macos","arch":"aarch64","event":"app_started","properties":{}}
"#,
        )
        .unwrap();

        let legacy_paths = LegacyStudioPaths {
            preferences_json: Some(pref_file.clone()),
            events_jsonl: Some(events_file.clone()),
        };

        // First launch
        let (db, recovery, summary) = init_studio_storage_with_paths(
            &db_path,
            &backups_dir,
            &recovery_log_path,
            &legacy_paths,
        )
        .unwrap();
        assert_eq!(recovery, RecoveryOutcome::Healthy);
        assert_eq!(summary.preferences.unwrap().imported, 1);
        assert_eq!(summary.telemetry.unwrap().imported, 1);
        assert_eq!(db.preferences().get().unwrap().onboarding_version, Some(3));
        drop(db);

        // Second launch is idempotent
        let (db2, recovery2, summary2) = init_studio_storage_with_paths(
            &db_path,
            &backups_dir,
            &recovery_log_path,
            &legacy_paths,
        )
        .unwrap();
        assert_eq!(recovery2, RecoveryOutcome::Healthy);
        assert!(summary2.preferences.unwrap().already_migrated);
        assert!(summary2.telemetry.unwrap().already_migrated);
        drop(db2);
    }

    #[test]
    fn test_init_studio_storage_with_paths_recovers_from_corruption() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("studio.redb");
        let backups_dir = temp.path().join("backups");
        let recovery_log_path = temp.path().join("studio-recovery.jsonl");

        // A healthy database exists, then gets corrupted — exactly the
        // scenario a real, unclean shutdown could produce.
        {
            let db = StudioDatabase::open(&db_path).unwrap();
            db.preferences()
                .update(|p| p.theme = Some("dark".to_string()))
                .unwrap();
        }
        fs::write(&db_path, b"corrupt bytes, not a redb file").unwrap();

        let legacy_paths = LegacyStudioPaths {
            preferences_json: None,
            events_jsonl: None,
        };
        let (db, recovery, _summary) = init_studio_storage_with_paths(
            &db_path,
            &backups_dir,
            &recovery_log_path,
            &legacy_paths,
        )
        .unwrap();

        // No backup existed, so this must be the fresh-database path — and
        // the app must still be able to use the database normally.
        assert!(matches!(
            recovery,
            RecoveryOutcome::FreshDatabaseCreated { .. }
        ));
        assert_eq!(db.preferences().get().unwrap().theme, None);
        assert!(
            recovery_log_path.exists(),
            "the recovery event must be logged"
        );
    }

    /// Pins the exact JSON shape `ui/src/lib/native.ts`'s
    /// `StudioRecoveryStatus` discriminated union expects: a snake_case
    /// `"kind"` tag (matching this codebase's convention for internally
    /// tagged Rust enums crossing the IPC boundary, e.g. `ProjectKind`),
    /// but camelCase field names within each variant (matching every
    /// other DTO in this crate, e.g. `StudioProjectDto`). A regression
    /// here silently breaks the frontend's recovery notice — TypeScript
    /// has no way to catch a JSON shape mismatch on its own.
    #[test]
    fn recovery_status_dto_serializes_to_the_shape_native_ts_expects() {
        let healthy = serde_json::to_value(RecoveryStatusDto::Healthy).unwrap();
        assert_eq!(healthy, serde_json::json!({ "kind": "healthy" }));

        let restored = serde_json::to_value(RecoveryStatusDto::RestoredFromBackup {
            message: "restored".to_string(),
            backup_generation: 2,
            preserved_original_path: "/tmp/studio.redb.corrupt-1".to_string(),
        })
        .unwrap();
        assert_eq!(
            restored,
            serde_json::json!({
                "kind": "restored_from_backup",
                "message": "restored",
                "backupGeneration": 2,
                "preservedOriginalPath": "/tmp/studio.redb.corrupt-1",
            })
        );

        let fresh = serde_json::to_value(RecoveryStatusDto::FreshDatabaseCreated {
            message: "fresh".to_string(),
            preserved_original_path: None,
        })
        .unwrap();
        assert_eq!(
            fresh,
            serde_json::json!({
                "kind": "fresh_database_created",
                "message": "fresh",
                "preservedOriginalPath": null,
            })
        );

        let unavailable = serde_json::to_value(RecoveryStatusDto::Unavailable).unwrap();
        assert_eq!(unavailable, serde_json::json!({ "kind": "unavailable" }));
    }
}
