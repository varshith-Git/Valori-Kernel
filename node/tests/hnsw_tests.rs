use valori_node::config::{NodeConfig, IndexKind, QuantizationKind};
// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use valori_node::engine::Engine;
use std::sync::Arc;
use std::time::Duration;

const MAX_RECORDS: usize = 100;
const D: usize = 4;
const MAX_NODES: usize = 100;
const MAX_EDGES: usize = 500;

#[tokio::test]
async fn test_hnsw_determinism() {
    let mut cfg = NodeConfig::default();
    cfg.max_records = MAX_RECORDS;
    cfg.dim = D;
    cfg.max_nodes = MAX_NODES;
    cfg.max_edges = MAX_EDGES;
    cfg.index_kind = IndexKind::Hnsw; // Explicitly test HNSW
    cfg.quantization_kind = QuantizationKind::None;
    
    // Create Engine 1
    let mut engine1: Engine<MAX_RECORDS, D, MAX_NODES, MAX_EDGES> = Engine::new(&cfg);
    
    // Insert vectors [A, B, C]
    let vec_a = vec![0.1; D];
    let vec_b = vec![0.2; D];
    let vec_c = vec![0.3; D];
    
    engine1.insert_record_from_f32(&vec_a).unwrap();
    engine1.insert_record_from_f32(&vec_b).unwrap();
    engine1.insert_record_from_f32(&vec_c).unwrap();
    
    // Snapshot
    let snap1 = engine1.snapshot().unwrap();
    
    // Create Engine 2 from Snapshot
    let mut engine2: Engine<MAX_RECORDS, D, MAX_NODES, MAX_EDGES> = Engine::new(&cfg);
    engine2.restore(&snap1).unwrap();
    
    // Verify Search Results match
    let query = vec![0.21; D]; // Close to B
    let hits1 = engine1.search_l2(&query, 3).unwrap();
    let hits2 = engine2.search_l2(&query, 3).unwrap();
    
    assert_eq!(hits1, hits2, "Search results must match between original and restored engine");
    
    // Insert [D] into both independently?
    // Engine2 was restored. 
    // If HNSW level generation is deterministic based on ID, and insertion order is same...
    // Actually, `restore` rebuilds the index by iterating kernel records.
    // Ensure kernel record iteration order is deterministic (usually by ID).
    // `state.get_record(rid)` is pulled by ID 0..MAX in the restore loop.
    // So insertion order into HNSW during restore is effectively `0, 1, 2...`.
    // Original insertion was also `0, 1, 2...`.
    // So the graphs should be IDENTICAL.
    // If graphs are identical, searches are identical.
    
    // Verify graph structure? We can't access internals easily.
    // But we can verify determinism of search.
    
    println!("Hits: {:?}", hits1);
}
