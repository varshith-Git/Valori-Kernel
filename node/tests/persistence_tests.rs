use valori_node::config::NodeConfig;
// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use valori_node::engine::Engine;
use std::sync::Arc;
use tempfile::tempdir;

const D: usize = 4;
const MAX_RECORDS: usize = 100;
const MAX_NODES: usize = 100;
const MAX_EDGES: usize = 500;

#[tokio::test]
async fn test_index_persistence() {
    let dir = tempdir().unwrap();
    let snap_path = dir.path().join("snapshot.bin");
    
    let mut cfg = NodeConfig::default();
    cfg.max_records = MAX_RECORDS;
    cfg.dim = D;
    cfg.max_nodes = MAX_NODES; // Was missing
    // MAX_EDGES is 2048 in default, 500 in test
    cfg.max_edges = MAX_EDGES;
    cfg.index_kind = valori_node::config::IndexKind::Hnsw;
    cfg.snapshot_path = Some(snap_path.clone());

    // 1. Create Engine, Insert Data
    {
        let mut engine = Engine::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>::new(&cfg);
        let id = engine.insert_record_from_f32(&[0.1, 0.2, 0.3, 0.4]).unwrap();
        assert_eq!(id, 0);

        // Verify Search works
        let results = engine.search_l2(&[0.1, 0.2, 0.3, 0.4], 1).unwrap();
        assert_eq!(results[0].0, 0);
        
        // Save Snapshot
        engine.save_snapshot(Some(&snap_path)).expect("Snapshot failed");
        assert!(snap_path.exists());
    }

    // 2. Restore to New Engine
    {
        let mut engine2 = Engine::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>::new(&cfg);
        
        let data = std::fs::read(&snap_path).unwrap();
        engine2.restore(&data).expect("Restore failed");
        
        // 3. Verify Index is present/working WITHOUT manual insert
        // The restore log should say "Restoring index from snapshot (fast load)..."
        // We verify by searching.
        let results = engine2.search_l2(&[0.1, 0.2, 0.3, 0.4], 1).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 0);
    }
    
    // 3. Test Corruption
    {
        let mut data = std::fs::read(&snap_path).unwrap();
        // Corrupt last byte (Checksum)
        let last = data.len() - 1;
        data[last] ^= 0xFF;
        
        let mut engine3 = Engine::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>::new(&cfg);
        let res = engine3.restore(&data);
        assert!(res.is_err());
        println!("Corruption check passed: {:?}", res.err());
    }

    // 4. Test Truncation
    {
        let mut data = std::fs::read(&snap_path).unwrap();
        // Truncate to half
        data.truncate(data.len() / 2);
        
        let mut engine4 = Engine::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>::new(&cfg);
        let res = engine4.restore(&data);
        assert!(res.is_err());
        println!("Truncation check passed: {:?}", res.err());
    }
}
