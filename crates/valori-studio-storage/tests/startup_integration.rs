// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! S2b-1: Startup migration integration tests.
//!
//! Verifies the real startup lifecycle against realistic temporary directory fixtures:
//! - Fresh installation: creates studio.redb, succeeds without legacy files.
//! - Existing installation: migrates preferences.json and events.jsonl, leaves legacy files untouched.
//! - Second startup: detects completed migration, performs no duplicate import.
//! - Migration failure: leaves legacy files and database intact and recoverable.
//! - Unrelated data: preserves existing metadata.redb, projects/ directories, and other files.

use std::fs;
use std::path::{Path, PathBuf};
use valori_domain::SessionId;
use valori_studio_storage::{
    db::{LegacyMigrationSummary, LegacyStudioPaths},
    path::{default_db_path, default_home_dir, STUDIO_DB_FILENAME},
    StudioDatabase, CURRENT_SCHEMA_VERSION,
};

const SAMPLE_PREFERENCES_JSON: &[u8] = br#"{
  "onboardingVersion": 3,
  "telemetryConsent": { "analytics": true, "crash": false },
  "installationId": "b6b6a6f0-1a2b-4c3d-8e9f-0123456789ab",
  "recentProjects": ["demo-project", "finance-rag"],
  "favoriteProjects": ["demo-project"],
  "lastOpenedProject": "demo-project",
  "lastPage": "/projects/demo-project"
}"#;

const SAMPLE_EVENTS_JSONL: &[u8] = br#"{"schema":1,"source":"desktop","event_id":"evt-101","timestamp":"2026-08-08T10:00:00Z","session_id":"a1a1a1a1-1a2b-4c3d-8e9f-0123456789ab","installation_id":"b6b6a6f0-1a2b-4c3d-8e9f-0123456789ab","version":"0.2.0","platform":"macos","arch":"aarch64","event":"app_started","properties":{"mode":"standalone"}}
{"schema":1,"source":"desktop","event_id":"evt-102","timestamp":"2026-08-08T10:05:00Z","session_id":"a1a1a1a1-1a2b-4c3d-8e9f-0123456789ab","installation_id":"b6b6a6f0-1a2b-4c3d-8e9f-0123456789ab","version":"0.2.0","platform":"macos","arch":"aarch64","event":"project_opened","properties":{"project":"demo-project"}}
"#;

/// Simulates the real startup migration sequence given resolved paths.
fn simulate_startup(
    db_path: &Path,
    legacy_paths: &LegacyStudioPaths,
    now: i64,
) -> (StudioDatabase, LegacyMigrationSummary) {
    let db = StudioDatabase::open(db_path).expect("studio.redb open failed");
    let summary = db.run_legacy_migration(legacy_paths, now);
    (db, summary)
}

#[test]
fn fresh_installation_creates_db_and_succeeds_without_legacy_files() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("valori-home").join("studio.redb");
    let legacy_paths = LegacyStudioPaths {
        preferences_json: Some(temp.path().join("config").join("preferences.json")),
        events_jsonl: Some(temp.path().join("config").join("events.jsonl")),
    };

    assert!(!db_path.exists());
    let now = 1_723_110_000_000;
    let (db, summary) = simulate_startup(&db_path, &legacy_paths, now);

    // studio.redb is created with the current schema version
    assert!(db_path.exists());
    assert_eq!(db.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);

    // Both legacy sources report not found, not an error
    let pref_report = summary.preferences.unwrap();
    assert!(!pref_report.source_found);
    assert!(!pref_report.already_migrated);
    assert_eq!(pref_report.imported, 0);

    let telem_report = summary.telemetry.unwrap();
    assert!(!telem_report.source_found);
    assert!(!telem_report.already_migrated);
    assert_eq!(telem_report.imported, 0);

    // Normal DB operations on a fresh install return valid default state
    let prefs = db.preferences().get().unwrap();
    assert_eq!(prefs.onboarding_version, None);
    assert_eq!(db.telemetry().count().unwrap(), 0);
}

