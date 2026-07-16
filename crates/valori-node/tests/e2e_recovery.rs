// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! End-to-end crash recovery tests.
//!
//! Verifies the fundamental durability contract:
//!   insert records → event log written → drop engine (simulates crash)
//!   → new engine calls try_recover() → state is identical to pre-crash state.
//!
//! All correctness is checked via `engine.get_proof().final_state_hash` — the
//! BLAKE3 state hash that drives the deterministic proof system.

use valori_node::config::{NodeConfig, IndexKind};
use valori_node::EngineFromNodeConfig;
use valori_node::engine::{Engine, RecoveryMode};
use tempfile::tempdir;

fn make_cfg(dir: &std::path::Path, dim: usize) -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.dim = dim;
    cfg.max_records = 128;
    cfg.max_nodes = 128;
    cfg.max_edges = 256;
    cfg.index_kind = IndexKind::BruteForce;
    cfg.event_log_path = Some(dir.join("events.log"));
    cfg.snapshot_path = Some(dir.join("snapshot.bin"));
    // No WAL — event log is canonical when configured
    cfg.wal_path = None;
    cfg
}

// ── Test 1: basic event-log recovery ──────────────────────────────────────────

#[test]
fn test_event_log_recovery_basic() {
    let dir = tempdir().unwrap();
    let cfg = make_cfg(dir.path(), 4);

    let pre_crash_hash;
    let n_inserted = 50usize;

    // ── Phase 1: create engine and insert records ──────────────────────────
    {
        let mut engine = Engine::new(&cfg);
        assert_eq!(engine.try_recover(), RecoveryMode::Fresh, "should start fresh");

        for i in 0..n_inserted {
            let v: Vec<f32> = (0..4).map(|j| (i * 10 + j) as f32 * 0.01).collect();
            engine.insert_record_from_f32(&v).expect("insert failed");
        }

        assert_eq!(engine.record_count(), n_inserted);
        pre_crash_hash = engine.get_proof().final_state_hash;

        // Engine is dropped here → BufWriter flushes → events reach disk.
    }

    // ── Phase 2: new engine recovers from event log ────────────────────────
    {
        let mut engine2 = Engine::new(&cfg);
        let mode = engine2.try_recover();

        assert!(
            matches!(mode, RecoveryMode::EventLog(n) if n == n_inserted as u64),
            "expected EventLog({}) recovery, got {:?}",
            n_inserted,
            mode
        );

        assert_eq!(engine2.record_count(), n_inserted,
            "record count must match after recovery");

        let post_recovery_hash = engine2.get_proof().final_state_hash;
        assert_eq!(
            pre_crash_hash, post_recovery_hash,
            "BLAKE3 state hash must be identical after event-log recovery"
        );
    }
}

// ── Test 2: snapshot fallback when event log is absent ────────────────────────

#[test]
fn test_snapshot_fallback_recovery() {
    let dir = tempdir().unwrap();

    // Config with snapshot but NO event log (WAL-only path)
    let mut cfg = NodeConfig::default();
    cfg.dim = 4;
    cfg.max_records = 64;
    cfg.max_nodes = 64;
    cfg.max_edges = 128;
    cfg.snapshot_path = Some(dir.path().join("snapshot.bin"));
    cfg.event_log_path = None;
    cfg.wal_path = None;

    let pre_crash_hash;

    // Phase 1: insert + save snapshot
    {
        let mut engine = Engine::new(&cfg);
        engine.try_recover(); // will be Fresh

        for i in 0..20u32 {
            let v: Vec<f32> = (0..4).map(|j| (i * 5 + j as u32) as f32 * 0.1).collect();
            engine.insert_record_from_f32(&v).expect("insert");
        }
        engine.save_snapshot(None).expect("save snapshot");
        pre_crash_hash = engine.get_proof().final_state_hash;
    }

    // Phase 2: fresh engine recovers from snapshot
    {
        let mut engine2 = Engine::new(&cfg);
        let mode = engine2.try_recover();

        assert_eq!(mode, RecoveryMode::Snapshot,
            "should recover from snapshot when no event log");
        assert_eq!(engine2.record_count(), 20);
        assert_eq!(pre_crash_hash, engine2.get_proof().final_state_hash,
            "state hash must match after snapshot recovery");
    }
}

