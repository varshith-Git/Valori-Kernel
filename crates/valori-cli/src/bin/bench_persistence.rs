// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Benchmark: snapshot save and load round-trip latency.
//!
//! Measures the real production persistence path:
//!   Engine::snapshot()  → write to disk
//!   read from disk      → Engine::restore()
//!
//! Data source: SIFT1M base vectors at `data/sift/sift/sift_base.fvecs`.

use anyhow::Result;
use bytemuck::cast_slice;
use memmap2::Mmap;
use std::fs::File;
use std::time::Instant;

use valori_kernel::adapters::sift_batch::SiftBatchLoader;
use valori_node::engine::Engine;
use valori_node::config::{NodeConfig, IndexKind, QuantizationKind};

const DIM: usize = 128;
const INGEST_LIMIT: usize = 50_000;

fn main() -> Result<()> {
    println!("🚀 Persistence Benchmark  (real Engine, {} vectors)", INGEST_LIMIT);

    // ── 1. Build Engine ──────────────────────────────────────────────────────
    let cfg = NodeConfig {
        dim:               DIM,
        index_kind:        IndexKind::BruteForce,
        quantization_kind: QuantizationKind::None,
        wal_path:          None,
        snapshot_path:     None,
        event_log_path:    None,
        ..NodeConfig::default()
    };
    let mut engine = Engine::new(&cfg);

    // ── 2. Ingest SIFT1M vectors ─────────────────────────────────────────────
    println!("📥 Ingesting {} vectors …", INGEST_LIMIT);

    let path = "data/sift/sift/sift_base.fvecs";
    let file = File::open(path).expect("SIFT base file not found at data/sift/sift/sift_base.fvecs");
    let mmap = unsafe { Mmap::map(&file)? };
    let mut loader = SiftBatchLoader::new(&mmap)
        .ok_or_else(|| anyhow::anyhow!("Invalid SIFT fvecs format"))?;

    let mut ingested = 0usize;
    'outer: while let Some((raw_bytes, count)) = loader.next_batch(1_000) {
        let stride = 4 + DIM * 4;
        for i in 0..count {
            if ingested >= INGEST_LIMIT {
                break 'outer;
            }
            let offset = i * stride;
            let vec_f32: &[f32] = cast_slice(&raw_bytes[offset + 4..offset + stride]);
            engine.insert_record_from_f32(vec_f32)
                .map_err(|e| anyhow::anyhow!("insert failed: {e:?}"))?;
            ingested += 1;
        }
    }
    println!("✅ Ingested {} records", engine.record_count());

    // ── 3. Save snapshot ─────────────────────────────────────────────────────
    let snap_path = "valori_bench_persist.val";
    let _ = std::fs::remove_file(snap_path);

    println!("💾 Saving snapshot to {} …", snap_path);
    let t_save = Instant::now();
    let snap_bytes = engine.snapshot()
        .map_err(|e| anyhow::anyhow!("snapshot failed: {e:?}"))?;
    std::fs::write(snap_path, &snap_bytes)?;
    let save_time = t_save.elapsed();

    println!("✅ Saved  {:.2} KB  in {:.2?}", snap_bytes.len() as f64 / 1024.0, save_time);

    // ── 4. Drop the old engine ───────────────────────────────────────────────
    let original_count = engine.record_count();
    drop(engine);
    println!("🗑️  Original engine dropped.");

    // ── 5. Restore from snapshot ─────────────────────────────────────────────
    println!("📂 Restoring from {} …", snap_path);
    let file_bytes = std::fs::read(snap_path)?;

    let mut restored = Engine::new(&cfg);
    let t_load = Instant::now();
    restored.restore(&file_bytes)
        .map_err(|e| anyhow::anyhow!("restore failed: {e:?}"))?;
    let load_time = t_load.elapsed();

    println!("✅ Restored {} records in {:.2?}", restored.record_count(), load_time);
    assert_eq!(
        restored.record_count(),
        original_count,
        "Restored record count must match original"
    );

    // ── 6. Spot-check search works ───────────────────────────────────────────
    let q = vec![0.0f32; DIM];
    let results = restored.search_l2(&q, 1)
        .map_err(|e| anyhow::anyhow!("search failed: {e:?}"))?;
    println!("🔎 Spot search: {} result(s). First ID: {:?}", results.len(), results.first().map(|r| r.0));

    // ── 7. Report ────────────────────────────────────────────────────────────
    println!();
    println!("──────────────────────────────────────────────");
    println!("  PERSISTENCE REPORT  ({} vectors, {} KB snapshot)", INGEST_LIMIT, snap_bytes.len() / 1024);
    println!("  Save time:    {:>10.3?}", save_time);
    println!("  Restore time: {:>10.3?}", load_time);
    println!("──────────────────────────────────────────────");

    let _ = std::fs::remove_file(snap_path);
    Ok(())
}
