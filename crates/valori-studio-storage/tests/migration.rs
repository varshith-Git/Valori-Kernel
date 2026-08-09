// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! S2a legacy-migration tests.
//!
//! Fixtures are hand-written to match the *real* on-disk shapes: the
//! `preferences.json` fixtures mirror `tauri-plugin-store`'s flat
//! `{key: value}` object using the exact keys `ui/src/lib/native.ts` writes
//! (`onboardingVersion`, `telemetryConsent`, `installationId`,
//! `recentProjects`, `favoriteProjects`, `lastOpenedProject`, `lastPage`);
//! the `events.jsonl` fixtures mirror `desktop/src-tauri/src/telemetry.rs`'s
//! `TelemetryEnvelope` (one JSON object per line, RFC3339 `timestamp`).

use valori_domain::SessionId;
use valori_studio_storage::StudioDatabase;

fn open_tmp() -> (tempfile::TempDir, StudioDatabase) {
    let dir = tempfile::tempdir().unwrap();
    let db = StudioDatabase::open(&dir.path().join("studio.redb")).unwrap();
    (dir, db)
}

const REAL_SHAPED_PREFERENCES: &[u8] = br#"{
  "onboardingVersion": 3,
  "telemetryConsent": { "analytics": true, "crash": false },
  "installationId": "b6b6a6f0-1a2b-4c3d-8e9f-0123456789ab",
  "recentProjects": ["demo", "acme-corp", "scratch"],
  "favoriteProjects": ["acme-corp"],
  "lastOpenedProject": "demo",
  "lastPage": "/projects/demo"
}"#;

// ── preferences.json ──────────────────────────────────────────────────────

#[test]
fn migrates_real_shaped_preferences_json() {
    let (_dir, db) = open_tmp();
    let report = db
        .migrate_legacy_preferences(REAL_SHAPED_PREFERENCES, 1_000)
        .unwrap();

    assert!(!report.already_migrated);
    assert!(report.source_found);
    assert_eq!(report.imported, 1);
    assert!(report.skipped.is_empty());

    let prefs = db.preferences().get().unwrap();
    assert_eq!(prefs.onboarding_version, Some(3));
    assert_eq!(prefs.telemetry_consent.unwrap().analytics, true);
    assert_eq!(prefs.last_page, Some("/projects/demo".to_string()));
    assert_eq!(
        prefs.installation_id.unwrap().to_string(),
        "b6b6a6f0-1a2b-4c3d-8e9f-0123456789ab"
    );
}

#[test]
fn preferences_migration_never_touches_the_projects_table() {
    let (_dir, db) = open_tmp();
    db.migrate_legacy_preferences(REAL_SHAPED_PREFERENCES, 1_000)
        .unwrap();
    assert!(
        db.projects().list().unwrap().is_empty(),
        "legacy names must not become ProjectId-keyed records"
    );
}

#[test]
fn preferences_migration_preserves_legacy_project_names_as_residue() {
    let (_dir, db) = open_tmp();
    db.migrate_legacy_preferences(REAL_SHAPED_PREFERENCES, 1_000)
        .unwrap();

    let names = db.legacy_project_names().unwrap().unwrap();
    assert_eq!(names.recent, vec!["demo", "acme-corp", "scratch"]);
    assert_eq!(names.favorite, vec!["acme-corp"]);
    assert_eq!(names.last_opened, Some("demo".to_string()));
}

#[test]
fn preferences_migration_is_idempotent() {
    let (_dir, db) = open_tmp();
    let first = db
        .migrate_legacy_preferences(REAL_SHAPED_PREFERENCES, 1_000)
        .unwrap();
    assert!(!first.already_migrated);

    // Change what a fresh read of the file would produce, to prove the
    // second call really is a no-op and does not re-import.
    let different = br#"{"onboardingVersion": 99}"#;
    let second = db.migrate_legacy_preferences(different, 2_000).unwrap();
    assert!(second.already_migrated);

    assert_eq!(
        db.preferences().get().unwrap().onboarding_version,
        Some(3),
        "second call must not have re-imported"
    );
}

