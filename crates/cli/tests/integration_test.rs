// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! CLI integration tests.
//!
//! All fixtures are created inline using the real kernel and event-log APIs so
//! these tests exercise exactly the same binary formats that a live Valori
//! database produces.

use std::path::{Path, PathBuf};
use tempfile::tempdir;
use valori_cli::commands::{diff, inspect, replay_query, timeline, verify};
use valori_cli::engine::ForensicEngine;

// ─── Fixture helpers ──────────────────────────────────────────────────────────

struct TestPaths {
    pub snapshot: PathBuf,
    pub log:      PathBuf,
}

/// Create a minimal database: 3 records + snapshot, then 3 more events in log.
fn build_test_db(dir: &Path) -> anyhow::Result<TestPaths> {
    use valori_kernel::event::KernelEvent;
    use valori_kernel::snapshot::encode::encode_state;
    use valori_kernel::state::kernel::KernelState;
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::vector::FxpVector;
    use valori_node::events::event_log::{EventLogWriter, LogEntry};

    const DIM: usize = 4;

    // ── 1. Build an initial kernel state (3 records) ─────────────────────────
    let mut state = KernelState::new();

    for i in 0u32..3 {
        let evt = KernelEvent::InsertRecord {
            id:       RecordId(i),
            vector:   FxpVector::new_zeros(DIM),
            metadata: None,
            tag:      0,
        };
        state.apply_event(&evt).expect("apply insert");
    }

    // ── 2. Write snapshot (VAL1 format) ──────────────────────────────────────
    let mut k_buf = vec![0u8; 65_536];
    let k_len     = encode_state(&state, &mut k_buf).expect("encode state");
    k_buf.truncate(k_len);

    let mut snap = Vec::new();
    snap.extend_from_slice(b"VAL1");
    snap.extend_from_slice(&(k_len as u32).to_le_bytes());
    snap.extend_from_slice(&k_buf);
    snap.extend_from_slice(&0u32.to_le_bytes()); // metadata section (empty)
    snap.extend_from_slice(&0u32.to_le_bytes()); // index section   (empty)

    let snap_path = dir.join("snapshot.val");
    std::fs::write(&snap_path, &snap)?;

    // ── 3. Write post-snapshot events to the event log ────────────────────────
    let log_path = dir.join("events.log");
    let mut writer = EventLogWriter::open(&log_path, Some(DIM as u32))?;

    for i in 3u32..6 {
        let evt = KernelEvent::InsertRecord {
            id:       RecordId(i),
            vector:   FxpVector::new_zeros(DIM),
            metadata: None,
            tag:      i as u64,
        };
        writer.append(&LogEntry::Event(evt))?;
    }
    // Flush by dropping.
    drop(writer);

    Ok(TestPaths { snapshot: snap_path, log: log_path })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn test_inspect_finds_both_files() {
    let dir   = tempdir().unwrap();
    let paths = build_test_db(dir.path()).unwrap();
    let _ = paths; // keep alive

    let result = inspect::run(
        Some(dir.path().to_path_buf()),
        None,
        None,
    );
    assert!(result.is_ok(), "inspect should succeed: {result:?}");
}

#[test]
fn test_verify_passes_on_valid_snapshot() {
    let dir   = tempdir().unwrap();
    let paths = build_test_db(dir.path()).unwrap();

    let result = verify::run(paths.snapshot.to_str().unwrap());
    assert!(result.is_ok(), "verify should pass on a valid snapshot: {result:?}");
}

#[test]
fn test_timeline_reads_all_events() {
    let dir   = tempdir().unwrap();
    let paths = build_test_db(dir.path()).unwrap();

    let result = timeline::run(paths.log.to_str().unwrap(), 0 /* no limit */);
    assert!(result.is_ok(), "timeline should parse the event log: {result:?}");
}

#[test]
fn test_verify_rejects_corrupt_snapshot() {
    let dir      = tempdir().unwrap();
    let bad_path = dir.path().join("bad.val");
    std::fs::write(&bad_path, b"JUNK0000this is not a valid snapshot").unwrap();

    let result = verify::run(bad_path.to_str().unwrap());
    assert!(result.is_err(), "verify should reject a corrupt snapshot");
}

#[test]
fn test_replay_to_advances_state() {
    let dir   = tempdir().unwrap();
    let paths = build_test_db(dir.path()).unwrap();

    // Snapshot has 3 records; log has 3 more events (IDs 3, 4, 5).
    let result = replay_query::run(
        paths.snapshot.to_str().unwrap(),
        paths.log.to_str().unwrap(),
        2,    // replay 2 events
        None,
        5,
    );
    assert!(result.is_ok(), "replay-query should succeed: {result:?}");
}

#[test]
fn test_replay_beyond_log_end_is_graceful() {
    let dir   = tempdir().unwrap();
    let paths = build_test_db(dir.path()).unwrap();

    // Request event #99 but only 3 exist in the log.
    let result = replay_query::run(
        paths.snapshot.to_str().unwrap(),
        paths.log.to_str().unwrap(),
        99,
        None,
        5,
    );
    assert!(result.is_ok(), "replay-query beyond log end should warn, not error");
}

#[test]
fn test_diff_identical_positions_shows_no_drift() {
    let dir   = tempdir().unwrap();
    let paths = build_test_db(dir.path()).unwrap();

    let result = diff::run(
        paths.snapshot.to_str().unwrap(),
        paths.log.to_str().unwrap(),
        2,
        2,
        None,
        5,
    );
    assert!(result.is_ok(), "diff at identical positions: {result:?}");
}

#[test]
fn test_diff_forward_detects_new_events() {
    let dir   = tempdir().unwrap();
    let paths = build_test_db(dir.path()).unwrap();

    let result = diff::run(
        paths.snapshot.to_str().unwrap(),
        paths.log.to_str().unwrap(),
        1,
        3,
        None,
        5,
    );
    assert!(result.is_ok(), "diff forward: {result:?}");
}

#[test]
fn test_forensic_engine_state_changes_after_replay() {
    let dir   = tempdir().unwrap();
    let paths = build_test_db(dir.path()).unwrap();

    let mut engine = ForensicEngine::from_snapshot(paths.snapshot.to_str().unwrap()).unwrap();
    assert_eq!(engine.state.record_count(), 3, "snapshot should contain 3 records");

    let hash_before = engine.blake3_hex();

    engine
        .replay_to(paths.log.to_str().unwrap(), 3)
        .unwrap();

    assert_eq!(
        engine.state.record_count(), 6,
        "after replaying 3 events, should have 6 records total"
    );
    assert_ne!(
        engine.blake3_hex(),
        hash_before,
        "state hash must change after replay"
    );
}
