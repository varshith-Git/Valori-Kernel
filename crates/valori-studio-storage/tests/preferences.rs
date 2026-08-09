// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use valori_domain::InstallationId;
use valori_studio_storage::preferences::{StudioPreferences, TelemetryConsent, WindowState};
use valori_studio_storage::StudioDatabase;

fn open_tmp() -> (tempfile::TempDir, StudioDatabase) {
    let dir = tempfile::tempdir().unwrap();
    let db = StudioDatabase::open(&dir.path().join("studio.redb")).unwrap();
    (dir, db)
}

#[test]
fn get_on_fresh_database_returns_defaults() {
    let (_dir, db) = open_tmp();
    assert_eq!(
        db.preferences().get().unwrap(),
        StudioPreferences::default()
    );
}

#[test]
fn set_then_get_round_trips() {
    let (_dir, db) = open_tmp();
    let prefs = StudioPreferences {
        theme: Some("dark".to_string()),
        language: Some("en-US".to_string()),
        accent_color: Some("indigo".to_string()),
        onboarding_version: Some(3),
        telemetry_consent: Some(TelemetryConsent {
            analytics: true,
            crash: false,
        }),
        window_state: Some(WindowState {
            width: 1280.0,
            height: 800.0,
            x: Some(10.0),
            y: Some(20.0),
            maximized: false,
        }),
        last_page: Some("/projects/demo".to_string()),
        installation_id: Some(InstallationId::new()),
        workspace_dir: Some("/Users/demo/valori-workspace".to_string()),
        model_dir: Some("/Users/demo/valori-models".to_string()),
        dock_icon: Some(true),
        terms_accepted: Some(true),
        notification_prefs: Some(serde_json::json!({"desktop": true})),
    };
    db.preferences().set(&prefs).unwrap();
    assert_eq!(db.preferences().get().unwrap(), prefs);
}

#[test]
fn update_is_read_modify_write() {
    let (_dir, db) = open_tmp();
    db.preferences()
        .update(|p| p.theme = Some("light".to_string()))
        .unwrap();
    db.preferences()
        .update(|p| p.language = Some("fr".to_string()))
        .unwrap();

    let prefs = db.preferences().get().unwrap();
    assert_eq!(prefs.theme, Some("light".to_string()));
    assert_eq!(prefs.language, Some("fr".to_string()));
}

#[test]
fn delete_reverts_to_defaults() {
    let (_dir, db) = open_tmp();
    db.preferences()
        .update(|p| p.theme = Some("dark".to_string()))
        .unwrap();
    assert!(db.preferences().delete().unwrap());
    assert_eq!(
        db.preferences().get().unwrap(),
        StudioPreferences::default()
    );
    // Deleting an already-absent record is a no-op, not an error.
    assert!(!db.preferences().delete().unwrap());
}

#[test]
fn reopen_preserves_preferences() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.redb");
    {
        let db = StudioDatabase::open(&path).unwrap();
        db.preferences()
            .update(|p| p.onboarding_version = Some(3))
            .unwrap();
    }
    {
        let db = StudioDatabase::open(&path).unwrap();
        assert_eq!(db.preferences().get().unwrap().onboarding_version, Some(3));
    }
}

/// A record serialized without a field this build added (simulated by
/// hand-writing JSON missing `accent_color`/`window_state`) must still
/// deserialize — the forward-compatibility property `#[serde(default)]`
/// exists to guarantee.
#[test]
fn old_shaped_json_without_newer_fields_still_deserializes() {
    let old_json = br#"{"theme":"dark","language":"en"}"#;
    let prefs: StudioPreferences = serde_json::from_slice(old_json).unwrap();
    assert_eq!(prefs.theme, Some("dark".to_string()));
    assert_eq!(prefs.language, Some("en".to_string()));
    assert_eq!(prefs.accent_color, None);
    assert_eq!(prefs.window_state, None);
    assert_eq!(prefs.workspace_dir, None);
    assert_eq!(prefs.dock_icon, None);
}

/// `workspaceDir`/`modelDir`/`dockIcon`/`termsAccepted` — the fields
/// `ui/src/lib/native.ts`'s onboarding flow writes and several components
/// read back (`AppShellGate.tsx`, `Sidebar.tsx`, `DaemonBanner.tsx`,
/// `SettingsModal.tsx`). Round-tripped explicitly so a regression here
/// (e.g. a field silently dropped again) fails loudly instead of as a
/// runtime no-op discovered by a user losing their workspace folder.
#[test]
fn onboarding_fields_round_trip() {
    let (_dir, db) = open_tmp();
    db.preferences()
        .update(|p| {
            p.workspace_dir = Some("/Users/demo/valori".to_string());
            p.model_dir = Some("/Users/demo/valori/models".to_string());
            p.dock_icon = Some(false);
            p.terms_accepted = Some(true);
        })
        .unwrap();

    let prefs = db.preferences().get().unwrap();
    assert_eq!(prefs.workspace_dir, Some("/Users/demo/valori".to_string()));
    assert_eq!(
        prefs.model_dir,
        Some("/Users/demo/valori/models".to_string())
    );
    assert_eq!(prefs.dock_icon, Some(false));
    assert_eq!(prefs.terms_accepted, Some(true));
}