#[test]
fn preferences_migration_merges_onto_existing_studio_preferences() {
    let (_dir, db) = open_tmp();
    // Something already wrote a preference directly through StudioDatabase
    // before migration ever runs (e.g. a build that already partially
    // wires this crate).
    db.preferences()
        .update(|p| p.theme = Some("dark".to_string()))
        .unwrap();

    db.migrate_legacy_preferences(REAL_SHAPED_PREFERENCES, 1_000)
        .unwrap();

    let prefs = db.preferences().get().unwrap();
    assert_eq!(
        prefs.theme,
        Some("dark".to_string()),
        "pre-existing fields the legacy source doesn't know about must survive"
    );
    assert_eq!(
        prefs.onboarding_version,
        Some(3),
        "legacy fields must still be applied"
    );
}

#[test]
fn preferences_migration_tolerates_unknown_fields_and_never_copies_secrets() {
    let (_dir, db) = open_tmp();
    let with_extra = br#"{
        "onboardingVersion": 1,
        "apiKey": "sk-should-never-be-copied",
        "someFutureField": {"nested": true}
    }"#;
    let report = db.migrate_legacy_preferences(with_extra, 1_000).unwrap();
    assert_eq!(report.imported, 1);

    // Structural guard: the serialized preferences record must never
    // contain the secret value, under any field name.
    let prefs = db.preferences().get().unwrap();
    let raw = serde_json::to_string(&prefs).unwrap();
    assert!(!raw.contains("sk-should-never-be-copied"));
}

#[test]
fn missing_preferences_file_is_reported_not_erred() {
    let (_dir, db) = open_tmp();
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist.json");

    let report = db
        .migrate_legacy_preferences_from_path(&missing, 1_000)
        .unwrap();
    assert!(!report.source_found);
    assert_eq!(report.imported, 0);
    assert!(!report.already_migrated);

    // A missing source must not set the completed flag — a file that
    // appears later should still be picked up.
    let real_file = dir.path().join("preferences.json");
    std::fs::write(&real_file, REAL_SHAPED_PREFERENCES).unwrap();
    let second = db
        .migrate_legacy_preferences_from_path(&real_file, 2_000)
        .unwrap();
    assert!(second.source_found);
    assert_eq!(second.imported, 1);
}

#[test]
fn malformed_preferences_json_fails_the_whole_call_without_partial_writes() {
    let (_dir, db) = open_tmp();
    let garbage: &[u8] = b"{ this is not valid json";
    let result = db.migrate_legacy_preferences(garbage, 1_000);
    assert!(result.is_err());

    // Nothing must have been written — the flag must be absent so a retry
    // (once the file is fixed) is still possible.
    assert_eq!(db.preferences().get().unwrap(), Default::default());
    let retry = db
        .migrate_legacy_preferences(REAL_SHAPED_PREFERENCES, 2_000)
        .unwrap();
    assert!(
        !retry.already_migrated,
        "a failed attempt must not have set the completed flag"
    );
}

// ── events.jsonl ──────────────────────────────────────────────────────────

fn envelope(event_id: &str, event: &str, timestamp: &str, session_id: Option<&str>) -> String {
    let session = match session_id {
        Some(id) => format!("\"{id}\""),
        None => "null".to_string(),
    };
    format!(
        r#"{{"schema":1,"source":"desktop","event_id":"{event_id}","timestamp":"{timestamp}","session_id":{session},"installation_id":"b6b6a6f0-1a2b-4c3d-8e9f-0123456789ab","version":"0.2.4","platform":"macos","arch":"aarch64","event":"{event}","properties":{{"k":"v"}}}}"#
    )
}

