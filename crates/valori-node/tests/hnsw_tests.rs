// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use valori_node::config::{IndexKind, NodeConfig, QuantizationKind};
use valori_node::engine::Engine;
use valori_node::EngineFromNodeConfig;

const DIM: usize = 4;

fn hnsw_cfg() -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.max_records = 100;
    cfg.dim = DIM;
    cfg.max_nodes = 100;
    cfg.max_edges = 500;
    cfg.index_kind = IndexKind::Hnsw;
    cfg.quantization_kind = QuantizationKind::None;
    cfg
}

#[test]
fn test_hnsw_determinism() {
    let cfg = hnsw_cfg();

    let mut engine1 = Engine::new(&cfg);
    engine1.insert_record_from_f32(&vec![0.1; DIM]).unwrap();
    engine1.insert_record_from_f32(&vec![0.2; DIM]).unwrap();
    engine1.insert_record_from_f32(&vec![0.3; DIM]).unwrap();

    let snap1 = engine1.snapshot().unwrap();

    let mut engine2 = Engine::new(&cfg);
    engine2.restore(&snap1).unwrap();

    let query = vec![0.21; DIM];
    let hits1 = engine1.search_l2(&query, 3).unwrap();
    let hits2 = engine2.search_l2(&query, 3).unwrap();

    assert_eq!(
        hits1, hits2,
        "Search results must match between original and restored HNSW engine"
    );
    println!("HNSW hits: {:?}", hits1);
}
