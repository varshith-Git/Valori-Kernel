// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Typed preferences service and Tauri command handlers for `studio.redb`.
//!
//! # Architecture (S2b-2a)
//!
//! Next.js / React (via `native.ts`)
//!        │
//!        ▼
//! Tauri commands (`get_preference`, `set_preference`, `get_installation_id`)
//!        │
//!        ▼
//! `StudioPreferencesService`
//!        │
//!        ▼
//! `Arc<StudioDatabase>`
//!        │
//!        ▼
//! `studio.redb` (`preferences` table)
//!
//! # Invariants
//!
//! - All preference reads and writes go to `studio.redb`'s `preferences` table.
//! - Legacy `preferences.json` is **never written to, modified, renamed, or deleted**.
//! - `installation_id` is stable and persisted forever across launches.

use std::sync::Arc;
use tauri::Manager;
use tracing::debug;
use valori_domain::InstallationId;
use valori_studio_storage::{
    preferences::{StudioPreferences, TelemetryConsent},
    StudioDatabase, StudioStorageResult,
};

/// Typed service wrapping preference operations on `StudioDatabase`.
#[derive(Clone)]
pub struct StudioPreferencesService {
    db: Arc<StudioDatabase>,
}

impl StudioPreferencesService {
    pub fn new(db: Arc<StudioDatabase>) -> Self {
        Self { db }
    }

    /// Fetches the full typed preferences record from `studio.redb`.
    pub fn get_all(&self) -> StudioStorageResult<StudioPreferences> {
        self.db.preferences().get()
    }

    /// Fully replaces the stored preferences record.
    #[allow(dead_code)]
    pub fn set_all(&self, prefs: &StudioPreferences) -> StudioStorageResult<()> {
        self.db.preferences().set(prefs)
    }

    /// Fetches a specific preference field by key as a JSON value.
    pub fn get_field(&self, key: &str) -> StudioStorageResult<Option<serde_json::Value>> {
        let prefs = self.db.preferences().get()?;
        let val = match key {
            "theme" => prefs.theme.map(serde_json::Value::from),
            "language" => prefs.language.map(serde_json::Value::from),
            "accentColor" | "accent_color" => prefs.accent_color.map(serde_json::Value::from),
            "onboardingVersion" | "onboarding_version" => {
                prefs.onboarding_version.map(serde_json::Value::from)
            }
            "telemetryConsent" | "telemetry_consent" => prefs
                .telemetry_consent
                .map(|c| serde_json::to_value(c).unwrap_or(serde_json::Value::Null)),
            "windowState" | "window_state" => prefs
                .window_state
                .map(|w| serde_json::to_value(w).unwrap_or(serde_json::Value::Null)),
            "lastPage" | "last_page" => prefs.last_page.map(serde_json::Value::from),
            "installationId" | "installation_id" => prefs
                .installation_id
                .map(|id| serde_json::Value::from(id.to_string())),
            "workspaceDir" | "workspace_dir" => prefs.workspace_dir.map(serde_json::Value::from),
            "modelDir" | "model_dir" => prefs.model_dir.map(serde_json::Value::from),
            "dockIcon" | "dock_icon" => prefs.dock_icon.map(serde_json::Value::from),
            "termsAccepted" | "terms_accepted" => prefs.terms_accepted.map(serde_json::Value::from),
            "notifs" | "notificationPrefs" | "notification_prefs" => prefs.notification_prefs,
            _ => None,
        };
        Ok(val)
    }

