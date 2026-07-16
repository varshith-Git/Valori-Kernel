// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! P5 — Brute-force vs BQ (binary quantization): recall, latency, memory.
//!
//! Three axes on the SAME synthetic clustered dataset:
//!   - search latency (p50/p99) for each index kind
//!   - recall@k of BQ against brute-force's own exact search as ground truth
//!   - memory bytes/vector, measured via peak RSS of an isolated child
//!     process per (index kind, scale) — building both engines sequentially
//!     in one process would let the first engine's freed heap contaminate
//!     the second's RSS reading, since allocators don't reliably return
//!     freed pages to the OS. Two scales per kind (20K, 100K) and taking
//!     the slope cancels fixed process/runtime overhead out of the estimate.
//!
//! Run: `cargo run --release -p valori-cli --bin bench_bf_vs_bq`

use std::collections::HashSet;
use std::time::Instant;
use valori_node::config::{IndexKind, NodeConfig, QuantizationKind};
use valori_node::engine::Engine;
use valori_node::EngineFromNodeConfig;

const DIM: usize = 128;
const N: usize = 50_000;
const K: usize = 10;
const QUERY_COUNT: usize = 300;
const CLUSTERS: usize = 20;

// ── Deterministic PRNG (xorshift64*) — avoids adding a `rand` dependency
// just for synthetic benchmark data. Not cryptographic; just needs to look
// varied enough to produce realistic cluster structure. ──────────────────

struct Rng(u64);
impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    /// Roughly uniform in [-1.0, 1.0).
    fn next_f32(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }
}

fn cluster_center(cluster: usize, dim: usize) -> Vec<f32> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15u64.wrapping_add(cluster as u64 * 0xBF58_476D_1CE4_E5B9));
    (0..dim).map(|_| rng.next_f32() * 5.0).collect()
}

/// A point near `cluster`'s center — inserted vectors and held-out queries
/// are both drawn from this, so nearest-neighbor structure is meaningful
/// (uniform-random high-dim vectors are all roughly equidistant, which
/// would make any recall number vacuous).
fn jittered_point(cluster: usize, seed: u64, dim: usize) -> Vec<f32> {
    let center = cluster_center(cluster, dim);
    let mut rng = Rng(seed ^ 0xD6E8_FEB8_6659_FD93);
    center.iter().map(|&c| c + rng.next_f32() * 0.3).collect()
}

fn percentile(sorted_ms: &[f64], p: f64) -> f64 {
    let idx = (((sorted_ms.len() - 1) as f64) * p / 100.0).round() as usize;
    sorted_ms[idx]
}

fn base_cfg(index_kind: IndexKind, max_records: usize) -> NodeConfig {
    NodeConfig {
        dim: DIM,
        max_records,
        index_kind,
        quantization_kind: QuantizationKind::None,
        wal_path: None,
        snapshot_path: None,
        event_log_path: None,
        ..NodeConfig::default()
    }
}

fn build_and_insert(cfg: &NodeConfig, n: usize) -> Engine {
    let mut engine = Engine::new(cfg);
    for i in 0..n {
        let v = jittered_point(i % CLUSTERS, i as u64, DIM);
        engine.insert_record_from_f32(&v).expect("insert failed");
    }
    engine.build_index();
    engine
}

