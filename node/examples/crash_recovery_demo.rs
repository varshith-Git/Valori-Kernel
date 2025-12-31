// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Simple Crash Recovery Demo
//!
//! This demonstrates WAL-based crash recovery.

use valori_node::config::{NodeConfig, IndexKind, QuantizationKind};
use valori_node::engine::Engine;
use tempfile::tempdir;

const MAX_RECORDS: usize = 1024;
const DIM: usize = 16;
const MAX_NODES: usize = 1024;
const MAX_EDGES: usize = 2048;

fn main() {
    println!("\n╔════════════════════════════════════════╗");
    println!("║  Valori Crash Recovery Demo           ║");
    println!("╚════════════════════════════════════════╝\n");

    let dir = tempdir().unwrap();
    let snapshot_path = dir.path().join("demo.snapshot");
    let wal_path = dir.path().join("demo.wal");

    let config = NodeConfig {
        max_records: MAX_RECORDS,
        dim: DIM,
        max_nodes: MAX_NODES,
        max_edges: MAX_EDGES,
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        index_kind: IndexKind::BruteForce,
        quantization_kind: QuantizationKind::None,
        snapshot_path: Some(snapshot_path.clone()),
        wal_path: Some(wal_path.clone()),
        auto_snapshot_interval_secs: None,
        auth_token: None,
        event_log_path: None,
        mode: valori_node::config::NodeMode::Leader,
    };

    // Phase 1: Insert & Snapshot
    println!("📝 Phase 1: Insert 50 records + snapshot");
    let mut engine = Engine::<MAX_RECORDS, DIM, MAX_NODES,MAX_EDGES>::new(&config);
    
    for i in 0..50 {
        let v: Vec<f32> = (0..DIM).map(|j| (i + j) as f32 * 0.1).collect();
        engine.insert_record_from_f32(&v).unwrap();
    }
    
    engine.save_snapshot(None).unwrap();
    println!("   ✅ Snapshot saved with 50 records\n");
    
    // Rotate WAL (remove old one)
    drop(engine);
    std::fs::remove_file(&wal_path).ok();
    
    // Phase 2: Restore + Insert more
    println!("📝 Phase 2: Restore from snapshot + insert 50 more");
    let mut engine2 = Engine::<MAX_RECORDS, DIM, MAX_NODES, MAX_EDGES>::new(&config);
    let snap_bytes = std::fs::read(&snapshot_path).unwrap();
    engine2.restore(&snap_bytes).unwrap();
    
    for i in 50..100 {
        let v: Vec<f32> = (0..DIM).map(|j| (i + j) as f32 * 0.1).collect();
        engine2.insert_record_from_f32(&v).unwrap();
    }
    
    let pre_crash_hash = engine2.get_proof().final_state_hash;
    println!("   ✅ 100 total records");
    println!("   📊 State hash: {:?}...\n", &pre_crash_hash[..6]);
    
    // Phase 3: CRASH
    println!("💥 Phase 3: CRASH! (dropping engine)");
    drop(engine2);
    println!("   💔 Memory lost, only snapshot + WAL on disk\n");
    
    // Phase 4: RECOVERY
    println!("🔧 Phase 4: RECOVERY (snapshot + WAL replay)");
    let mut recovered = Engine::<MAX_RECORDS, DIM, MAX_NODES, MAX_EDGES>::new(&config);
    let snap_bytes2 = std::fs::read(&snapshot_path).unwrap();
    
    let cmds = recovered.restore_with_wal_replay(&snap_bytes2, &wal_path).unwrap();
    println!("   ✅ replayed {} commands from WAL", cmds);
    
    // Phase 5: VALIDATE
    println!("\n✨ Phase 5: VALIDATION");
    let recovered_hash = recovered.get_proof().final_state_hash;
    println!("   Pre-crash : {:?}...", &pre_crash_hash[..6]);
    println!("   Recovered : {:?}...", &recovered_hash[..6]);
    
    if pre_crash_hash == recovered_hash {
        println!("\n   ✅✅✅ SUCCESS! Hashes MATCH!");
        println!("   🎯 Deterministic recovery proven!");
    } else {
        println!("\n   ❌ FAILURE: Hashes don't match");
        std::process::exit(1);
    }
    
    println!("\n╔════════════════════════════════════════╗");
    println!("║  ✅ Crash Recovery Working! 🚀        ║");
    println!("╚════════════════════════════════════════╝\n");
}
