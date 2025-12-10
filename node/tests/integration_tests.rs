use valori_node::config::NodeConfig;
use valori_node::engine::Engine;

// We need concrete types for Engine
const MAX_RECORDS: usize = 16;
const D: usize = 4;
const MAX_NODES: usize = 16;
const MAX_EDGES: usize = 32;

type TestEngine = Engine<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>;

#[test]
fn test_engine_workflow() {
    let mut cfg = NodeConfig::default();
    cfg.max_records = MAX_RECORDS;
    cfg.dim = D;
    cfg.max_nodes = MAX_NODES;
    cfg.max_edges = MAX_EDGES;
    let mut engine = TestEngine::new(&cfg);

    // 1. Insert records
    let v1 = [1.0, 0.0, 0.0, 0.0];
    let v2 = [0.0, 1.0, 0.0, 0.0];
    
    let id1 = engine.insert_record_from_f32(&v1).unwrap();
    let id2 = engine.insert_record_from_f32(&v2).unwrap();
    
    assert_eq!(id1, 0);
    assert_eq!(id2, 1);
    
    // 2. Search
    let query = [1.0, 0.0, 0.0, 0.0];
    let results = engine.search_l2(&query, 2).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, 0); // Exact match first
    
    // 3. Graph
    let nid1 = engine.create_node_for_record(Some(id1), 0).unwrap(); // 0 = NodeKind::Record
    let nid2 = engine.create_node_for_record(Some(id2), 0).unwrap();
    
    let eid = engine.create_edge(nid1, nid2, 0).unwrap(); // 0 = EdgeKind::Relation
    
    assert_eq!(eid, 0);
    
    // 4. Snapshot
    let snapshot = engine.snapshot().unwrap();
    assert!(snapshot.len() > 0);
    
    // 5. Restore
    let mut engine2 = TestEngine::new(&cfg);
    engine2.restore(&snapshot).unwrap();
    
    // Check search on restored engine
    let results2 = engine2.search_l2(&query, 2).unwrap();
    assert_eq!(results, results2);
}

#[test]
fn test_input_bounds_validation() {
    let mut cfg = NodeConfig::default();
    cfg.max_records = MAX_RECORDS;
    cfg.dim = D;
    cfg.max_nodes = MAX_NODES;
    cfg.max_edges = MAX_EDGES;
    let mut engine = TestEngine::new(&cfg);

    // Safe Vector
    let safe_vec = [100.0, -100.0, 32000.0, -32000.0];
    assert!(engine.insert_record_from_f32(&safe_vec).is_ok());

    // Unsafe Positive
    let unsafe_pos = [1.0, 1.0, 33000.0, 1.0];
    let err = engine.insert_record_from_f32(&unsafe_pos);
    assert!(err.is_err());
    println!("Caught expected error: {:?}", err);

    // Unsafe Negative
    let unsafe_neg = [1.0, -33000.0, 1.0, 1.0];
    assert!(engine.insert_record_from_f32(&unsafe_neg).is_err());
    
    // Unsafe Search Query
    let err_search = engine.search_l2(&unsafe_pos, 2);
    assert!(err_search.is_err());
}
