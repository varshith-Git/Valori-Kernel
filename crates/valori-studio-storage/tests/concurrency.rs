// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Concurrency and durability tests.
//!
//! `StudioDatabase` wraps a bare `redb::Database` with no external
//! `Mutex`/`RwLock` (see `crate::db` module docs) — redb serializes
//! writers internally and lets readers proceed without blocking on a
//! writer. These tests exercise exactly the access pattern the crate
//! actually supports: one `StudioDatabase` shared by reference (`&`,
//! or `Arc<StudioDatabase>` in a real caller) across threads, each
//! thread calling `&self` methods that open and commit their own
//! transaction.

use std::sync::Arc;
use std::thread;

use valori_domain::SessionId;
use valori_studio_storage::telemetry::{StudioTelemetryEvent, TelemetryCategory};
use valori_studio_storage::StudioDatabase;

fn open_tmp() -> (tempfile::TempDir, Arc<StudioDatabase>) {
    let dir = tempfile::tempdir().unwrap();
    let db = StudioDatabase::open(&dir.path().join("studio.redb")).unwrap();
    (dir, Arc::new(db))
}

/// Many threads writing distinct rows to the same table concurrently: no
/// write may be lost, even though redb serializes the underlying
/// transactions.
#[test]
fn concurrent_writers_to_the_same_table_lose_no_writes() {
    let (_dir, db) = open_tmp();
    const THREADS: usize = 16;

    let handles: Vec<_> = (0..THREADS)
        .map(|i| {
            let db = Arc::clone(&db);
            thread::spawn(move || {
                let event = StudioTelemetryEvent::new(
                    format!("event-{i}"),
                    None,
                    serde_json::json!({"i": i}),
                    i as i64,
                    TelemetryCategory::Analytics,
                );
                db.telemetry().enqueue(&event).unwrap();
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(db.telemetry().count().unwrap(), THREADS);
}

/// Threads writing to different tables concurrently must not deadlock or
/// interfere with each other.
#[test]
fn concurrent_writers_to_different_tables_succeed() {
    let (_dir, db) = open_tmp();

    let db1 = Arc::clone(&db);
    let t1 = thread::spawn(move || {
        for i in 0..20 {
            db1.preferences()
                .update(|p| p.last_page = Some(format!("/page/{i}")))
                .unwrap();
        }
    });

    let db2 = Arc::clone(&db);
    let t2 = thread::spawn(move || {
        for i in 0..20 {
            let id = SessionId::new();
            db2.sessions().start(id, None, "0.2.4", "macos", i).unwrap();
        }
    });

    t1.join().unwrap();
    t2.join().unwrap();

    assert_eq!(db.sessions().list().unwrap().len(), 20);
}

/// A reader must not block on a concurrent writer, and must see either the
/// pre-write or post-write state — never a torn/partial value.
#[test]
fn concurrent_reads_observe_consistent_snapshots() {
    let (_dir, db) = open_tmp();
    db.preferences()
        .update(|p| p.theme = Some("light".to_string()))
        .unwrap();

    let writer_db = Arc::clone(&db);
    let writer = thread::spawn(move || {
        for _ in 0..50 {
            writer_db
                .preferences()
                .update(|p| {
                    p.theme = Some(
                        if p.theme.as_deref() == Some("light") {
                            "dark"
                        } else {
                            "light"
                        }
                        .to_string(),
                    );
                })
                .unwrap();
        }
    });

    let mut readers = Vec::new();
    for _ in 0..8 {
        let reader_db = Arc::clone(&db);
        readers.push(thread::spawn(move || {
            for _ in 0..50 {
                let prefs = reader_db.preferences().get().unwrap();
                // Never a torn value — always exactly one of the two valid themes.
                assert!(matches!(
                    prefs.theme.as_deref(),
                    Some("light") | Some("dark")
                ));
            }
        }));
    }

    writer.join().unwrap();
    for r in readers {
        r.join().unwrap();
    }
}

/// A panic inside an `update()` closure must not leave the database in a
/// half-written state — the write transaction is dropped (not committed)
/// on unwind, so the stored value is exactly what it was before the call.
#[test]
fn panicking_update_closure_does_not_corrupt_or_partially_apply() {
    let (_dir, db) = open_tmp();
    db.preferences()
        .update(|p| p.theme = Some("light".to_string()))
        .unwrap();

    let db_for_panic = Arc::clone(&db);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        db_for_panic
            .preferences()
            .update(|p| {
                p.theme = Some("dark".to_string());
                panic!("simulated failure mid-update");
            })
            .unwrap();
    }));
    assert!(
        result.is_err(),
        "the panic must propagate, not be swallowed"
    );

    // The aborted transaction must not have committed the partial change.
    assert_eq!(
        db.preferences().get().unwrap().theme,
        Some("light".to_string())
    );
}

/// After many writes across threads, the database must still open cleanly
/// and report the expected content — the end-to-end durability property
/// this crate relies on (backed by redb's fsync-on-commit guarantee, the
/// same one already trusted for the Raft log — see
/// `docs/architecture/studio-storage.md` §"Durability").
#[test]
fn survives_reopen_after_heavy_concurrent_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.redb");
    let db = Arc::new(StudioDatabase::open(&path).unwrap());

    let handles: Vec<_> = (0..32)
        .map(|i| {
            let db = Arc::clone(&db);
            thread::spawn(move || {
                let event = StudioTelemetryEvent::new(
                    format!("e{i}"),
                    None,
                    serde_json::json!({}),
                    i as i64,
                    TelemetryCategory::Analytics,
                );
                db.telemetry().enqueue(&event).unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    drop(db);

    let reopened = StudioDatabase::open(&path).unwrap();
    assert_eq!(reopened.telemetry().count().unwrap(), 32);
}