#[test]
fn existing_installation_migrates_legacy_files_and_preserves_originals() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join(".valori").join("studio.redb");
    let config_dir = temp.path().join("app_config");
    fs::create_dir_all(&config_dir).unwrap();

    let pref_file = config_dir.join("preferences.json");
    let events_file = config_dir.join("events.jsonl");

    fs::write(&pref_file, SAMPLE_PREFERENCES_JSON).unwrap();
    fs::write(&events_file, SAMPLE_EVENTS_JSONL).unwrap();

    let pref_bytes_before = fs::read(&pref_file).unwrap();
    let events_bytes_before = fs::read(&events_file).unwrap();

    let legacy_paths = LegacyStudioPaths {
        preferences_json: Some(pref_file.clone()),
        events_jsonl: Some(events_file.clone()),
    };

    let now = 1_723_110_000_000;
    let (db, summary) = simulate_startup(&db_path, &legacy_paths, now);

    // 1. studio.redb created and opened
    assert!(db_path.exists());
    assert_eq!(db.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);

    // 2. Migration succeeds
    let pref_report = summary.preferences.unwrap();
    assert!(pref_report.source_found);
    assert!(!pref_report.already_migrated);
    assert_eq!(pref_report.imported, 1);
    assert!(pref_report.skipped.is_empty());

    let telem_report = summary.telemetry.unwrap();
    assert!(telem_report.source_found);
    assert!(!telem_report.already_migrated);
    assert_eq!(telem_report.imported, 2);
    assert!(telem_report.skipped.is_empty());

    // 3. Database contains the imported state
    let prefs = db.preferences().get().unwrap();
    assert_eq!(prefs.onboarding_version, Some(3));
    assert_eq!(prefs.telemetry_consent.unwrap().analytics, true);
    assert_eq!(prefs.last_page, Some("/projects/demo-project".to_string()));
    assert_eq!(
        prefs.installation_id.unwrap().to_string(),
        "b6b6a6f0-1a2b-4c3d-8e9f-0123456789ab"
    );

    let legacy_names = db.legacy_project_names().unwrap().unwrap();
    assert_eq!(legacy_names.recent, vec!["demo-project", "finance-rag"]);
    assert_eq!(legacy_names.favorite, vec!["demo-project"]);
    assert_eq!(legacy_names.last_opened, Some("demo-project".to_string()));

    assert_eq!(db.telemetry().count().unwrap(), 2);
    let events = db.telemetry().peek_batch(10).unwrap();
    assert_eq!(events[0].event_id, "evt-101");
    assert_eq!(events[1].event_id, "evt-102");

    // 4. CRITICAL INVARIANT: Legacy files are NEVER modified, renamed, or deleted
    let pref_bytes_after = fs::read(&pref_file).unwrap();
    let events_bytes_after = fs::read(&events_file).unwrap();
    assert_eq!(pref_bytes_before, pref_bytes_after);
    assert_eq!(events_bytes_before, events_bytes_after);
}

#[test]
fn second_startup_is_idempotent_and_performs_no_duplicate_import() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("studio.redb");
    let pref_file = temp.path().join("preferences.json");
    let events_file = temp.path().join("events.jsonl");

    fs::write(&pref_file, SAMPLE_PREFERENCES_JSON).unwrap();
    fs::write(&events_file, SAMPLE_EVENTS_JSONL).unwrap();

    let legacy_paths = LegacyStudioPaths {
        preferences_json: Some(pref_file.clone()),
        events_jsonl: Some(events_file.clone()),
    };

    // ── First startup ──
    let now1 = 1_723_110_000_000;
    let (db1, summary1) = simulate_startup(&db_path, &legacy_paths, now1);
    assert_eq!(summary1.preferences.unwrap().imported, 1);
    assert_eq!(summary1.telemetry.unwrap().imported, 2);
    drop(db1);

    // ── Second startup ──
    let now2 = 1_723_110_060_000;
    let (db2, summary2) = simulate_startup(&db_path, &legacy_paths, now2);

    let pref_report2 = summary2.preferences.unwrap();
    assert!(pref_report2.already_migrated);
    assert!(pref_report2.source_found);
    assert_eq!(pref_report2.imported, 0);

    let telem_report2 = summary2.telemetry.unwrap();
    assert!(telem_report2.already_migrated);
    assert!(telem_report2.source_found);
    assert_eq!(telem_report2.imported, 0);

    // Queue count is still exactly 2 (no duplicate telemetry events appended)
    assert_eq!(db2.telemetry().count().unwrap(), 2);
    drop(db2);

    // ── Third startup ──
    let now3 = 1_723_110_120_000;
    let (db3, summary3) = simulate_startup(&db_path, &legacy_paths, now3);
    assert!(summary3.preferences.unwrap().already_migrated);
    assert!(summary3.telemetry.unwrap().already_migrated);
    assert_eq!(db3.telemetry().count().unwrap(), 2);
    assert_eq!(db3.telemetry().count().unwrap(), 2);
}