    /// Updates a specific preference field in `studio.redb`.
    pub fn set_field(&self, key: &str, value: serde_json::Value) -> StudioStorageResult<()> {
        self.db.preferences().update(|p| match key {
            "theme" => {
                p.theme = value.as_str().map(String::from);
            }
            "language" => {
                p.language = value.as_str().map(String::from);
            }
            "accentColor" | "accent_color" => {
                p.accent_color = value.as_str().map(String::from);
            }
            "onboardingVersion" | "onboarding_version" => {
                p.onboarding_version = value.as_u64().map(|v| v as u32);
            }
            "telemetryConsent" | "telemetry_consent" => {
                p.telemetry_consent = serde_json::from_value(value).ok();
            }
            "windowState" | "window_state" => {
                p.window_state = serde_json::from_value(value).ok();
            }
            "lastPage" | "last_page" => {
                p.last_page = value.as_str().map(String::from);
            }
            "installationId" | "installation_id" => {
                p.installation_id = value
                    .as_str()
                    .and_then(|s| s.parse::<InstallationId>().ok());
            }
            "workspaceDir" | "workspace_dir" => {
                p.workspace_dir = value.as_str().map(String::from);
            }
            "modelDir" | "model_dir" => {
                p.model_dir = value.as_str().map(String::from);
            }
            "dockIcon" | "dock_icon" => {
                p.dock_icon = value.as_bool();
            }
            "termsAccepted" | "terms_accepted" => {
                p.terms_accepted = value.as_bool();
            }
            "notifs" | "notificationPrefs" | "notification_prefs" => {
                p.notification_prefs = Some(value.clone());
            }
            _ => {
                debug!("Ignoring unknown or unmodeled preference key `{key}`");
            }
        })?;
        Ok(())
    }

    /// Returns the installation id from `studio.redb`, generating and persisting
    /// a permanent UUID if one has not yet been assigned.
    ///
    /// This is the **sole canonical implementation** of installation-identity
    /// get-or-init (Studio Installation Identity phase). It must be called
    /// unconditionally during app startup (see `lib.rs`'s `setup()`),
    /// independent of telemetry consent, Cloud login, or project state.
    /// `telemetry.rs` reads the value through this same method — it does not
    /// (and must not) maintain its own get-or-init logic.
    pub fn get_or_init_installation_id(&self) -> StudioStorageResult<InstallationId> {
        let mut id = self.db.preferences().get()?.installation_id;
        if id.is_none() {
            let fresh = InstallationId::new();
            self.db.preferences().update(|p| {
                if p.installation_id.is_none() {
                    p.installation_id = Some(fresh);
                }
            })?;
            id = Some(fresh);
        }
        Ok(id.unwrap())
    }

    /// Returns current telemetry consent from `studio.redb`.
    pub fn get_telemetry_consent(&self) -> StudioStorageResult<TelemetryConsent> {
        Ok(self
            .db
            .preferences()
            .get()?
            .telemetry_consent
            .unwrap_or_default())
    }

    /// Persists updated telemetry consent in `studio.redb`.
    pub fn set_telemetry_consent(&self, consent: TelemetryConsent) -> StudioStorageResult<()> {
        self.db.preferences().update(|p| {
            p.telemetry_consent = Some(consent);
        })?;
        Ok(())
    }
}

