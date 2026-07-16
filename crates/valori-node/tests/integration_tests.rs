// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use valori_node::EngineFromNodeConfig;

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

    let id1 = engine
        .insert_record_from_f32(&[1.0, 0.0, 0.0, 0.0])
        .unwrap();
    let id2 = engine
        .insert_record_from_f32(&[0.0, 1.0, 0.0, 0.0])
        .unwrap();

    assert_eq!(id1, 0);
    assert_eq!(id2, 1);

    let results = engine.search_l2(&[1.0, 0.0, 0.0, 0.0], 2).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, 0); // exact match first

    let nid1 = engine.create_node_for_record(Some(id1), 0, 0).unwrap();
    let nid2 = engine.create_node_for_record(Some(id2), 0, 0).unwrap();
    let eid = engine.create_edge(nid1, nid2, 0).unwrap();
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
    assert!(engine
        .insert_record_from_f32(&[100.0, -100.0, 32000.0, -32000.0])
        .is_ok());

    // Values outside the Q16.16 range must be rejected.
    assert!(engine
        .insert_record_from_f32(&[1.0, 1.0, 33000.0, 1.0])
        .is_err());
    assert!(engine
        .insert_record_from_f32(&[1.0, -33000.0, 1.0, 1.0])
        .is_err());

    // Out-of-range search query must also be rejected.
    assert!(engine.search_l2(&[1.0, 1.0, 33000.0, 1.0], 2).is_err());
}

// Regression test: VALORI_DIM must be enforced from the first insert.
// Before the fix, a node started with dim=4 would silently accept a 6-element
// vector as the first insert and lock to dim=6, rejecting all subsequent
// correctly-sized vectors.
#[test]
fn test_dim_enforced_from_config() {
    let cfg = make_cfg(4);
    let mut engine = Engine::new(&cfg);

    // A wrong-size vector on the very first insert must be rejected.
    let result = engine.insert_record_from_f32(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    assert!(
        result.is_err(),
        "6-element vector must be rejected when VALORI_DIM=4, got: {result:?}"
    );

    // A correctly-sized vector must be accepted.
    assert!(
        engine.insert_record_from_f32(&[1.0, 0.0, 0.0, 0.0]).is_ok(),
        "4-element vector must be accepted when VALORI_DIM=4"
    );

    // And a second wrong-size vector after a correct insert must also be rejected.
    assert!(
        engine
            .insert_record_from_f32(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0])
            .is_err(),
        "6-element vector must still be rejected after a valid insert"
    );
}

#[test]
fn test_search_dim_validated() {
    let cfg = make_cfg(4);
    let mut engine = Engine::new(&cfg);

    // Insert a valid record to lock the dim.
    engine
        .insert_record_from_f32(&[1.0, 0.0, 0.0, 0.0])
        .unwrap();

    // A wrong-dim query must be rejected.
    assert!(
        engine.search_l2(&[1.0, 0.0], 5).is_err(),
        "2-element query against dim=4 store must return an error, not results"
    );

    // A query with one extra element must also be rejected.
    assert!(
        engine.search_l2(&[1.0, 0.0, 0.0, 0.0, 0.0], 5).is_err(),
        "5-element query against dim=4 store must return an error"
    );

    // Correct-dim query must succeed.
    assert!(
        engine.search_l2(&[1.0, 0.0, 0.0, 0.0], 5).is_ok(),
        "4-element query against dim=4 store must succeed"
    );

    // Same checks via the namespace-scoped path.
    assert!(
        engine.search_l2_ns(&[1.0, 0.0], 5, 0).is_err(),
        "2-element ns-query against dim=4 store must return an error"
    );
    assert!(
        engine.search_l2_ns(&[1.0, 0.0, 0.0, 0.0], 5, 0).is_ok(),
        "4-element ns-query against dim=4 store must succeed"
    );
}