#[test]
fn migration_failure_on_corrupt_legacy_file_preserves_file_and_leaves_db_recoverable() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("studio.redb");
    let corrupt_pref_file = temp.path().join("preferences.json");
    let valid_events_file = temp.path().join("events.jsonl");

    let invalid_json = br#"{ "onboardingVersion": 3, "telemetryConsent": INVALID_JSON_SYNTAX"#;
    fs::write(&corrupt_pref_file, invalid_json).unwrap();
    fs::write(&valid_events_file, SAMPLE_EVENTS_JSONL).unwrap();

    let legacy_paths = LegacyStudioPaths {
        preferences_json: Some(corrupt_pref_file.clone()),
        events_jsonl: Some(valid_events_file.clone()),
    };

    // First startup with malformed preferences.json
    let now1 = 1_723_110_000_000;
    let (db1, summary1) = simulate_startup(&db_path, &legacy_paths, now1);

    // Preferences migration fails with a serde error
    assert!(summary1.preferences.is_err());
    // Telemetry migration succeeds independently
    assert_eq!(summary1.telemetry.unwrap().imported, 2);

    // The corrupt preferences file on disk is untouched
    assert_eq!(fs::read(&corrupt_pref_file).unwrap(), invalid_json);

    // The database remains healthy and unmigrated for preferences
    let prefs_after_failure = db1.preferences().get().unwrap();
    assert_eq!(prefs_after_failure.onboarding_version, None);
    drop(db1);

    // User or fix updates preferences.json to valid content
    fs::write(&corrupt_pref_file, SAMPLE_PREFERENCES_JSON).unwrap();

    // Next launch: preferences migration runs and succeeds
    let now2 = 1_723_110_060_000;
    let (db2, summary2) = simulate_startup(&db_path, &legacy_paths, now2);
    let pref_report2 = summary2.preferences.unwrap();
    assert!(!pref_report2.already_migrated);
    assert_eq!(pref_report2.imported, 1);

    // Telemetry was already migrated on the first run, so it skips
    assert!(summary2.telemetry.unwrap().already_migrated);

    let recovered_prefs = db2.preferences().get().unwrap();
    assert_eq!(recovered_prefs.onboarding_version, Some(3));
}