#[test]
fn migrates_real_shaped_events_jsonl() {
    let (_dir, db) = open_tmp();
    let session = SessionId::new();
    let jsonl = format!(
        "{}\n{}\n",
        envelope(
            "11111111-1111-1111-1111-111111111111",
            "app_launched",
            "2026-01-01T00:00:00Z",
            Some(&session.to_string())
        ),
        envelope(
            "22222222-2222-2222-2222-222222222222",
            "update_checked",
            "2026-01-01T00:05:00Z",
            None
        ),
    );

    let report = db
        .migrate_legacy_telemetry_queue(jsonl.as_bytes(), 1_000)
        .unwrap();
    assert!(!report.already_migrated);
    assert!(report.source_found);
    assert_eq!(report.imported, 2);
    assert!(report.skipped.is_empty());

    let batch = db.telemetry().peek_batch(10).unwrap();
    assert_eq!(batch.len(), 2);
    assert_eq!(batch[0].event_name, "app_launched");
    assert_eq!(batch[0].session_id, Some(session));
    assert_eq!(batch[1].event_name, "update_checked");
    assert_eq!(batch[1].session_id, None);
}

#[test]
fn events_jsonl_ordering_survives_migration() {
    let (_dir, db) = open_tmp();
    let jsonl = format!(
        "{}\n{}\n{}\n",
        envelope("a", "third", "2026-01-01T00:10:00Z", None),
        envelope("b", "first", "2026-01-01T00:00:00Z", None),
        envelope("c", "second", "2026-01-01T00:05:00Z", None),
    );
    db.migrate_legacy_telemetry_queue(jsonl.as_bytes(), 1_000)
        .unwrap();

    let batch = db.telemetry().peek_batch(10).unwrap();
    assert_eq!(
        batch
            .iter()
            .map(|e| e.event_name.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "second", "third"]
    );
}

#[test]
fn malformed_lines_are_skipped_not_fatal() {
    let (_dir, db) = open_tmp();
    let jsonl = format!(
        "{}\nnot even json\n{}\n",
        envelope("a", "good-one", "2026-01-01T00:00:00Z", None),
        envelope("b", "good-two", "2026-01-01T00:01:00Z", None),
    );
    let report = db
        .migrate_legacy_telemetry_queue(jsonl.as_bytes(), 1_000)
        .unwrap();

    assert_eq!(report.imported, 2);
    assert_eq!(report.skipped.len(), 1);
    assert!(report.skipped[0].reason.contains("malformed"));
}

#[test]
fn invalid_session_id_is_skipped_as_a_field_but_event_still_imports() {
    let (_dir, db) = open_tmp();
    let jsonl = format!(
        "{}\n",
        envelope("a", "event", "2026-01-01T00:00:00Z", Some("not-a-uuid"))
    );
    let report = db
        .migrate_legacy_telemetry_queue(jsonl.as_bytes(), 1_000)
        .unwrap();

    // The event itself imports (with session_id dropped); the invalid
    // session id is recorded as a skip reason, not a lost event.
    assert_eq!(report.imported, 1);
    assert_eq!(report.skipped.len(), 1);
    assert!(report.skipped[0].reason.contains("invalid session id"));

    let batch = db.telemetry().peek_batch(10).unwrap();
    assert_eq!(batch[0].session_id, None);
}

#[test]
fn unparseable_timestamp_skips_the_line() {
    let (_dir, db) = open_tmp();
    let jsonl = r#"{"schema":1,"source":"desktop","event_id":"a","timestamp":"not-a-date","session_id":null,"installation_id":"x","version":"0.2.4","platform":"macos","arch":"aarch64","event":"e","properties":{}}"#.to_string() + "\n";
    let report = db
        .migrate_legacy_telemetry_queue(jsonl.as_bytes(), 1_000)
        .unwrap();

    assert_eq!(report.imported, 0);
    assert_eq!(report.skipped.len(), 1);
    assert!(report.skipped[0].reason.contains("timestamp"));
}

#[test]
fn telemetry_migration_is_idempotent() {
    let (_dir, db) = open_tmp();
    let jsonl = format!("{}\n", envelope("a", "e", "2026-01-01T00:00:00Z", None));

    let first = db
        .migrate_legacy_telemetry_queue(jsonl.as_bytes(), 1_000)
        .unwrap();
    assert_eq!(first.imported, 1);

    let different = format!("{}\n", envelope("b", "e2", "2026-01-01T00:00:00Z", None));
    let second = db
        .migrate_legacy_telemetry_queue(different.as_bytes(), 2_000)
        .unwrap();
    assert!(second.already_migrated);
    assert_eq!(
        db.telemetry().count().unwrap(),
        1,
        "second call must not have imported the different file"
    );
}

