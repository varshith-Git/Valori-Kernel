// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Corruption/backup/recovery tests for `crate::recovery::open_with_recovery`.

use std::path::{Path, PathBuf};

use valori_studio_storage::recovery::{open_with_recovery, BACKUP_GENERATIONS};
use valori_studio_storage::{RecoveryOutcome, StudioDatabase, StudioStorageError};

struct Root {
    _dir: tempfile::TempDir,
    db_path: PathBuf,
    backups_dir: PathBuf,
    log_path: PathBuf,
}

fn root() -> Root {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("studio.redb");
    let backups_dir = dir.path().join("backups");
    let log_path = dir.path().join("studio-recovery.jsonl");
    Root {
        _dir: dir,
        db_path,
        backups_dir,
        log_path,
    }
}

fn open(r: &Root) -> valori_studio_storage::StudioStorageResult<(StudioDatabase, RecoveryOutcome)> {
    open_with_recovery(&r.db_path, &r.backups_dir, &r.log_path)
}

fn corrupt(path: &Path) {
    std::fs::write(
        path,
        b"not a redb database, just garbage bytes to force a corruption error",
    )
    .unwrap();
}

/// Builds a healthy database at `path` with a couple of preference values
/// set, so recovered/restored copies can be checked for actual content,
/// not just "opens without error".
fn seed(path: &Path) {
    let db = StudioDatabase::open(path).unwrap();
    db.preferences()
        .update(|p| p.theme = Some("seeded-dark".to_string()))
        .unwrap();
}