// ── Test 3: event log beats snapshot when both exist ─────────────────────────

#[test]
fn test_event_log_wins_over_snapshot() {
    let dir = tempdir().unwrap();
    let cfg = make_cfg(dir.path(), 4);

    let hash_after_30;

    // Phase 1: insert 20, save snapshot, then insert 10 more events
    {
        let mut engine = Engine::new(&cfg);
        engine.try_recover();

        for i in 0..20u32 {
            let v: Vec<f32> = (0..4).map(|j| (i + j as u32) as f32 * 0.05).collect();
            engine.insert_record_from_f32(&v).unwrap();
        }
        engine.save_snapshot(None).unwrap(); // snapshot has 20 records

        for i in 20..30u32 {
            let v: Vec<f32> = (0..4).map(|j| (i + j as u32) as f32 * 0.05).collect();
            engine.insert_record_from_f32(&v).unwrap();
        }

        assert_eq!(engine.record_count(), 30);
        hash_after_30 = engine.get_proof().final_state_hash;
        // Drop → flush event log (30 events on disk, snapshot has only 20)
    }

    // Phase 2: recovery must replay all 30 events from event log, NOT use snapshot
    {
        let mut engine2 = Engine::new(&cfg);
        let mode = engine2.try_recover();

        assert!(
            matches!(mode, RecoveryMode::EventLog(30)),
            "event log must win over snapshot; got {:?}", mode
        );
        assert_eq!(engine2.record_count(), 30,
            "should have 30 records (10 post-snapshot events replayed)");
        assert_eq!(hash_after_30, engine2.get_proof().final_state_hash,
            "post-snapshot events must be in recovered state");
    }
}

// ── Test 4: fresh start when nothing exists ────────────────────────────────────

#[test]
fn test_fresh_start_when_nothing_exists() {
    let dir = tempdir().unwrap();
    let cfg = make_cfg(dir.path(), 4);

    let mut engine = Engine::new(&cfg);
    let mode = engine.try_recover();

    assert_eq!(mode, RecoveryMode::Fresh);
    assert_eq!(engine.record_count(), 0);
}

// ── Test 6: metadata sidecar survives crash and event-log recovery ─────────────
//
// `MetadataStore` lives in memory only; there is no `SetMetadata` kernel event.
// To survive an event-log recovery, `Engine::flush_metadata()` writes an atomic
// JSON sidecar, and `try_recover()` calls `load_metadata()` after replay.
// This test verifies the full round-trip.

#[test]
fn test_metadata_persists_through_event_log_recovery() {
    let dir = tempdir().unwrap();
    let cfg = make_cfg(dir.path(), 4);

    // Phase 1: insert a record, set metadata, then "crash" (drop).
    {
        let mut engine = Engine::new(&cfg);
        engine.try_recover();

        let rid = engine.insert_record_from_f32(&[1.0, 0.0, 0.0, 0.0]).unwrap();

        let key = format!("record_{}", rid);
        engine.metadata.set(key.clone(), serde_json::json!({ "label": "cat", "score": 0.95 }));
        engine.flush_metadata().expect("flush_metadata must succeed");
        // Drop → event log flushed to disk, sidecar already written.
    }

    // Phase 2: new engine recovers from event log; metadata sidecar is loaded.
    {
        let mut engine2 = Engine::new(&cfg);
        let mode = engine2.try_recover();

        assert!(
            matches!(mode, RecoveryMode::EventLog(1)),
            "expected EventLog(1), got {:?}", mode
        );

        let val = engine2.metadata.get("record_0")
            .expect("metadata must survive recovery");
        assert_eq!(val["label"], serde_json::json!("cat"),
            "label must round-trip through sidecar");
        assert_eq!(val["score"], serde_json::json!(0.95),
            "score must round-trip through sidecar");
    }
}

