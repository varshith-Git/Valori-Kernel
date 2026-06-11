// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use valori_node::config::NodeConfig;
use valori_node::engine::Engine;

fn make_cfg(dim: usize) -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.dim = dim;
    cfg.max_records = 16;
    cfg.max_nodes = 16;
    cfg.max_edges = 32;
    cfg
}

#[test]
fn test_engine_workflow() {
    let cfg = make_cfg(4);
    let mut engine = Engine::new(&cfg);

    let id1 = engine.insert_record_from_f32(&[1.0, 0.0, 0.0, 0.0]).unwrap();
    let id2 = engine.insert_record_from_f32(&[0.0, 1.0, 0.0, 0.0]).unwrap();

    assert_eq!(id1, 0);
    assert_eq!(id2, 1);

    let results = engine.search_l2(&[1.0, 0.0, 0.0, 0.0], 2).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, 0); // exact match first

    let nid1 = engine.create_node_for_record(Some(id1), 0).unwrap();
    let nid2 = engine.create_node_for_record(Some(id2), 0).unwrap();
    let eid  = engine.create_edge(nid1, nid2, 0).unwrap();
    assert_eq!(eid, 0);

    let snapshot = engine.snapshot().unwrap();
    assert!(!snapshot.is_empty());

    let mut engine2 = Engine::new(&cfg);
    engine2.restore(&snapshot).unwrap();

    let results2 = engine2.search_l2(&[1.0, 0.0, 0.0, 0.0], 2).unwrap();
    assert_eq!(results, results2);
}

#[test]
fn test_input_bounds_validation() {
    let cfg = make_cfg(4);
    let mut engine = Engine::new(&cfg);

    // Values within the Q16.16 safe range are accepted.
    assert!(engine.insert_record_from_f32(&[100.0, -100.0, 32000.0, -32000.0]).is_ok());

    // Values outside the Q16.16 range must be rejected.
    assert!(engine.insert_record_from_f32(&[1.0, 1.0, 33000.0, 1.0]).is_err());
    assert!(engine.insert_record_from_f32(&[1.0, -33000.0, 1.0, 1.0]).is_err());

    // Out-of-range search query must also be rejected.
    assert!(engine.search_l2(&[1.0, 1.0, 33000.0, 1.0], 2).is_err());
}
