// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Database-level tests: fresh creation, reopen, schema versioning,
//! migration scaffold, unsupported-future-version handling, and backward
//! compatibility with a pre-versioning fixture.

use redb::{Database, TableDefinition};
use valori_studio_storage::{StudioDatabase, StudioStorageError, CURRENT_SCHEMA_VERSION};

fn tmp_path() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.redb");
    (dir, path)
}

#[test]
fn fresh_database_gets_current_schema_version() {
    let (_dir, path) = tmp_path();
    let db = StudioDatabase::open(&path).unwrap();
    assert_eq!(db.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
}

#[test]
fn opening_creates_parent_directory() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("a").join("b").join("c");
    let path = nested.join("studio.redb");
    assert!(!nested.exists());

    let _db = StudioDatabase::open(&path).unwrap();
    assert!(path.exists());
}

#[test]
fn reopen_preserves_schema_version_and_data() {
    let (_dir, path) = tmp_path();
    {
        let db = StudioDatabase::open(&path).unwrap();
        db.preferences()
            .update(|p| p.theme = Some("dark".to_string()))
            .unwrap();
    }
    {
        let db = StudioDatabase::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
        assert_eq!(
            db.preferences().get().unwrap().theme,
            Some("dark".to_string())
        );
    }
}

#[test]
fn reopen_is_idempotent_across_many_cycles() {
    let (_dir, path) = tmp_path();
    for i in 0..5 {
        let db = StudioDatabase::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
        db.updates().update(|u| u.last_checked = Some(i)).unwrap();
    }
    let db = StudioDatabase::open(&path).unwrap();
    assert_eq!(db.updates().get().unwrap().last_checked, Some(4));
}

/// A database claiming a schema version newer than this build understands
/// must be refused, and left byte-for-byte untouched.
#[test]
fn unsupported_future_schema_version_fails_clearly_and_preserves_the_file() {
    let (_dir, path) = tmp_path();

    // Hand-build a database claiming a future version, bypassing
    // StudioDatabase entirely (simulating "opened by a newer build").
    const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");
    {
        let db = Database::create(&path).unwrap();
        let tx = db.begin_write().unwrap();
        {
            let mut t = tx.open_table(META).unwrap();
            let future_version = CURRENT_SCHEMA_VERSION + 999;
            t.insert(
                "schema_version",
                serde_json::to_vec(&future_version).unwrap().as_slice(),
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }

    let before = std::fs::read(&path).unwrap();
    let result = StudioDatabase::open(&path);

    match result {
        Err(StudioStorageError::UnsupportedSchemaVersion { found, supported }) => {
            assert_eq!(found, CURRENT_SCHEMA_VERSION + 999);
            assert_eq!(supported, CURRENT_SCHEMA_VERSION);
        }
        other => panic!("expected UnsupportedSchemaVersion, got {other:?}"),
    }

    let after = std::fs::read(&path).unwrap();
    assert_eq!(
        before, after,
        "refusing to open a future schema must not modify the file"
    );
}

/// Simulates a hypothetical pre-versioning database: tables already exist
/// and already hold data, but the `meta` table (and therefore
/// `schema_version`) is entirely absent — the state `read_schema_version`
/// treats identically to "brand new database." Opening it must backfill
/// the version marker WITHOUT wiping the pre-existing rows — the
/// concrete proof that `open()` never destroys user state on its own
/// database, even in the least-versioned case it can encounter.
#[test]
fn opening_a_pre_versioning_shaped_database_backfills_version_without_data_loss() {
    let (_dir, path) = tmp_path();
    const PREFERENCES: TableDefinition<&str, &[u8]> = TableDefinition::new("preferences");

    {
        let db = Database::create(&path).unwrap();
        let tx = db.begin_write().unwrap();
        {
            // Populate the preferences table directly, as if written by a
            // hypothetical earlier, unversioned build — no meta table at all.
            let mut t = tx.open_table(PREFERENCES).unwrap();
            let prefs = serde_json::json!({"theme": "light", "language": "en"});
            t.insert("singleton", serde_json::to_vec(&prefs).unwrap().as_slice())
                .unwrap();
        }
        tx.commit().unwrap();
    }

    let db = StudioDatabase::open(&path).unwrap();
    assert_eq!(db.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);

    let prefs = db.preferences().get().unwrap();
    assert_eq!(prefs.theme, Some("light".to_string()));
    assert_eq!(prefs.language, Some("en".to_string()));
}

/// A database file that is not a valid redb database (corruption, or a
/// stray file placed at the path) must surface a clear error and must not
/// be silently deleted or recreated.
#[test]
fn corrupt_or_invalid_file_fails_clearly_without_being_recreated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.redb");
    std::fs::write(&path, b"this is not a redb database, just garbage bytes").unwrap();

    let before = std::fs::read(&path).unwrap();
    let result = StudioDatabase::open(&path);
    assert!(
        result.is_err(),
        "opening a garbage file must fail, not silently recreate it"
    );

    let after = std::fs::read(&path).unwrap();
    assert_eq!(
        before, after,
        "a failed open must never modify or replace the original file"
    );
}
