// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! End-to-end proof / determinism tests.
//!
//! Verifies that:
//!   1. `get_proof().final_state_hash` round-trips through snapshot / restore.
//!   2. `verify_embedding()` returns the correct record after insert.
//!   3. Two engines that apply the same sequence of operations produce an
//!      identical BLAKE3 state hash (full determinism across restarts).

use valori_node::config::{NodeConfig, IndexKind, QuantizationKind};
use valori_node::EngineFromNodeConfig;
use valori_node::engine::{Engine, RecoveryMode};
use tempfile::tempdir;

fn make_cfg_no_log(dim: usize) -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.dim = dim;
    cfg.max_records = 256;
    cfg.max_nodes = 256;
    cfg.max_edges = 512;
    cfg.index_kind = IndexKind::BruteForce;
    cfg.quantization_kind = QuantizationKind::None;
    cfg.event_log_path = None;
    cfg.wal_path = None;
    cfg.snapshot_path = None;
    cfg
}

fn make_cfg_with_event_log(dir: &std::path::Path, dim: usize) -> NodeConfig {
    let mut cfg = make_cfg_no_log(dim);
    cfg.event_log_path = Some(dir.join("events.log"));
    cfg.snapshot_path = Some(dir.join("snapshot.bin"));
    cfg
}

// ── Test 1: hash is stable through snapshot → restore ────────────────────────
//
// insert records → get_proof() → save_snapshot → fresh engine → restore →
//   get_proof() must match.

#[test]
fn test_state_hash_round_trips_through_snapshot() {
    let dir = tempdir().unwrap();
    let mut cfg = make_cfg_no_log(4);
    cfg.snapshot_path = Some(dir.path().join("snap.bin"));

    let mut engine = Engine::new(&cfg);

    for i in 0..30u32 {
        let v: Vec<f32> = (0..4).map(|j| (i * 4 + j as u32) as f32 * 0.01).collect();
        engine.insert_record_from_f32(&v).expect("insert");
    }

    let hash_before = engine.get_proof().final_state_hash;
    let snap_bytes = engine.snapshot().expect("snapshot");

    // Restore into a fresh engine.
    let mut engine2 = Engine::new(&cfg);
    engine2.restore(&snap_bytes).expect("restore");

    let hash_after = engine2.get_proof().final_state_hash;

    assert_eq!(
        hash_before, hash_after,
        "BLAKE3 state hash must be identical after snapshot round-trip"
    );
    assert_eq!(engine2.record_count(), 30, "record count must be preserved");
}

// ── Test 2: verify_embedding round-trip ──────────────────────────────────────
//
// Insert a record with a known vector, then search for it.
// The nearest neighbour at k=1 must be that exact record.

#[test]
fn test_verify_embedding_round_trip() {
    let mut engine = Engine::new(&make_cfg_no_log(4));

    let target = vec![0.1f32, 0.2, 0.3, 0.4];
    let rid = engine.insert_record_from_f32(&target).expect("insert");

    // Insert decoy records that are far from the target.
    for i in 1..10u32 {
        let v: Vec<f32> = (0..4).map(|j| (i * 100 + j as u32) as f32).collect();
        engine.insert_record_from_f32(&v).expect("insert decoy");
    }

    let results = engine.search_l2(&target, 1).expect("search");
    assert!(!results.is_empty(), "search must return at least one result");
    assert_eq!(
        results[0].0, rid,
        "nearest neighbour of the target vector must be the target record itself"
    );
    // Distance to itself should be zero (or near-zero due to fixed-point rounding).
    assert!(
        results[0].1 < 1e-3,
        "distance to own vector must be near-zero, got {}",
        results[0].1
    );
}

// ── Test 3: identical operation sequences produce identical hashes ─────────────
//
// Create two engines in separate temp dirs.  Apply the same 50 insert +
// 5 soft-delete operations in the same order.  Final BLAKE3 hashes must match.

#[test]
fn test_determinism_across_two_independent_engines() {
    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();

    let vectors: Vec<Vec<f32>> = (0..50)
        .map(|i| (0..4).map(|j| (i * 4 + j) as f32 * 0.007).collect())
        .collect();

    let mut hash_a = [0u8; 32];
    let mut hash_b = [0u8; 32];

    for (dir, hash_out) in [(&dir_a, &mut hash_a), (&dir_b, &mut hash_b)] {
        let cfg = make_cfg_with_event_log(dir.path(), 4);
        let mut engine = Engine::new(&cfg);
        engine.try_recover();

        for v in &vectors {
            engine.insert_record_from_f32(v).expect("insert");
        }
        // Soft-delete records 5, 10, 15, 20, 25 — same set in both engines.
        for id in [5u32, 10, 15, 20, 25] {
            engine.soft_delete_record(id).expect("soft delete");
        }

        *hash_out = engine.get_proof().final_state_hash;
    }

    assert_eq!(
        hash_a, hash_b,
        "two engines applying identical operations must produce the same BLAKE3 hash"
    );
}

// ── Test 4: hash changes on every mutation ────────────────────────────────────
//
// Each insert must produce a different hash (otherwise the hash isn't tracking
// mutations).

#[test]
fn test_hash_changes_on_every_insert() {
    let mut engine = Engine::new(&make_cfg_no_log(4));
    let mut seen = std::collections::HashSet::new();

    for i in 0..20u32 {
        let v: Vec<f32> = (0..4).map(|j| (i * 4 + j as u32) as f32 * 0.1).collect();
        engine.insert_record_from_f32(&v).expect("insert");

        let hash = engine.get_proof().final_state_hash;
        assert!(
            seen.insert(hash),
            "hash after insert {} must differ from all prior hashes", i
        );
    }
}

// ── Test 5: hash is stable through event-log recovery ─────────────────────────
//
// This complements e2e_recovery::test_event_log_recovery_basic but focuses
// explicitly on the proof / hash contract.

#[test]
fn test_proof_hash_stable_through_event_log_recovery() {
    let dir = tempdir().unwrap();
    let cfg = make_cfg_with_event_log(dir.path(), 4);

    let pre_crash_hash;

    {
        let mut engine = Engine::new(&cfg);
        assert_eq!(engine.try_recover(), RecoveryMode::Fresh);

        for i in 0..40u32 {
            let v: Vec<f32> = (0..4).map(|j| (i + j as u32) as f32 * 0.03).collect();
            engine.insert_record_from_f32(&v).expect("insert");
        }

        pre_crash_hash = engine.get_proof().final_state_hash;
        // Drop → flush event log to disk.
    }

    {
        let mut engine2 = Engine::new(&cfg);
        let mode = engine2.try_recover();

        assert!(
            matches!(mode, RecoveryMode::EventLog(40)),
            "expected EventLog(40), got {:?}", mode
        );

        let post_hash = engine2.get_proof().final_state_hash;
        assert_eq!(
            pre_crash_hash, post_hash,
            "proof hash must be identical after event-log recovery"
        );
    }
}