// ── Test 4b: collections (namespaces) survive event-log recovery ──────────────
//
// Regression for the UI bug: after a hard restart, projects were visible (from
// the UI manifest) but their collections vanished. Root cause: collection names
// live only in `engine.namespaces`, which the event log does not carry and the
// event-log recovery path did not rebuild. The fix mirrors the metadata sidecar:
// `create_collection` writes `namespaces.json`, and `try_recover()` loads it.

#[test]
fn test_collections_persist_through_event_log_recovery() {
    let dir = tempdir().unwrap();
    let cfg = make_cfg(dir.path(), 4);

    // Phase 1: create collections, insert a record into one, then "crash".
    {
        let mut engine = Engine::new(&cfg);
        engine.try_recover();

        let id_a = engine.create_collection("proj--docs").expect("create docs");
        let _id_b = engine.create_collection("proj--notes").expect("create notes");

        // Put a record in one collection so the event log is non-empty and the
        // recovery path is EventLog (not Fresh).
        engine.insert_record_from_f32_ns(&[1.0, 0.0, 0.0, 0.0], id_a)
            .expect("insert into collection");
        // Drop → event log flushed; namespace sidecar already written.
    }

    // Phase 2: new engine recovers; collection names must come back.
    {
        let mut engine2 = Engine::new(&cfg);
        let mode = engine2.try_recover();
        assert!(
            matches!(mode, RecoveryMode::EventLog(_)),
            "expected EventLog recovery, got {:?}", mode
        );

        let names: Vec<String> = engine2.list_collections()
            .into_iter().map(|(n, _)| n).collect();
        assert!(names.contains(&"proj--docs".to_string()),
            "collection 'proj--docs' must survive restart, got {:?}", names);
        assert!(names.contains(&"proj--notes".to_string()),
            "collection 'proj--notes' must survive restart, got {:?}", names);

        // Names must still resolve to the same ids (so existing records map back).
        assert!(engine2.namespaces.resolve(Some("proj--docs")).is_some(),
            "'proj--docs' must resolve after recovery");
    }
}

// ── Test 7: legacy WAL recovery (P7 — previously dead code, never called
// from try_recover; a restart under Persistence::Wal silently lost every
// command written since the last snapshot) ─────────────────────────────────

#[test]
fn test_wal_recovery_basic() {
    let dir = tempdir().unwrap();
    let mut cfg = NodeConfig::default();
    cfg.dim = 4;
    cfg.max_records = 64;
    cfg.max_nodes = 64;
    cfg.max_edges = 128;
    cfg.index_kind = IndexKind::BruteForce;
    // WAL-only: no event log, no snapshot — the legacy persistence backend.
    cfg.event_log_path = None;
    cfg.snapshot_path = None;
    cfg.wal_path = Some(dir.path().join("legacy.wal"));

    let pre_crash_hash;
    let n_inserted = 25usize;

    {
        let mut engine = Engine::new(&cfg);
        assert_eq!(engine.try_recover(), RecoveryMode::Fresh, "should start fresh");

        for i in 0..n_inserted {
            let v: Vec<f32> = (0..4).map(|j| (i * 10 + j) as f32 * 0.01).collect();
            engine.insert_record_from_f32(&v).expect("insert failed");
        }
        assert_eq!(engine.record_count(), n_inserted);
        pre_crash_hash = engine.get_proof().final_state_hash;
        // Drop → WalWriter has already flushed on every append_event call.
    }

    {
        let mut engine2 = Engine::new(&cfg);
        let mode = engine2.try_recover();

        assert!(
            matches!(mode, RecoveryMode::Wal(n) if n == n_inserted),
            "expected Wal({n_inserted}) recovery, got {mode:?}"
        );
        assert_eq!(engine2.record_count(), n_inserted, "record count must match after WAL recovery");
        assert_eq!(
            pre_crash_hash,
            engine2.get_proof().final_state_hash,
            "state hash must be identical after legacy WAL recovery"
        );

        // Search index must have been rebuilt too (WAL replay bypasses the
        // normal insert path's incremental index update).
        let hits = engine2.search_l2(&[0.0, 0.01, 0.02, 0.03], 1).unwrap();
        assert!(!hits.is_empty(), "search index must be rebuilt after WAL recovery");
    }
}