#[test]
fn migration_preserves_existing_unrelated_files_and_projects() {
    let temp = tempfile::tempdir().unwrap();
    let valori_home = temp.path().join(".valori");
    fs::create_dir_all(&valori_home).unwrap();

    // Existing metadata.redb (daemon control plane)
    let metadata_file = valori_home.join("metadata.redb");
    fs::write(&metadata_file, b"MOCK_METADATA_REDB_CONTENT_UNCHANGED").unwrap();

    // Existing project data directory
    let project_dir = valori_home.join("projects").join("my-project");
    fs::create_dir_all(&project_dir).unwrap();
    let manifest_file = project_dir.join("project.json");
    fs::write(&manifest_file, b"{\"name\":\"my-project\",\"dim\":128}").unwrap();

    let db_path = valori_home.join(STUDIO_DB_FILENAME);
    let pref_file = temp.path().join("preferences.json");
    fs::write(&pref_file, SAMPLE_PREFERENCES_JSON).unwrap();

    let legacy_paths = LegacyStudioPaths {
        preferences_json: Some(pref_file),
        events_jsonl: None,
    };

    let now = 1_723_110_000_000;
    let (db, summary) = simulate_startup(&db_path, &legacy_paths, now);
    assert_eq!(summary.preferences.unwrap().imported, 1);

    // Unrelated files remain byte-identical
    assert_eq!(
        fs::read(&metadata_file).unwrap(),
        b"MOCK_METADATA_REDB_CONTENT_UNCHANGED"
    );
    assert_eq!(
        fs::read(&manifest_file).unwrap(),
        b"{\"name\":\"my-project\",\"dim\":128}"
    );

    // studio.redb is cleanly created alongside them
    assert!(db_path.exists());
    assert_eq!(db.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
}

#[test]
fn preferences_runtime_flow_persists_to_studio_redb_and_leaves_legacy_file_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("valori-home").join("studio.redb");
    let pref_file = temp.path().join("config").join("preferences.json");
    fs::create_dir_all(pref_file.parent().unwrap()).unwrap();
    fs::write(&pref_file, SAMPLE_PREFERENCES_JSON).unwrap();

    let legacy_paths = LegacyStudioPaths {
        preferences_json: Some(pref_file.clone()),
        events_jsonl: None,
    };

    // 1. Initial startup runs migration
    let now = 1_723_110_000_000;
    let (db, summary) = simulate_startup(&db_path, &legacy_paths, now);
    assert_eq!(summary.preferences.unwrap().imported, 1);

    let initial = db.preferences().get().unwrap();
    assert_eq!(initial.onboarding_version, Some(3));
    assert_eq!(initial.telemetry_consent.as_ref().unwrap().analytics, true);
    let original_install_id = initial.installation_id.unwrap();

    // 2. Perform runtime modifications (Theme changed to "light", telemetry consent changed)
    db.preferences()
        .update(|p| {
            p.theme = Some("light".to_string());
            p.telemetry_consent = Some(valori_studio_storage::preferences::TelemetryConsent {
                analytics: false,
                crash: true,
            });
        })
        .unwrap();

    // 3. Verify preferences.json on disk is completely unchanged (byte-for-byte)
    assert_eq!(
        fs::read(&pref_file).unwrap(),
        SAMPLE_PREFERENCES_JSON,
        "legacy preferences.json must NEVER be modified by runtime preference writes"
    );

    // 4. Simulate application restart (drop db and reopen)
    drop(db);

    let (db2, summary2) = simulate_startup(&db_path, &legacy_paths, now + 5000);
    assert!(
        summary2.preferences.unwrap().already_migrated,
        "Second launch detects migration marker"
    );

    // 5. Verify restarted Studio reads the modified values from studio.redb
    let restarted_prefs = db2.preferences().get().unwrap();
    assert_eq!(restarted_prefs.theme, Some("light".to_string()));
    assert_eq!(
        restarted_prefs.telemetry_consent.unwrap(),
        valori_studio_storage::preferences::TelemetryConsent {
            analytics: false,
            crash: true,
        }
    );
    assert_eq!(
        restarted_prefs.installation_id.unwrap(),
        original_install_id,
        "installation_id must be stable forever across restarts"
    );

    // 6. Verify legacy file is STILL untouched
    assert_eq!(
        fs::read(&pref_file).unwrap(),
        SAMPLE_PREFERENCES_JSON,
        "legacy preferences.json must remain byte-identical after restarts"
    );
}

