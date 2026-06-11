// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use tempfile::tempdir;

fn make_cfg(dir: &std::path::Path) -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.dim = 4;
    cfg.max_records = 100;
    cfg.max_nodes = 100;
    cfg.max_edges = 500;
    cfg.index_kind = valori_node::config::IndexKind::Hnsw;
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
        let id = engine.insert_record_from_f32(&[0.1, 0.2, 0.3, 0.4]).unwrap();
        assert_eq!(id, 0);

        let results = engine.search_l2(&[0.1, 0.2, 0.3, 0.4], 1).unwrap();
        assert_eq!(results[0].0, 0);

        engine.save_snapshot(Some(&snap_path)).expect("Snapshot failed");
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