#[test]
fn test_snapshot_wins_over_wal_when_both_present() {
    // `save_snapshot()` never truncates or rotates the WAL (unlike
    // `EventLogWriter::rotate`, which splices the chain at a checkpoint),
    // so with both configured the WAL can contain the FULL history,
    // duplicate ids and all, relative to the snapshot. Replaying it after
    // a snapshot restore would hit an immediate duplicate-id rejection on
    // the first pre-snapshot record — so recovery must treat snapshot and
    // WAL as either/or, not layered: snapshot wins outright, WAL is never
    // attempted once the snapshot has already recovered a state.
    let dir = tempdir().unwrap();
    let mut cfg = NodeConfig::default();
    cfg.dim = 4;
    cfg.max_records = 64;
    cfg.max_nodes = 64;
    cfg.max_edges = 128;
    cfg.index_kind = IndexKind::BruteForce;
    cfg.event_log_path = None;
    cfg.snapshot_path = Some(dir.path().join("snapshot.bin"));
    cfg.wal_path = Some(dir.path().join("legacy.wal"));

    let pre_crash_hash;

    {
        let mut engine = Engine::new(&cfg);
        engine.try_recover(); // Fresh

        for i in 0..15u32 {
            let v: Vec<f32> = (0..4).map(|j| (i + j) as f32 * 0.05).collect();
            engine.insert_record_from_f32(&v).unwrap();
        }
        engine.save_snapshot(None).unwrap();

        assert_eq!(engine.record_count(), 15);
        pre_crash_hash = engine.get_proof().final_state_hash;
        // The WAL on disk now has all 15 inserts too (never truncated).
    }

    {
        let mut engine2 = Engine::new(&cfg);
        let mode = engine2.try_recover();

        assert_eq!(
            mode,
            RecoveryMode::Snapshot,
            "snapshot must win outright — replaying the untruncated WAL on top would duplicate-id-reject"
        );
        assert_eq!(engine2.record_count(), 15);
        assert_eq!(
            pre_crash_hash,
            engine2.get_proof().final_state_hash,
            "state hash must match the pre-crash snapshot"
        );
    }
}

// ── Test 5: search index is rebuilt correctly after recovery ──────────────────

#[test]
fn test_search_index_rebuilt_after_recovery() {
    let dir = tempdir().unwrap();
    let cfg = make_cfg(dir.path(), 4);

    // Phase 1: insert known vectors
    {
        let mut engine = Engine::new(&cfg);
        engine.try_recover();
        engine.insert_record_from_f32(&[1.0, 0.0, 0.0, 0.0]).unwrap();
        engine.insert_record_from_f32(&[0.0, 1.0, 0.0, 0.0]).unwrap();
        engine.insert_record_from_f32(&[0.0, 0.0, 1.0, 0.0]).unwrap();
    }

    // Phase 2: recover and search
    {
        let mut engine2 = Engine::new(&cfg);
        let mode = engine2.try_recover();
        assert!(matches!(mode, RecoveryMode::EventLog(3)), "expected EventLog(3), got {:?}", mode);

        let hits = engine2.search_l2(&[1.0, 0.0, 0.0, 0.0], 1).unwrap();
        assert!(!hits.is_empty(), "search index must be rebuilt after recovery");
        assert_eq!(hits[0].0, 0, "nearest neighbor of [1,0,0,0] must be record 0");
    }
}