fn latency_percentiles_ms(engine: &Engine, queries: &[Vec<f32>]) -> (f64, f64) {
    let mut lats: Vec<f64> = Vec::with_capacity(queries.len());
    for q in queries {
        let t0 = Instant::now();
        let _ = engine.search_l2(q, K).expect("search failed");
        lats.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    lats.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (percentile(&lats, 50.0), percentile(&lats, 99.0))
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "--mem-child" {
        return run_mem_child(&args[2], args[3].parse()?);
    }
    run_benchmark()
}

/// Isolated child mode: build exactly one engine, report peak RSS, exit.
/// Invoked by `run_benchmark` via `Command::new(current_exe)` so this
/// process's memory footprint can't be polluted by any other engine.
fn run_mem_child(kind_arg: &str, n: usize) -> anyhow::Result<()> {
    let index_kind = match kind_arg {
        "bq" => IndexKind::Bq,
        _ => IndexKind::BruteForce,
    };
    let cfg = base_cfg(index_kind, n + 1_000);
    let engine = build_and_insert(&cfg, n);
    // Keep `engine` alive up to the RSS read — without this the optimizer
    // is free to drop it early since nothing downstream uses it.
    std::hint::black_box(&engine);
    println!("RSS_BYTES={}", peak_rss_bytes());
    Ok(())
}

fn run_benchmark() -> anyhow::Result<()> {
    println!("Brute-force vs BQ  (dim={DIM}, n={N}, k={K}, {CLUSTERS} clusters, release build recommended)\n");

    println!("Building brute-force engine ({N} records)...");
    let t0 = Instant::now();
    let bf_engine = build_and_insert(&base_cfg(IndexKind::BruteForce, N + 1_000), N);
    println!("  done in {:.2?}", t0.elapsed());

    println!("Building BQ engine ({N} records)...");
    let t0 = Instant::now();
    let bq_engine = build_and_insert(&base_cfg(IndexKind::Bq, N + 1_000), N);
    println!("  done in {:.2?}\n", t0.elapsed());

    // Held-out queries: same cluster distribution, never inserted.
    let queries: Vec<Vec<f32>> = (0..QUERY_COUNT)
        .map(|i| jittered_point(i % CLUSTERS, 0xABCD_EF00_0000_0000 + i as u64, DIM))
        .collect();

    // ── Latency ──────────────────────────────────────────────────────────
    let (bf_p50, bf_p99) = latency_percentiles_ms(&bf_engine, &queries);
    let (bq_p50, bq_p99) = latency_percentiles_ms(&bq_engine, &queries);

    // ── Recall@K: brute-force's own exact search IS the ground truth ────
    let mut recall_sum = 0.0f64;
    for q in &queries {
        let truth: HashSet<u32> = bf_engine.search_l2(q, K)?.into_iter().map(|(id, _)| id).collect();
        let got: HashSet<u32> = bq_engine.search_l2(q, K)?.into_iter().map(|(id, _)| id).collect();
        recall_sum += truth.intersection(&got).count() as f64 / K as f64;
    }
    let recall_at_k = recall_sum / QUERY_COUNT as f64;

    println!("{:<12} | {:>10} | {:>10} | {:>12}", "Index", "p50", "p99", "Recall@10");
    println!("{}", "-".repeat(52));
    println!("{:<12} | {:>7.3} ms | {:>7.3} ms | {:>12}", "bruteforce", bf_p50, bf_p99, "1.000 (truth)");
    println!("{:<12} | {:>7.3} ms | {:>7.3} ms | {:>12.3}", "bq", bq_p50, bq_p99, recall_at_k);

    // ── Memory bytes/vector: isolated child processes, two scales, slope ──
    println!("\nMemory bytes/vector (peak RSS, isolated child process, slope over 20K→100K records):");
    println!("{:<12} | {:>12} | {:>12} | {:>14}", "Index", "RSS @20K", "RSS @100K", "bytes/vector");
    println!("{}", "-".repeat(58));

    for (label, kind_arg) in [("bruteforce", "bruteforce"), ("bq", "bq")] {
        let rss_small = measure_child_rss(kind_arg, 20_000)?;
        let rss_large = measure_child_rss(kind_arg, 100_000)?;
        let slope = (rss_large as f64 - rss_small as f64) / (100_000.0 - 20_000.0);
        println!(
            "{:<12} | {:>9.2} MB | {:>9.2} MB | {:>11.1} B",
            label,
            rss_small as f64 / 1_048_576.0,
            rss_large as f64 / 1_048_576.0,
            slope
        );
    }

    Ok(())
}

fn measure_child_rss(kind_arg: &str, n: usize) -> anyhow::Result<u64> {
    let exe = std::env::current_exe()?;
    let output = std::process::Command::new(exe)
        .arg("--mem-child")
        .arg(kind_arg)
        .arg(n.to_string())
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "memory-measurement child process failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .find(|l| l.starts_with("RSS_BYTES="))
        .ok_or_else(|| anyhow::anyhow!("child did not report RSS_BYTES; stdout was: {stdout}"))?;
    Ok(line.trim_start_matches("RSS_BYTES=").parse()?)
}

fn peak_rss_bytes() -> u64 {
    #[cfg(unix)]
    unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        // macOS reports ru_maxrss in bytes; Linux reports it in KiB.
        #[cfg(target_os = "macos")]
        {
            usage.ru_maxrss as u64
        }
        #[cfg(not(target_os = "macos"))]
        {
            usage.ru_maxrss as u64 * 1024
        }
    }
    #[cfg(not(unix))]
    {
        0
    }
}