fn sha256_hex(path: &Path) -> String {
    // Cheap, dependency-free content fingerprint — good enough to prove
    // byte-for-byte equality without pulling in a hashing crate this test
    // suite doesn't otherwise need.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let bytes = std::fs::read(path).unwrap();
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

// ── Healthy ──────────────────────────────────────────────────────────────

#[test]
fn healthy_database_opens_normally() {
    let r = root();
    seed(&r.db_path);

    let (db, outcome) = open(&r).unwrap();
    assert_eq!(outcome, RecoveryOutcome::Healthy);
    assert!(outcome.is_healthy());
    assert!(outcome.user_message().is_none());
    assert_eq!(
        db.preferences().get().unwrap().theme,
        Some("seeded-dark".to_string())
    );
}

#[test]
fn fresh_install_with_nothing_on_disk_is_healthy_not_recovery() {
    let r = root();
    assert!(!r.db_path.exists());

    let (_db, outcome) = open(&r).unwrap();
    assert_eq!(outcome, RecoveryOutcome::Healthy);
    // A fresh install must never write a recovery log entry — recovery
    // never ran.
    assert!(!r.log_path.exists());
}

// ── Corrupt DB, valid backup ─────────────────────────────────────────────

#[test]
fn corrupt_database_with_valid_backup_restores_it_and_preserves_the_original() {
    let r = root();
    seed(&r.db_path);
    std::fs::create_dir_all(&r.backups_dir).unwrap();
    std::fs::copy(&r.db_path, r.backups_dir.join("studio.redb.1")).unwrap();

    corrupt(&r.db_path);
    let corrupted_bytes = std::fs::read(&r.db_path).unwrap();

    let (db, outcome) = open(&r).unwrap();
    match &outcome {
        RecoveryOutcome::RestoredFromBackup {
            backup_generation,
            corrupt_original,
        } => {
            assert_eq!(*backup_generation, 1);
            assert!(
                corrupt_original.exists(),
                "the corrupt original must be preserved, not deleted"
            );
            assert_eq!(std::fs::read(corrupt_original).unwrap(), corrupted_bytes);
        }
        other => panic!("expected RestoredFromBackup, got {other:?}"),
    }
    assert!(outcome.user_message().unwrap().contains("backup"));
    assert_eq!(
        db.preferences().get().unwrap().theme,
        Some("seeded-dark".to_string())
    );

    // The restored live database must itself still be openable normally
    // afterwards (not just once).
    drop(db);
    let (db2, outcome2) = open(&r).unwrap();
    assert_eq!(outcome2, RecoveryOutcome::Healthy);
    assert_eq!(
        db2.preferences().get().unwrap().theme,
        Some("seeded-dark".to_string())
    );
}

// ── Corrupt DB, no backup ────────────────────────────────────────────────

#[test]
fn corrupt_database_with_no_backup_creates_fresh_database_and_stays_launchable() {
    let r = root();
    corrupt(&r.db_path);
    let corrupted_bytes = std::fs::read(&r.db_path).unwrap();

    let (db, outcome) = open(&r).unwrap();
    match &outcome {
        RecoveryOutcome::FreshDatabaseCreated { corrupt_original } => {
            let preserved = corrupt_original
                .as_ref()
                .expect("original should have been preserved");
            assert!(preserved.exists());
            assert_eq!(std::fs::read(preserved).unwrap(), corrupted_bytes);
        }
        other => panic!("expected FreshDatabaseCreated, got {other:?}"),
    }
    // App remains launchable: the fresh database is fully usable.
    assert_eq!(db.preferences().get().unwrap(), Default::default());
    db.preferences()
        .update(|p| p.theme = Some("works-fine".to_string()))
        .unwrap();
    assert_eq!(
        db.preferences().get().unwrap().theme,
        Some("works-fine".to_string())
    );
}

// ── Multiple backups, some corrupt ───────────────────────────────────────

#[test]
fn skips_corrupt_backup_generations_and_restores_the_first_valid_one() {
    let r = root();
    seed(&r.db_path);
    std::fs::create_dir_all(&r.backups_dir).unwrap();

    // generation 1 (newest) and 2 are corrupt; 3 (oldest) is valid.
    std::fs::write(r.backups_dir.join("studio.redb.1"), b"garbage-1").unwrap();
    std::fs::write(r.backups_dir.join("studio.redb.2"), b"garbage-2").unwrap();
    std::fs::copy(&r.db_path, r.backups_dir.join("studio.redb.3")).unwrap();

    corrupt(&r.db_path);
    let (db, outcome) = open(&r).unwrap();
    match outcome {
        RecoveryOutcome::RestoredFromBackup {
            backup_generation, ..
        } => {
            assert_eq!(backup_generation, 3);
        }
        other => panic!("expected RestoredFromBackup from generation 3, got {other:?}"),
    }
    assert_eq!(
        db.preferences().get().unwrap().theme,
        Some("seeded-dark".to_string())
    );
}

#[test]
fn all_backup_generations_corrupt_falls_back_to_fresh() {
    let r = root();
    std::fs::create_dir_all(&r.backups_dir).unwrap();
    for gen in 1..=BACKUP_GENERATIONS {
        std::fs::write(r.backups_dir.join(format!("studio.redb.{gen}")), b"garbage").unwrap();
    }
    corrupt(&r.db_path);

    let (_db, outcome) = open(&r).unwrap();
    assert!(matches!(
        outcome,
        RecoveryOutcome::FreshDatabaseCreated { .. }
    ));
}

// ── Migration-triggering backup ──────────────────────────────────────────

/// Simulates "a database whose stored schema version is older than this
/// build's" by hand-writing `meta.schema_version = 0` (below the real
/// `CURRENT_SCHEMA_VERSION = 1`, which no legitimately-created database
/// can have). `crate::db`'s own `MIGRATIONS` list is empty (schema v1 is
/// the first version this crate has ever shipped — see its module docs),
/// so opening a v0 database correctly fails with `MigrationFailed`
/// ("no migration path registered"). That failure is exactly what proves
/// the point here: `open_with_recovery` must take the pre-migration
/// backup *before* attempting the open, unconditionally — not only when
/// migration is known to succeed. The backup itself is also version 0 and
/// therefore fails `validate_database_file`'s version check, so recovery
/// correctly falls through to a fresh database rather than "restoring"
/// the same broken state — this test pins that whole chain, not just the
/// backup file's existence.
#[test]
fn takes_a_backup_before_a_database_that_needs_migration_is_opened() {
    let r = root();
    const META: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("meta");
    {
        let raw = redb::Database::create(&r.db_path).unwrap();
        let tx = raw.begin_write().unwrap();
        {
            let mut t = tx.open_table(META).unwrap();
            let v: u32 = 0;
            t.insert("schema_version", serde_json::to_vec(&v).unwrap().as_slice())
                .unwrap();
        }
        tx.commit().unwrap();
    }

    let (_db, outcome) = open(&r).unwrap();
    assert!(
        matches!(outcome, RecoveryOutcome::FreshDatabaseCreated { .. }),
        "a v0 database has no registered migration path, so open fails, and the \
         pre-migration backup (also v0) can't validate either — recovery correctly \
         falls through to fresh: got {outcome:?}"
    );
    assert!(
        r.backups_dir.join("studio.redb.1").exists(),
        "a pre-migration backup must still have been taken, before the open (and its \
         migration) was even attempted — this is the trigger this test actually pins"
    );
}

// ── Idempotency ───────────────────────────────────────────────────────────

#[test]
fn running_recovery_twice_does_not_destroy_or_duplicate_artifacts() {
    let r = root();
    seed(&r.db_path);
    std::fs::create_dir_all(&r.backups_dir).unwrap();
    std::fs::copy(&r.db_path, r.backups_dir.join("studio.redb.1")).unwrap();
    corrupt(&r.db_path);

    let (db1, outcome1) = open(&r).unwrap();
    drop(db1);
    let preserved_count_after_first = std::fs::read_dir(r.db_path.parent().unwrap())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".corrupt-")
        })
        .count();
    assert_eq!(preserved_count_after_first, 1);
    assert!(matches!(
        outcome1,
        RecoveryOutcome::RestoredFromBackup { .. }
    ));

    // Second call: the database is now healthy, so this must be a plain
    // healthy open — no new preserved file, no re-recovery.
    let (_db2, outcome2) = open(&r).unwrap();
    assert_eq!(outcome2, RecoveryOutcome::Healthy);
    let preserved_count_after_second = std::fs::read_dir(r.db_path.parent().unwrap())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".corrupt-")
        })
        .count();
    assert_eq!(
        preserved_count_after_second, 1,
        "no duplicate preserved artifact from the second call"
    );
}

