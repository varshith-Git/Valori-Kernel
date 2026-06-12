// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Crash Recovery Demo
//!
//! Demonstrates event-log based crash recovery via `Engine::try_recover()`.

use valori_node::config::{NodeConfig, IndexKind, QuantizationKind};
use valori_node::engine::{Engine, RecoveryMode};
use tempfile::tempdir;

const DIM: usize = 16;

fn main() {
    println!("\n╔════════════════════════════════════════╗");
    println!("║  Valori Crash Recovery Demo           ║");
    println!("╚════════════════════════════════════════╝\n");

    let dir = tempdir().unwrap();

    let config = NodeConfig {
        max_records: 1024,
        dim: DIM,
        max_nodes: 1024,
        max_edges: 2048,
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        index_kind: IndexKind::BruteForce,
        quantization_kind: QuantizationKind::None,
        snapshot_path: Some(dir.path().join("demo.snapshot")),
        wal_path: None,
        auto_snapshot_interval_secs: None,
        auth_token: None,
        event_log_path: Some(dir.path().join("events.log")),
        mode: valori_node::config::NodeMode::Leader,
        ..Default::default()
    };

    // ── Phase 1: Insert 100 records ───────────────────────────────────────────
    println!("📝 Phase 1: Insert 100 records");
    let pre_crash_hash;
    {
        let mut engine = Engine::new(&config);
        assert_eq!(engine.try_recover(), RecoveryMode::Fresh);

        for i in 0..100 {
            let v: Vec<f32> = (0..DIM).map(|j| (i + j) as f32 * 0.1).collect();
            engine.insert_record_from_f32(&v).unwrap();
        }

        pre_crash_hash = engine.get_proof().final_state_hash;
        println!("   ✅ 100 records inserted");
        println!("   📊 State hash: {:?}...\n", &pre_crash_hash[..6]);
        // Drop → BufWriter flushes → all events reach disk.
    }

    // ── Phase 2: CRASH ────────────────────────────────────────────────────────
    println!("💥 Phase 2: CRASH! (engine dropped, only event log on disk)\n");

    // ── Phase 3: RECOVERY ─────────────────────────────────────────────────────
    println!("🔧 Phase 3: RECOVERY via try_recover()");
    let recovered_hash;
    {
        let mut engine2 = Engine::new(&config);
        match engine2.try_recover() {
            RecoveryMode::EventLog(n) => {
                println!("   ✅ Replayed {} events from event log", n);
                assert_eq!(n, 100, "must replay all 100 events");
            }
            other => panic!("unexpected recovery mode: {:?}", other),
        }
        recovered_hash = engine2.get_proof().final_state_hash;
    }

    // ── Phase 4: VALIDATE ─────────────────────────────────────────────────────
    println!("\n✨ Phase 4: VALIDATION");
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