#[test]
fn project_registry_runtime_lifecycle_and_invariants() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("valori-home").join("studio.redb");
    let pref_file = temp.path().join("config").join("preferences.json");
    fs::create_dir_all(pref_file.parent().unwrap()).unwrap();
    fs::write(&pref_file, SAMPLE_PREFERENCES_JSON).unwrap();

    let legacy_paths = LegacyStudioPaths {
        preferences_json: Some(pref_file.clone()),
        events_jsonl: None,
    };

    let now = 1_723_110_000_000;
    let (db, summary) = simulate_startup(&db_path, &legacy_paths, now);
    assert_eq!(summary.preferences.unwrap().imported, 1);

    // 1. Reconcile legacy project names against known local projects
    let demo_id = valori_domain::ProjectId::new();
    let demo_dir = temp.path().join("projects").join("demo-project");
    fs::create_dir_all(&demo_dir).unwrap();

    let known = vec![(demo_id, "demo-project".to_string(), demo_dir.clone())];
    let legacy = db.legacy_project_names().unwrap().unwrap();
    assert_eq!(legacy.favorite, vec!["demo-project".to_string()]);
    assert_eq!(
        legacy.recent,
        vec!["demo-project".to_string(), "finance-rag".to_string()]
    );

    // Reconcile: demo-project gets real ProjectId; finance-rag remains unresolved legacy residue
    for (id, name, path) in &known {
        let is_favorite = legacy.favorite.contains(name);
        let record = db.projects().register_local(*id, name, path, now).unwrap();
        if is_favorite {
            db.projects().set_favorite(*id, true).unwrap();
        }
        if legacy.last_opened.as_deref() == Some(name.as_str()) {
            db.projects().touch_last_opened(*id, now + 100).unwrap();
        }
        assert_eq!(record.id, *id);
    }

    // 2. Verified resolved vs unresolved
    let demo_rec = db.projects().get(demo_id).unwrap().unwrap();
    assert_eq!(demo_rec.id, demo_id);
    assert_eq!(demo_rec.display_name, "demo-project");
    assert!(demo_rec.favorite, "reconciled favorite status");
    assert_eq!(demo_rec.last_opened_at, Some(now + 100));

    // Legacy "finance-rag" was never assigned a fake ID
    assert_eq!(db.projects().list().unwrap().len(), 1);

    // 3. Register a new second project
    let finance_id = valori_domain::ProjectId::new();
    let finance_dir = temp.path().join("projects").join("finance-rag");
    fs::create_dir_all(&finance_dir).unwrap();
    db.projects()
        .register_local(finance_id, "finance-rag", &finance_dir, now + 200)
        .unwrap();
    db.projects()
        .touch_last_opened(finance_id, now + 500)
        .unwrap();

    // 4. Test Recents ordering: finance (now+500) > demo (now+100)
    let recent = db.projects().recent(10).unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].id, finance_id);
    assert_eq!(recent[1].id, demo_id);

    // 5. Project rename preserves ProjectId
    let renamed = db.projects().rename(demo_id, "demo-production").unwrap();
    assert_eq!(renamed.id, demo_id, "ProjectId must not change on rename");
    assert_eq!(renamed.display_name, "demo-production");
    assert!(renamed.favorite, "favorite survives rename");

    // 6. Project move preserves ProjectId
    let moved_dir = temp.path().join("projects").join("demo-moved");
    fs::create_dir_all(&moved_dir).unwrap();
    let moved = db.projects().set_local_path(demo_id, &moved_dir).unwrap();
    assert_eq!(moved.id, demo_id, "ProjectId must not change on move");

    // 7. Missing directory preserves registry entry with path intact
    fs::remove_dir_all(&moved_dir).unwrap();
    let missing_rec = db.projects().get(demo_id).unwrap().unwrap();
    assert_eq!(missing_rec.id, demo_id);
    match &missing_rec.kind {
        valori_studio_storage::project::ProjectKind::Local { path } => {
            assert_eq!(path, &moved_dir);
            assert!(!path.exists(), "directory is missing on disk");
        }
        _ => panic!("expected Local"),
    }

    // 8. Re-open and verify persistence across restarts
    drop(db);

    let (db2, _) = simulate_startup(&db_path, &legacy_paths, now + 1000);
    let list = db2.projects().list().unwrap();
    assert_eq!(list.len(), 2);
    let persisted_demo = db2.projects().get(demo_id).unwrap().unwrap();
    assert_eq!(persisted_demo.display_name, "demo-production");
    assert!(persisted_demo.favorite);

    // 9. Legacy preferences file is STILL 100% byte-for-byte untouched
    assert_eq!(
        fs::read(&pref_file).unwrap(),
        SAMPLE_PREFERENCES_JSON,
        "legacy preferences.json must remain byte-identical throughout project lifecycle"
    );
}