#[test]
fn telemetry_migration_respects_queue_capacity_keeping_newest() {
    let (_dir, db) = open_tmp();
    let mut jsonl = String::new();
    for i in 0..(valori_studio_storage::telemetry::MAX_QUEUE_LEN + 5) {
        jsonl.push_str(&envelope(
            &format!("event-{i:04}"),
            "e",
            &format!("2026-01-01T00:{:02}:{:02}Z", (i / 60) % 60, i % 60),
            None,
        ));
        jsonl.push('\n');
    }

    let report = db
        .migrate_legacy_telemetry_queue(jsonl.as_bytes(), 1_000)
        .unwrap();
    assert_eq!(
        report.imported,
        valori_studio_storage::telemetry::MAX_QUEUE_LEN
    );
    assert_eq!(report.skipped.len(), 5);
    for skip in &report.skipped {
        assert_eq!(skip.reason, "queue capacity");
    }
    assert_eq!(
        db.telemetry().count().unwrap(),
        valori_studio_storage::telemetry::MAX_QUEUE_LEN
    );
}

#[test]
fn missing_events_jsonl_is_reported_not_erred() {
    let (_dir, db) = open_tmp();
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("events.jsonl");

    let report = db
        .migrate_legacy_telemetry_queue_from_path(&missing, 1_000)
        .unwrap();
    assert!(!report.source_found);
    assert_eq!(report.imported, 0);
}

// ── run_legacy_migration orchestrator ────────────────────────────────────

#[test]
fn run_legacy_migration_handles_both_sources_independently() {
    let (_dir, db) = open_tmp();
    let dir = tempfile::tempdir().unwrap();
    let prefs_path = dir.path().join("preferences.json");
    let events_path = dir.path().join("events.jsonl");
    std::fs::write(&prefs_path, REAL_SHAPED_PREFERENCES).unwrap();
    std::fs::write(
        &events_path,
        format!("{}\n", envelope("a", "e", "2026-01-01T00:00:00Z", None)),
    )
    .unwrap();

    let paths = valori_studio_storage::LegacyStudioPaths {
        preferences_json: Some(prefs_path),
        events_jsonl: Some(events_path),
    };
    let summary = db.run_legacy_migration(&paths, 1_000);

    assert_eq!(summary.preferences.unwrap().imported, 1);
    assert_eq!(summary.telemetry.unwrap().imported, 1);
}

#[test]
fn run_legacy_migration_with_no_paths_is_a_harmless_no_op() {
    let (_dir, db) = open_tmp();
    let summary =
        db.run_legacy_migration(&valori_studio_storage::LegacyStudioPaths::default(), 1_000);
    assert!(!summary.preferences.unwrap().source_found);
    assert!(!summary.telemetry.unwrap().source_found);
}

#[test]
fn legacy_files_are_never_modified_by_migration() {
    let (_dir, db) = open_tmp();
    let dir = tempfile::tempdir().unwrap();
    let prefs_path = dir.path().join("preferences.json");
    let events_path = dir.path().join("events.jsonl");
    std::fs::write(&prefs_path, REAL_SHAPED_PREFERENCES).unwrap();
    let events_content = format!("{}\n", envelope("a", "e", "2026-01-01T00:00:00Z", None));
    std::fs::write(&events_path, &events_content).unwrap();

    let prefs_before = std::fs::read(&prefs_path).unwrap();
    let events_before = std::fs::read(&events_path).unwrap();

    db.migrate_legacy_preferences_from_path(&prefs_path, 1_000)
        .unwrap();
    db.migrate_legacy_telemetry_queue_from_path(&events_path, 1_000)
        .unwrap();

    assert_eq!(std::fs::read(&prefs_path).unwrap(), prefs_before);
    assert_eq!(std::fs::read(&events_path).unwrap(), events_before);
    assert!(prefs_path.exists());
    assert!(events_path.exists());
}