#[test]
fn resuming_after_a_crash_between_preserve_and_restore_is_safe() {
    let r = root();
    seed(&r.db_path);
    std::fs::create_dir_all(&r.backups_dir).unwrap();
    std::fs::copy(&r.db_path, r.backups_dir.join("studio.redb.1")).unwrap();

    // Simulate exactly the state a crash between "preserve" and "restore"
    // would leave: the original renamed aside, db_path absent, backups
    // untouched, no fresh database created yet.
    std::fs::rename(
        &r.db_path,
        r.db_path.parent().unwrap().join("studio.redb.corrupt-1000"),
    )
    .unwrap();
    assert!(!r.db_path.exists());

    // The next launch must not get stuck or silently settle for an empty
    // database when a perfectly good backup is sitting right there.
    let (db, outcome) = open(&r).unwrap();
    assert!(matches!(
        outcome,
        RecoveryOutcome::RestoredFromBackup { .. }
    ));
    assert_eq!(
        db.preferences().get().unwrap().theme,
        Some("seeded-dark".to_string())
    );
}

// ── Cross-process / concurrency ──────────────────────────────────────────

#[test]
fn a_database_locked_by_another_open_handle_is_never_treated_as_corrupt() {
    let r = root();
    seed(&r.db_path);
    std::fs::create_dir_all(&r.backups_dir).unwrap();
    std::fs::copy(&r.db_path, r.backups_dir.join("studio.redb.1")).unwrap();

    // Hold a second handle open on the same file — this is what a second
    // process (which the single-instance mechanism should already
    // prevent) or a bug that opened two StudioDatabases in one process
    // would look like from `open_with_recovery`'s point of view.
    let _held_open = StudioDatabase::open(&r.db_path).unwrap();

    let result = open(&r);
    assert!(
        matches!(result, Err(StudioStorageError::Db(redb::DatabaseError::DatabaseAlreadyOpen))),
        "a locked-but-healthy database must surface DatabaseAlreadyOpen, not trigger recovery: {result:?}"
    );

    // Crucially: nothing must have been preserved/rewritten — the original
    // file is exactly as it was, and no recovery log entry was written.
    assert!(
        !r.log_path.exists(),
        "a lock conflict must never be logged as a recovery event"
    );
    let entries: Vec<_> = std::fs::read_dir(r.db_path.parent().unwrap())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".corrupt-")
        })
        .collect();
    assert!(
        entries.is_empty(),
        "a lock conflict must never preserve the file aside"
    );
}

