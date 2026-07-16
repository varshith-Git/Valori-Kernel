// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! End-to-end proof and hash determinism tests.
//!
//! What the unit tests in src/snapshot/blake3.rs do NOT cover:
//!   - hash_state_blake3 over a non-empty kernel (real inserts)
//!   - state hash survives crash → event-log recovery round-trip
//!   - generate_proof_bytes: same input → same bytes, different input → different bytes
//!   - identical operations on two separate engines → identical state hash
//!   - snapshot → restore preserves state hash

use tempfile::tempdir;
use valori_node::config::NodeConfig;
use valori_node::EngineFromNodeConfig;
use valori_node::engine::Engine;
use valori_node::events::event_log::EventLogWriter;
use valori_node::events::event_journal::EventJournal;
use valori_node::events::event_commit::EventCommitter;
use valori_node::events::event_replay::recover_from_event_log;
use valori_kernel::proof::generate_proof_bytes;
use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::vector::FxpVector;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::state::kernel::KernelState;

const DIM: usize = 4;

fn make_cfg(dir: &std::path::Path) -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.dim = DIM;
    cfg.max_records = 64;
    cfg.event_log_path = Some(dir.join("events.log"));
    cfg
}

fn make_fxp_vec(values: [f32; DIM]) -> FxpVector {
    FxpVector {
        data: values.iter().map(|&f| {
            // Q16.16 conversion — same as engine.rs
            FxpScalar((f * 65536.0) as i32)
        }).collect(),
    }
}

fn make_insert_event(id: u32, values: [f32; DIM]) -> KernelEvent {
    KernelEvent::InsertRecord {
        id: RecordId(id),
        vector: make_fxp_vec(values),
        metadata: None,
        tag: 0,
    }
}

// ── Test 1: generate_proof_bytes is deterministic and sensitive ───────────────

#[test]
fn test_proof_bytes_deterministic_and_sensitive() {
    let values: Vec<i32> = vec![
        (0.1f32 * 65536.0) as i32,
        (0.5f32 * 65536.0) as i32,
        (-0.3f32 * 65536.0) as i32,
        (1.0f32 * 65536.0) as i32,
    ];

    let proof_a = generate_proof_bytes(&values);
    let proof_b = generate_proof_bytes(&values);
    assert_eq!(proof_a, proof_b, "Proof must be deterministic for same input");
    assert!(!proof_a.is_empty(), "Proof must not be empty");

    let mut different = values.clone();
    different[0] += 1;
    let proof_other = generate_proof_bytes(&different);
    assert_ne!(proof_a, proof_other, "Different inputs must produce different proofs");
}

// ── Test 2: hash_state_blake3 changes on real inserts ────────────────────────

#[test]
fn test_hash_changes_on_insert() {
    let dir = tempdir().unwrap();
    let cfg = make_cfg(dir.path());
    let mut engine = Engine::new(&cfg);

    let hash_empty = engine.get_proof().final_state_hash;

    engine.insert_record_from_f32(&[0.1, 0.2, 0.3, 0.4]).unwrap();
    let hash_1 = engine.get_proof().final_state_hash;
    assert_ne!(hash_empty, hash_1, "Hash must change after first insert");

    engine.insert_record_from_f32(&[0.5, 0.6, 0.7, 0.8]).unwrap();
    let hash_2 = engine.get_proof().final_state_hash;
    assert_ne!(hash_1, hash_2, "Hash must change after second insert");
}

// ── Test 3: identical operations → identical hash (cross-run determinism) ────

#[test]
fn test_hash_deterministic_across_runs() {
    fn build_hash() -> [u8; 32] {
        let dir = tempdir().unwrap();
        let cfg = {
            let mut c = NodeConfig::default();
            c.dim = DIM;
            c.max_records = 64;
            c.event_log_path = Some(dir.path().join("events.log"));
            c
        };
        let mut engine = Engine::new(&cfg);
        for i in 0..5u32 {
            let v = i as f32 / 10.0;
            engine.insert_record_from_f32(&[v, v + 0.1, v + 0.2, v + 0.3]).unwrap();
        }
        engine.get_proof().final_state_hash
    }

    assert_eq!(build_hash(), build_hash(), "Same operations must produce identical state hash");
}

// ── Test 4: crash → event-log recovery → hash matches pre-crash hash ─────────

#[test]
fn test_event_log_recovery_preserves_hash() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("events.log");

    // Phase 1: write events via committer and record hash.
    let pre_crash_hash = {
        let log_writer = EventLogWriter::open(&log_path, Some(DIM as u32)).unwrap();
        let journal = EventJournal::new();
        let live = KernelState::new();
        let mut committer = EventCommitter::new(log_writer, journal, live);

        for i in 0..4u32 {
            let v = i as f32 / 5.0;
            committer.commit_event(make_insert_event(i, [v, v, v, v])).unwrap();
        }

        hash_state_blake3(committer.live_state())
        // committer dropped here — flush before "crash"
    };

    // Phase 2: cold recovery from event log only (no snapshot).
    let (recovered_state, _journal, _count) = recover_from_event_log(&log_path).unwrap();
    let post_recovery_hash = hash_state_blake3(&recovered_state);

    assert_eq!(
        pre_crash_hash, post_recovery_hash,
        "State hash must be identical after event-log recovery"
    );
}

// ── Test 5: snapshot → restore preserves hash ─────────────────────────────────

#[test]
fn test_snapshot_round_trip_preserves_hash() {
    let dir = tempdir().unwrap();
    let cfg = make_cfg(dir.path());
    let mut engine = Engine::new(&cfg);

    for i in 0..4u32 {
        let v = i as f32 / 10.0;
        engine.insert_record_from_f32(&[v, v + 0.05, v + 0.1, v + 0.15]).unwrap();
    }

    let hash_before = engine.get_proof().final_state_hash;
    let snap = engine.snapshot().unwrap();

    let dir2 = tempdir().unwrap();
    let cfg2 = make_cfg(dir2.path());
    let mut engine2 = Engine::new(&cfg2);
    engine2.restore(&snap).unwrap();
    let hash_after = engine2.get_proof().final_state_hash;

    assert_eq!(hash_before, hash_after, "State hash must survive snapshot → restore");
}