#[test]
fn session_runtime_lifecycle_and_crash_reconciliation() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("valori-home").join("studio.redb");
    let pref_file = temp.path().join("config").join("preferences.json");
    fs::create_dir_all(pref_file.parent().unwrap()).unwrap();
    fs::write(&pref_file, SAMPLE_PREFERENCES_JSON).unwrap();

    let legacy_paths = LegacyStudioPaths {
        preferences_json: Some(pref_file.clone()),
        events_jsonl: None,
    };

    let now = 1_723_110_000_000;
    let (db1, _) = simulate_startup(&db_path, &legacy_paths, now);
    let install_id = db1.preferences().get().unwrap().installation_id;

    // Launch 1: Session 1 starts
    let session1 = SessionId::new();
    let s1_rec = db1
        .sessions()
        .start(session1, install_id, "0.2.0", "macos", now)
        .unwrap();
    assert_eq!(s1_rec.id, session1);
    assert_eq!(s1_rec.started_at, now);
    assert_eq!(s1_rec.ended_at, None);
    assert!(!s1_rec.crashed);
    assert!(s1_rec.is_open());

    // Dev-mode remount: calling start again with the same SessionId is idempotent
    let s1_remount = db1
        .sessions()
        .start(session1, install_id, "0.2.0", "macos", now + 500)
        .unwrap();
    assert_eq!(
        s1_remount.started_at, now,
        "started_at must not move on remount"
    );

    // Clean exit: Session 1 ends
    let s1_ended = db1.sessions().end(session1, now + 10_000, false).unwrap();
    assert_eq!(s1_ended.ended_at, Some(now + 10_000));
    assert!(!s1_ended.crashed);
    assert!(!s1_ended.is_open());

    drop(db1);

    // Launch 2: Session 2 starts, but simulates an abrupt termination / crash (no end() called)
    let (db2, _) = simulate_startup(&db_path, &legacy_paths, now + 20_000);
    let session2 = SessionId::new();
    db2.sessions()
        .start(session2, install_id, "0.2.0", "macos", now + 20_000)
        .unwrap();

    // Abrupt termination simulated: db2 is dropped without calling end()
    drop(db2);

    // Launch 3: Next startup starts Session 3 and reconciles crashed prior sessions
    let (db3, _) = simulate_startup(&db_path, &legacy_paths, now + 30_000);
    let session3 = SessionId::new();
    db3.sessions()
        .start(session3, install_id, "0.2.0", "macos", now + 30_000)
        .unwrap();

    // Startup reconciles prior open sessions
    let crashed_count = db3
        .sessions()
        .reconcile_crashed(session3, now + 30_000)
        .unwrap();
    assert_eq!(crashed_count, 1, "Session 2 must be marked crashed");

    // Verify Session 2 is marked crashed with ended_at recorded
    let s2_persisted = db3.sessions().get(session2).unwrap().unwrap();
    assert!(s2_persisted.crashed);
    assert_eq!(s2_persisted.ended_at, Some(now + 30_000));

    // Verify Session 1 remains cleanly ended
    let s1_persisted = db3.sessions().get(session1).unwrap().unwrap();
    assert!(!s1_persisted.crashed);
    assert_eq!(s1_persisted.ended_at, Some(now + 10_000));

    // Verify Session 3 is currently active and open
    let s3_persisted = db3.sessions().get(session3).unwrap().unwrap();
    assert!(!s3_persisted.crashed);
    assert_eq!(s3_persisted.ended_at, None);
    assert!(s3_persisted.is_open());

    // Verify recent sessions ordering
    let recents = db3.sessions().recent(10).unwrap();
    assert_eq!(recents.len(), 3);
    assert_eq!(recents[0].id, session3);
    assert_eq!(recents[1].id, session2);
    assert_eq!(recents[2].id, session1);

    // Legacy preferences file remains completely unmodified
    assert_eq!(
        fs::read(&pref_file).unwrap(),
        SAMPLE_PREFERENCES_JSON,
        "legacy preferences.json must remain byte-identical throughout session lifecycle"
    );
}
