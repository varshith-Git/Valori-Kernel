// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Evaluation and Benchmark of 1-bit Binary Quantization (BQ) vs Brute-Force Index.

use std::time::Instant;
use valori_kernel::index::bq::BinaryQuantizationIndex;
use valori_kernel::index::brute_force::BruteForceIndex;
use valori_kernel::index::{SearchResult, VectorIndex};
use valori_kernel::storage::pool::RecordPool;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

/// Simple deterministic linear congruential generator for test data generation.
struct PseudoRand {
    state: u64,
}

impl PseudoRand {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_i32(&mut self, min: i32, max: i32) -> i32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let range = (max - min) as u64;
        min + ((self.state >> 32) % range) as i32
    }

    fn next_vec(&mut self, dim: usize) -> FxpVector {
        let mut data = Vec::with_capacity(dim);
        for _ in 0..dim {
            let val = self.next_i32(-65536, 65536);
            data.push(FxpScalar(val));
        }
        FxpVector { data }
    }
}

#[test]
fn evaluate_bq_value_add() {
    let num_records = 250000;
    let dim = 128;
    let num_queries = 100;
    let k = 10;

    println!("\n==============================================================");
    println!("🚀 VALORI KERNEL: BQ vs BRUTE-FORCE INDEX EVALUATION");
    println!("==============================================================");
    println!(
        "Dataset: {} clustered embeddings, Dimension: {}, Top-K: {}",
        num_records, dim, k
    );

    let mut rng = PseudoRand::new(42);
    let mut pool = RecordPool::new();

    // Generate 1000 cluster centers (like 1000 semantic topics)
    let num_clusters = 1000;
    let records_per_cluster = num_records / num_clusters;
    let mut centers = Vec::with_capacity(num_clusters);
    for _ in 0..num_clusters {
        centers.push(rng.next_vec(dim));
    }

    for center in &centers {
        for _ in 0..records_per_cluster {
            let mut vec = center.clone();
            // Add small random perturbation around cluster center
            for scalar in vec.data.iter_mut() {
                let noise = rng.next_i32(-8192, 8192);
                scalar.0 = scalar.0.saturating_add(noise);
            }
            pool.insert(vec, None, 0, 0).unwrap();
        }
    }

    // Generate queries by perturbing cluster centers
    let mut queries = Vec::with_capacity(num_queries);
    for i in 0..num_queries {
        let center = &centers[i % num_clusters];
        let mut q = center.clone();
        for scalar in q.data.iter_mut() {
            let noise = rng.next_i32(-4096, 4096);
            scalar.0 = scalar.0.saturating_add(noise);
        }
        queries.push(q);
    }

    // 1. Build and Measure Memory Footprint
    let mut bf_idx = BruteForceIndex::default();
    let bf_build_start = Instant::now();
    bf_idx.rebuild(&pool);
    let bf_build_time = bf_build_start.elapsed();
    let raw_vec_bytes = num_records * dim * 4;

    let mut bq_idx = BinaryQuantizationIndex::new();
    let bq_build_start = Instant::now();
    bq_idx.rebuild(&pool);
    let bq_build_time = bq_build_start.elapsed();
    let bq_index_bytes = bq_idx.codes.len() * 8;

    println!("\n📦 MEMORY FOOTPRINT & BUILD TIME:");
    println!(
        "  • Full Q16.16 Vectors Size : {:>8} bytes ({:.2} KB)",
        raw_vec_bytes,
        raw_vec_bytes as f64 / 1024.0
    );
    println!(
        "  • BQ Bitstring Index Size  : {:>8} bytes ({:.2} KB)",
        bq_index_bytes,
        bq_index_bytes as f64 / 1024.0
    );
    println!(
        "  • Memory Reduction         : {:.1}x smaller ({:.1}% saved!)",
        raw_vec_bytes as f64 / bq_index_bytes as f64,
        (1.0 - (bq_index_bytes as f64 / raw_vec_bytes as f64)) * 100.0
    );
    println!(
        "  • BQ Index Rebuild Time    : {:.2?} (vs {:.2?} for BF)",
        bq_build_time, bf_build_time
    );

    // 2. Measure Search Latency & Speedup
    let mut bf_results = vec![SearchResult::default(); k];
    let bf_search_start = Instant::now();
    let mut all_bf_hits = Vec::with_capacity(num_queries);
    for q in &queries {
        let count = bf_idx.search(&pool, q, &mut bf_results, None);
        all_bf_hits.push(bf_results[..count].to_vec());
    }
    let bf_search_time = bf_search_start.elapsed();

    let mut bq_results = vec![SearchResult::default(); k];
    let bq_search_start = Instant::now();
    let mut all_bq_hits = Vec::with_capacity(num_queries);
    for q in &queries {
        let count = bq_idx.search(&pool, q, &mut bq_results, None);
        all_bq_hits.push(bq_results[..count].to_vec());
    }
    let bq_search_time = bq_search_start.elapsed();

    println!("\n⚡ SEARCH LATENCY (Over {} queries):", num_queries);
    println!(
        "  • Brute-Force Total Time   : {:.2?} ({:.2?} / query)",
        bf_search_time,
        bf_search_time / num_queries as u32
    );
    println!(
        "  • BQ Two-Stage Total Time  : {:.2?} ({:.2?} / query)",
        bq_search_time,
        bq_search_time / num_queries as u32
    );
    let speedup = bf_search_time.as_secs_f64() / bq_search_time.as_secs_f64();
    println!("  • Latency Speedup          : {:.2}x faster!", speedup);

    // 3. Measure Recall@K
    let mut total_recall_hits = 0;
    let mut total_expected_hits = 0;

    for (bf_hits, bq_hits) in all_bf_hits.iter().zip(all_bq_hits.iter()) {
        total_expected_hits += bf_hits.len();
        for bq_hit in bq_hits {
            if bf_hits.iter().any(|bf| bf.id == bq_hit.id) {
                total_recall_hits += 1;
            }
        }
    }

    let recall_pct = (total_recall_hits as f64 / total_expected_hits as f64) * 100.0;
    println!("\n🎯 ACCURACY / RECALL@{}:", k);
    println!(
        "  • Exact matches found      : {} / {}",
        total_recall_hits, total_expected_hits
    );
    println!("  • Recall@{} Score           : {:.2}%", k, recall_pct);
    println!("==============================================================\n");

    assert!(
        recall_pct > 80.0,
        "Recall should be high due to two-stage rescoring!"
    );
}