// ── Tauri Commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_preference(
    app: tauri::AppHandle,
    key: String,
) -> Result<Option<serde_json::Value>, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase state not initialized".to_string())?;
    let service = StudioPreferencesService::new(db.inner().clone());
    service.get_field(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_preference(
    app: tauri::AppHandle,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase state not initialized".to_string())?;
    let service = StudioPreferencesService::new(db.inner().clone());
    service.set_field(&key, value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_all_preferences(app: tauri::AppHandle) -> Result<StudioPreferences, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase state not initialized".to_string())?;
    let service = StudioPreferencesService::new(db.inner().clone());
    service.get_all().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_installation_id_command(app: tauri::AppHandle) -> Result<String, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase state not initialized".to_string())?;
    let service = StudioPreferencesService::new(db.inner().clone());
    service
        .get_or_init_installation_id()
        .map(|id| id.to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_telemetry_consent_command(app: tauri::AppHandle) -> Result<TelemetryConsent, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase state not initialized".to_string())?;
    let service = StudioPreferencesService::new(db.inner().clone());
    service.get_telemetry_consent().map_err(|e| e.to_string())
}

/// For whichever category(ies) `consent` has turned off, discards every
/// event of that category already sitting in `telemetry_queue`. This is
/// the eager half of the S2c revocation invariant ("no previously queued
/// analytics event may subsequently be uploaded"); the uploader boundary
/// in `telemetry.rs`'s `drain_queue` (which re-checks each event's
/// category consent immediately before sending) is the half that makes it
/// safe even if this discard were somehow skipped or raced. See
/// `telemetry.rs`'s module doc, "Consent boundary", for the full
/// guarantee.
///
/// A plain function taking `&StudioDatabase`, not a method on
/// `StudioPreferencesService` — so the service itself keeps only ever
/// touching the `preferences` table (the boundary S2b-2d.1 established:
/// "`StudioPreferencesService` never reaches into `TelemetryQueue`'s
/// table"). Orchestrating two typed stores together is the command
/// layer's job (`set_telemetry_consent_command`, below), which already
/// holds `Arc<StudioDatabase>` — not either store reaching into the
/// other's. Also directly testable without a `tauri::AppHandle`, matching
/// this crate's existing test convention (every other service/command
/// pair in this file is tested the same way).
///
/// Idempotent: discarding an already-empty (or already-clean) category is
/// a no-op, so calling this repeatedly with the same consent value is
/// safe.
fn discard_revoked_telemetry_categories(db: &StudioDatabase, consent: &TelemetryConsent) {
    use valori_studio_storage::telemetry::TelemetryCategory;
    if !consent.analytics {
        let _ = db
            .telemetry()
            .discard_category(TelemetryCategory::Analytics);
    }
    if !consent.crash {
        let _ = db.telemetry().discard_category(TelemetryCategory::Crash);
    }
}

#[tauri::command]
pub fn set_telemetry_consent_command(
    app: tauri::AppHandle,
    consent: TelemetryConsent,
) -> Result<(), String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase state not initialized".to_string())?;
    let service = StudioPreferencesService::new(db.inner().clone());
    service
        .set_telemetry_consent(consent.clone())
        .map_err(|e| e.to_string())?;
    discard_revoked_telemetry_categories(&db, &consent);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// A recognizable fake secret — never a real credential shape, per the
    /// Studio S3 task's own instruction.
    const FAKE_SECRET: &str = "valori-test-secret-123456";

    /// §16 of the Studio S3 phase: `studio.redb`'s generic preference
    /// bridge must structurally reject every secret-shaped key — proves
    /// the runtime behavior behind
    /// `credential_security_architecture.rs`'s companion source-level test
    /// (`preferences_service_source_has_no_secret_shaped_match_arm`).
    #[test]
    fn generic_preference_bridge_rejects_every_secret_shaped_key() {
        let temp = tempdir().unwrap();
        let db = Arc::new(StudioDatabase::open(&temp.path().join("studio.redb")).unwrap());
        let service = StudioPreferencesService::new(db);

        for key in [
            "apiKey",
            "api_key",
            "llmApiKey",
            "embeddingApiKey",
            "rerankerApiKey",
            "secret",
            "token",
            "password",
            "authorization",
            "credential",
        ] {
            service
                .set_field(key, serde_json::json!(FAKE_SECRET))
                .expect("set_field itself must not error even for an unrecognized key");
            assert_eq!(
                service.get_field(key).unwrap(),
                None,
                "set_field(\"{key}\", ...) must be a silent no-op — no secret-shaped key may be storable"
            );
        }

        // Whole-record serialization: the fake secret must not appear
        // anywhere in the persisted preferences record, under any field.
        let all = service.get_all().unwrap();
        let serialized = serde_json::to_string(&all).unwrap();
        assert!(
            !serialized.contains(FAKE_SECRET),
            "StudioPreferences must never serialize a secret value"
        );
    }

    #[test]
    fn test_preferences_service_crud_and_idempotency() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("studio.redb");
        let db = Arc::new(StudioDatabase::open(&db_path).unwrap());
        let service = StudioPreferencesService::new(db.clone());

        // 1. Initial state is default
        let initial = service.get_all().unwrap();
        assert_eq!(initial.theme, None);
        assert_eq!(initial.onboarding_version, None);

        // 2. Set theme and onboarding version
        service
            .set_field("theme", serde_json::json!("dark"))
            .unwrap();
        service
            .set_field("onboardingVersion", serde_json::json!(3))
            .unwrap();

        assert_eq!(
            service.get_field("theme").unwrap(),
            Some(serde_json::json!("dark"))
        );
        assert_eq!(
            service.get_field("onboardingVersion").unwrap(),
            Some(serde_json::json!(3))
        );

        // 3. Set telemetry consent
        let consent = TelemetryConsent {
            analytics: true,
            crash: false,
        };
        service.set_telemetry_consent(consent.clone()).unwrap();
        assert_eq!(service.get_telemetry_consent().unwrap(), consent);

        // 4. Installation ID lazy initialization and stability
        let id1 = service.get_or_init_installation_id().unwrap();
        let id2 = service.get_or_init_installation_id().unwrap();
        assert_eq!(id1, id2);

        // 5. Reopen database and verify values persist
        drop(service);
        drop(db);

        let db_reopened = Arc::new(StudioDatabase::open(&db_path).unwrap());
        let service_reopened = StudioPreferencesService::new(db_reopened);

        assert_eq!(
            service_reopened.get_field("theme").unwrap(),
            Some(serde_json::json!("dark"))
        );
        assert_eq!(
            service_reopened.get_field("onboardingVersion").unwrap(),
            Some(serde_json::json!(3))
        );
        assert_eq!(
            service_reopened.get_telemetry_consent().unwrap().analytics,
            true
        );
        assert_eq!(service_reopened.get_or_init_installation_id().unwrap(), id1);
    }

    /// Regression test: `Welcome.tsx` writes `workspaceDir`/`modelDir`/
    /// `dockIcon`/`termsAccepted` via `setPreference` at the end of
    /// onboarding, and `AppShellGate.tsx`/`Sidebar.tsx`/`DaemonBanner.tsx`/
    /// `SettingsModal.tsx`/`settings/page.tsx` read `workspaceDir` back on
    /// every subsequent launch. These four keys previously had no arm in
    /// `get_field`/`set_field`'s match, so `set_field` silently no-opped
    /// and `get_field` always returned `None` — onboarding appeared to
    /// complete but the chosen workspace folder was never actually
    /// persisted anywhere. This test fails loudly if that regresses.
    #[test]
    fn test_onboarding_fields_are_not_silently_dropped() {
        let temp = tempdir().unwrap();
        let db = Arc::new(StudioDatabase::open(&temp.path().join("studio.redb")).unwrap());
        let service = StudioPreferencesService::new(db);

        for (key, value) in [
            ("workspaceDir", serde_json::json!("/Users/demo/valori")),
            ("modelDir", serde_json::json!("/Users/demo/valori/models")),
            ("dockIcon", serde_json::json!(true)),
            ("termsAccepted", serde_json::json!(true)),
        ] {
            service.set_field(key, value.clone()).unwrap();
            assert_eq!(
                service.get_field(key).unwrap(),
                Some(value),
                "preference `{key}` must round-trip, not be silently dropped"
            );
        }

        // The typed record itself carries the values, not just the
        // key-indirected accessors — proves this isn't a coincidental
        // pass through some other path.
        let all = service.get_all().unwrap();
        assert_eq!(all.workspace_dir, Some("/Users/demo/valori".to_string()));
        assert_eq!(all.model_dir, Some("/Users/demo/valori/models".to_string()));
        assert_eq!(all.dock_icon, Some(true));
        assert_eq!(all.terms_accepted, Some(true));
    }

    /// S7 — `notifs` migrated off `localStorage["valori:notifs"]` (desktop
    /// only) onto this same generic key/value bridge. Both spellings the
    /// UI might use must round-trip.
    #[test]
    fn notification_prefs_round_trip_through_every_key_spelling() {
        let temp = tempdir().unwrap();
        let db = Arc::new(StudioDatabase::open(&temp.path().join("studio.redb")).unwrap());
        let service = StudioPreferencesService::new(db);

        let value = serde_json::json!({"desktop": true, "sound": false});
        service.set_field("notifs", value.clone()).unwrap();
        assert_eq!(service.get_field("notifs").unwrap(), Some(value.clone()));
        assert_eq!(
            service.get_field("notificationPrefs").unwrap(),
            Some(value.clone())
        );
        assert_eq!(
            service.get_field("notification_prefs").unwrap(),
            Some(value)
        );
    }

    // ── Consent revocation orchestration (S2c) ───────────────────────────

    use valori_studio_storage::telemetry::{StudioTelemetryEvent, TelemetryCategory};

    /// `discard_revoked_telemetry_categories` is exactly what
    /// `set_telemetry_consent_command` calls after persisting consent —
    /// tested directly here (no `tauri::AppHandle` needed) against real
    /// `studio.redb` storage.
    #[test]
    fn discard_revoked_telemetry_categories_removes_only_the_revoked_category() {
        let temp = tempdir().unwrap();
        let db = Arc::new(StudioDatabase::open(&temp.path().join("studio.redb")).unwrap());

        db.telemetry()
            .enqueue(&StudioTelemetryEvent::new(
                "page_view",
                None,
                serde_json::json!({}),
                1000,
                TelemetryCategory::Analytics,
            ))
            .unwrap();
        db.telemetry()
            .enqueue(&StudioTelemetryEvent::new(
                "studio_crashed",
                None,
                serde_json::json!({}),
                1100,
                TelemetryCategory::Crash,
            ))
            .unwrap();
        assert_eq!(db.telemetry().count().unwrap(), 2);

        // analytics off, crash still on.
        discard_revoked_telemetry_categories(
            &db,
            &TelemetryConsent {
                analytics: false,
                crash: true,
            },
        );

        let remaining = db.telemetry().peek_batch(10).unwrap();
        assert_eq!(
            remaining.len(),
            1,
            "only the analytics event must be discarded"
        );
        assert_eq!(remaining[0].category, TelemetryCategory::Crash);
    }

    /// Both categories revoked at once discards both.
    #[test]
    fn discard_revoked_telemetry_categories_handles_both_categories_off() {
        let temp = tempdir().unwrap();
        let db = Arc::new(StudioDatabase::open(&temp.path().join("studio.redb")).unwrap());

        db.telemetry()
            .enqueue(&StudioTelemetryEvent::new(
                "a",
                None,
                serde_json::json!({}),
                1000,
                TelemetryCategory::Analytics,
            ))
            .unwrap();
        db.telemetry()
            .enqueue(&StudioTelemetryEvent::new(
                "c",
                None,
                serde_json::json!({}),
                1100,
                TelemetryCategory::Crash,
            ))
            .unwrap();

        discard_revoked_telemetry_categories(
            &db,
            &TelemetryConsent {
                analytics: false,
                crash: false,
            },
        );
        assert_eq!(db.telemetry().count().unwrap(), 0);
    }

    /// Calling it repeatedly with the same (off) consent is a safe no-op —
    /// no error, no re-discovery of already-gone rows.
    #[test]
    fn discard_revoked_telemetry_categories_is_idempotent() {
        let temp = tempdir().unwrap();
        let db = Arc::new(StudioDatabase::open(&temp.path().join("studio.redb")).unwrap());
        db.telemetry()
            .enqueue(&StudioTelemetryEvent::new(
                "a",
                None,
                serde_json::json!({}),
                1000,
                TelemetryCategory::Analytics,
            ))
            .unwrap();

        let consent = TelemetryConsent {
            analytics: false,
            crash: false,
        };
        discard_revoked_telemetry_categories(&db, &consent);
        discard_revoked_telemetry_categories(&db, &consent);
        discard_revoked_telemetry_categories(&db, &consent);
        assert_eq!(db.telemetry().count().unwrap(), 0);
    }

    /// When both categories are still on, nothing is discarded — the
    /// function must not be a blanket "clear the queue on every consent
    /// write" hammer.
    #[test]
    fn discard_revoked_telemetry_categories_does_nothing_when_both_consents_remain_on() {
        let temp = tempdir().unwrap();
        let db = Arc::new(StudioDatabase::open(&temp.path().join("studio.redb")).unwrap());
        db.telemetry()
            .enqueue(&StudioTelemetryEvent::new(
                "a",
                None,
                serde_json::json!({}),
                1000,
                TelemetryCategory::Analytics,
            ))
            .unwrap();

        discard_revoked_telemetry_categories(
            &db,
            &TelemetryConsent {
                analytics: true,
                crash: true,
            },
        );
        assert_eq!(
            db.telemetry().count().unwrap(),
            1,
            "consent staying on must not discard anything"
        );
    }

    // ── Installation identity lifecycle (Studio Installation Identity phase) ─
    //
    // These pin the invariant documented on `get_or_init_installation_id`:
    // installation_id must exist independent of telemetry consent, must be
    // stable across restarts (fresh `StudioDatabase::open` calls stand in
    // for a process restart), and must never be regenerated once set.

    /// Fresh `studio.redb`, telemetry never touched: the id must still be
    /// generated the moment `get_or_init_installation_id` is called — this
    /// is the call `lib.rs`'s `setup()` makes unconditionally at startup.
    #[test]
    fn fresh_database_gets_an_installation_id_with_no_telemetry_consent_set() {
        let temp = tempdir().unwrap();
        let db = Arc::new(StudioDatabase::open(&temp.path().join("studio.redb")).unwrap());
        let service = StudioPreferencesService::new(db.clone());

        // No telemetry_consent record exists at all — the default fail-closed
        // {false, false} state a real fresh install starts in.
        assert_eq!(
            service.get_telemetry_consent().unwrap(),
            TelemetryConsent::default()
        );

        let id = service.get_or_init_installation_id().unwrap();
        let stored = service.get_all().unwrap().installation_id;
        assert_eq!(stored, Some(id), "the generated id must be persisted");
    }

    /// The critical regression test: analytics AND crash both explicitly
    /// false must not prevent installation_id from existing. This is the
    /// exact live-database state the audit found broken.
    #[test]
    fn installation_id_exists_even_when_telemetry_is_fully_disabled() {
        let temp = tempdir().unwrap();
        let db = Arc::new(StudioDatabase::open(&temp.path().join("studio.redb")).unwrap());
        let service = StudioPreferencesService::new(db.clone());

        service
            .set_telemetry_consent(TelemetryConsent {
                analytics: false,
                crash: false,
            })
            .unwrap();

        let id = service.get_or_init_installation_id().unwrap();
        assert_eq!(
            service.get_all().unwrap().installation_id,
            Some(id),
            "installation_id must be generated regardless of telemetry consent"
        );
    }

    /// Telemetry enabled: same guarantee holds (not just the off case).
    #[test]
    fn installation_id_exists_when_telemetry_is_enabled() {
        let temp = tempdir().unwrap();
        let db = Arc::new(StudioDatabase::open(&temp.path().join("studio.redb")).unwrap());
        let service = StudioPreferencesService::new(db.clone());

        service
            .set_telemetry_consent(TelemetryConsent {
                analytics: true,
                crash: true,
            })
            .unwrap();

        let id = service.get_or_init_installation_id().unwrap();
        assert_eq!(service.get_all().unwrap().installation_id, Some(id));
    }

    /// Stable across a simulated restart: two separate `StudioDatabase::open`
    /// calls against the same file must observe the identical id.
    #[test]
    fn installation_id_is_stable_across_database_reopen() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("studio.redb");

        let id_first = {
            let db = Arc::new(StudioDatabase::open(&path).unwrap());
            let service = StudioPreferencesService::new(db);
            service.get_or_init_installation_id().unwrap()
        };

        let id_second = {
            let db = Arc::new(StudioDatabase::open(&path).unwrap());
            let service = StudioPreferencesService::new(db);
            service.get_or_init_installation_id().unwrap()
        };

        assert_eq!(id_first, id_second, "restart must reuse the same id");
    }

    /// Case A of the audit: a valid installation_id already exists — the
    /// service must preserve it exactly, never regenerate.
    #[test]
    fn existing_installation_id_is_preserved_exactly() {
        let temp = tempdir().unwrap();
        let db = Arc::new(StudioDatabase::open(&temp.path().join("studio.redb")).unwrap());
        let existing = InstallationId::new();
        db.preferences()
            .update(|p| p.installation_id = Some(existing))
            .unwrap();

        let service = StudioPreferencesService::new(db);
        let id = service.get_or_init_installation_id().unwrap();
        assert_eq!(
            id, existing,
            "must reuse the pre-existing id, not replace it"
        );
    }

    /// Session linkage: whatever `get_or_init_installation_id` returns is
    /// exactly what a new session must be started with.
    #[test]
    fn get_or_init_installation_id_value_matches_what_a_new_session_would_receive() {
        let temp = tempdir().unwrap();
        let db = Arc::new(StudioDatabase::open(&temp.path().join("studio.redb")).unwrap());
        let service = StudioPreferencesService::new(db.clone());

        let id = service.get_or_init_installation_id().unwrap();

        let session_service = crate::session_service::SessionService::new(db.clone());
        let session_id = valori_domain::SessionId::new();
        let started = session_service
            .start_session(session_id, Some(id), "0.0.0-test", "test", 1000)
            .unwrap();

        assert_eq!(
            started.installation_id,
            Some(id.to_string()),
            "session.installation_id must equal preferences.installation_id"
        );
    }
}
