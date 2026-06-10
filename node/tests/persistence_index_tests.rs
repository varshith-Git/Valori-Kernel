// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use valori_node::config::{NodeConfig, IndexKind, QuantizationKind};
use valori_node::engine::Engine;
use tempfile::tempdir;

const DIM: usize = 16;
const N: usize = 200;

fn ivf_cfg(dir: &std::path::Path) -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.dim = DIM;
    cfg.max_records = N;
    cfg.max_nodes = N;
    cfg.max_edges = N;
    cfg.index_kind = IndexKind::Ivf;
    cfg.quantization_kind = QuantizationKind::None;
    cfg.snapshot_path = Some(dir.join("ivf_snap.bin"));
    cfg
}

#[test]
fn test_ivf_persistence() {
    let dir = tempdir().unwrap();
    let cfg = ivf_cfg(dir.path());
    let snap_path = cfg.snapshot_path.clone().unwrap();

    // ── 1. Build ──────────────────────────────────────────────────────────────
    {
        let mut engine = Engine::new(&cfg);

        for i in 0..100usize {
            let val = i as f32 / 100.0;
            let mut vec = vec![0.0f32; DIM];
            vec[0] = val;
            engine.insert_record_from_f32(&vec).unwrap();
        }

        engine.save_snapshot(None).unwrap();
    }

    // ── 2. Restore and verify ─────────────────────────────────────────────────
    {
        let mut engine = Engine::new(&cfg);
        let data = std::fs::read(&snap_path).expect("Snapshot file missing");
        engine.restore(&data).expect("Restore failed");

        let mut q = vec![0.0f32; DIM];
        q[0] = 0.5;
        let hits = engine.search_l2(&q, 5).unwrap();
        assert!(!hits.is_empty());
        // Record 50 has vec[0] = 0.5 — should be nearest to the query.
        assert_eq!(hits[0].0, 50, "IVF restore: record 50 should be nearest to 0.5");
    }
}
