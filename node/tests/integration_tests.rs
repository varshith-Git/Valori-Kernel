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
    let cfg = NodeConfig::default();
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
