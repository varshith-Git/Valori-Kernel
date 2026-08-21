// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use tempfile::tempdir;
use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use valori_node::EngineFromNodeConfig;

fn make_cfg(dir: &std::path::Path) -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.max_records = 100;
    cfg.max_nodes = 100;
    cfg.max_edges = 500;
    cfg.snapshot_path = Some(dir.join("snapshot.bin"));
    cfg
}

#[tokio::test]
async fn test_index_persistence() {
    let dir = tempdir().unwrap();
    let cfg = make_cfg(dir.path());
    let snap_path = cfg.snapshot_path.clone().unwrap();

    // ── 1. Insert and save ────────────────────────────────────────────────────
    {
        let mut engine = Engine::new(&cfg);
        let id = engine
            .insert_record_from_f32(&[0.1, 0.2, 0.3, 0.4])
            .unwrap();
        assert_eq!(id, 0);

        let results = engine.search_l2(&[0.1, 0.2, 0.3, 0.4], 1).unwrap();
        assert_eq!(results[0].0, 0);

        engine
            .save_snapshot(Some(&snap_path))
            .expect("Snapshot failed");
        assert!(snap_path.exists());
    }

    // ── 2. Restore and verify search works without re-inserting ───────────────
    {
        let mut engine2 = Engine::new(&cfg);
        let data = std::fs::read(&snap_path).unwrap();
        engine2.restore(&data).expect("Restore failed");

        let results = engine2.search_l2(&[0.1, 0.2, 0.3, 0.4], 1).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 0);
    }

    // ── 3. Truncated snapshot must return an error ─────────────────────────────
    {
        let mut data = std::fs::read(&snap_path).unwrap();
        data.truncate(data.len() / 2);

        let mut engine3 = Engine::new(&cfg);
        let res = engine3.restore(&data);
        assert!(res.is_err(), "Truncated snapshot must be rejected");
        println!("Truncation check passed: {:?}", res.err());
    }
}

/// S8 regression (Local Cloud E2E phase): `create_collection()` used to
/// mutate `KernelState` (bumping the hashed `version` counter) via a
/// direct `state.apply_event_ns()` call that bypassed the durability
/// layer entirely — so a namespace created right before a restart made
/// the state hash unrecoverable by replay, even though every visible
/// record was byte-identical. Fixed by routing through
/// `commit_and_apply_ns()` (like every other mutation) plus an explicit
/// `flush_pending_events()` before "restart" here — mirrors what
/// `main.rs::shutdown_signal` now does for real, since a single-event
/// commit like this one only flushes its write buffer on demand
/// (`DEFAULT_WRITE_BUFFER_SIZE = 64`), unlike a batch insert, which
/// always flushes unconditionally.
#[tokio::test]
async fn test_state_hash_survives_restart_after_collection_create() {
    use valori_kernel::snapshot::blake3::hash_state_blake3;

    let dir = tempdir().unwrap();
    let mut cfg = NodeConfig::default();
    cfg.max_records = 100;
    cfg.event_log_path = Some(dir.path().join("events.log"));

    let live_hash = {
        let mut engine = Engine::new(&cfg);
        let ns = engine
            .create_collection_with_config(
                "s8-check",
                4,
                valori_domain::Metric::SquaredL2,
                valori_domain::IndexKind::Brute,
            )
            .unwrap();
        engine
            .insert_batch_ns(
                &[
                    vec![1.0, 0.0, 0.0, 0.0],
                    vec![0.0, 1.0, 0.0, 0.0],
                    vec![0.5, 0.5, 0.0, 0.0],
                ],
                None,
                ns,
                None,
            )
            .unwrap();
        let hash = hash_state_blake3(&engine.state);
        // Mirrors main.rs's shutdown_signal: flush pending single-event
        // commits before "restart" (dropping this engine / replaying).
        engine.flush_pending_events().unwrap();
        hash
    };

    let (replayed_state, _journal, count) =
        valori_storage::events::event_replay::recover_from_event_log(
            cfg.event_log_path.as_ref().unwrap(),
        )
        .unwrap();
    assert_eq!(
        count, 5,
        "expected AutoCreateNamespace + ConfigureNamespace + 3 InsertRecord events to be durably logged"
    );

    let replay_hash = hash_state_blake3(&replayed_state);
    assert_eq!(
        live_hash, replay_hash,
        "state hash must be reproduced identically by event-log replay after a collection was created"
    );
}