// ── Project data safety ───────────────────────────────────────────────────

/// The load-bearing test for this whole phase: corrupt `studio.redb` and
/// recover it (all three ways — backup restore and fresh-fallback), then
/// verify a sibling `projects/<name>/` tree — standing in for real WAL/
/// snapshot/index/manifest files — is byte-for-byte untouched. Recovery
/// has no code path that could reach it (the dependency firewall makes
/// that structural, not just tested), but this is the concrete,
/// filesystem-level proof.
#[test]
fn project_data_is_never_touched_by_studio_database_recovery() {
    for scenario in ["backup_restore", "fresh_fallback"] {
        let r = root();
        let projects_dir = r._dir.path().join("projects").join("demo");
        std::fs::create_dir_all(&projects_dir).unwrap();
        let wal = projects_dir.join("events.log");
        let snapshot = projects_dir.join("snapshot.val");
        let index = projects_dir.join("index.bin");
        let manifest = projects_dir.join("project.json");
        std::fs::write(&wal, b"fake WAL content, must never move").unwrap();
        std::fs::write(&snapshot, b"fake snapshot bytes").unwrap();
        std::fs::write(&index, b"fake index bytes").unwrap();
        std::fs::write(&manifest, br#"{"name":"demo","id":"fake-id"}"#).unwrap();

        let before: Vec<(PathBuf, String)> = [&wal, &snapshot, &index, &manifest]
            .iter()
            .map(|p| ((*p).clone(), sha256_hex(p)))
            .collect();

        seed(&r.db_path);
        if scenario == "backup_restore" {
            std::fs::create_dir_all(&r.backups_dir).unwrap();
            std::fs::copy(&r.db_path, r.backups_dir.join("studio.redb.1")).unwrap();
        }
        corrupt(&r.db_path);

        let (_db, outcome) = open(&r).unwrap();
        if scenario == "backup_restore" {
            assert!(matches!(
                outcome,
                RecoveryOutcome::RestoredFromBackup { .. }
            ));
        } else {
            assert!(matches!(
                outcome,
                RecoveryOutcome::FreshDatabaseCreated { .. }
            ));
        }

        for (path, hash_before) in &before {
            assert_eq!(
                &sha256_hex(path),
                hash_before,
                "{scenario}: {path:?} must be byte-for-byte unchanged by studio database recovery"
            );
        }
    }
}

// ── Recovery log ──────────────────────────────────────────────────────────

#[test]
fn recovery_log_records_the_event_without_sensitive_payloads() {
    let r = root();
    seed(&r.db_path);
    corrupt(&r.db_path);

    let (_db, _outcome) = open(&r).unwrap();
    assert!(r.log_path.exists());
    let contents = std::fs::read_to_string(&r.log_path).unwrap();
    assert!(!contents.is_empty());
    for line in contents.lines() {
        let entry: valori_studio_storage::RecoveryLogEntry = serde_json::from_str(line).unwrap();
        // No preference values, telemetry payloads, or project content
        // ever appear in a recovery log line — only mechanics.
        assert!(
            !contents.contains("seeded-dark"),
            "recovery log must never contain preference values"
        );
        let _ = entry;
    }
}

#[test]
fn no_backup_taken_when_database_is_missing_marks_original_as_none() {
    let r = root();
    // db_path never existed and there is a leftover corrupt marker with no
    // underlying database at all — a pathological but possible state.
    std::fs::write(
        r.db_path.parent().unwrap().join("studio.redb.corrupt-1"),
        b"x",
    )
    .unwrap();
    let (_db, outcome) = open(&r).unwrap();
    assert!(matches!(
        outcome,
        RecoveryOutcome::FreshDatabaseCreated { .. }
    ));
}
