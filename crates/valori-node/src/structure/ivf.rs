// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use super::index::VectorIndex;
use super::deterministic::kmeans::{deterministic_kmeans, f32_to_q16, l2_sq_q16 as _l2_sq_q16_scalar};
use serde::{Serialize, Deserialize};
use std::cell::RefCell;

// ── SIMD-accelerated Q16.16 L2 squared distance ──────────────────────────────
// Uses NEON int32→int64 multiply-accumulate on aarch64; scalar fallback elsewhere.
// Produces the same i64 result as l2_sq_q16() — drop-in replacement.

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn l2_sq_neon(a: &[i32], b: &[i32]) -> i64 {
    use std::arch::aarch64::*;
    let len = a.len();
    let mut pa = a.as_ptr();
    let mut pb = b.as_ptr();
    let mut acc0 = vdupq_n_s64(0);
    let mut acc1 = vdupq_n_s64(0);
    let mut acc2 = vdupq_n_s64(0);
    let mut acc3 = vdupq_n_s64(0);
    let chunks8 = len / 8;
    for _ in 0..chunks8 {
        let d0 = vsubq_s32(vld1q_s32(pa),        vld1q_s32(pb));
        let d1 = vsubq_s32(vld1q_s32(pa.add(4)), vld1q_s32(pb.add(4)));
        acc0 = vmlal_s32(acc0, vget_low_s32(d0),  vget_low_s32(d0));
        acc1 = vmlal_s32(acc1, vget_high_s32(d0), vget_high_s32(d0));
        acc2 = vmlal_s32(acc2, vget_low_s32(d1),  vget_low_s32(d1));
        acc3 = vmlal_s32(acc3, vget_high_s32(d1), vget_high_s32(d1));
        pa = pa.add(8); pb = pb.add(8);
    }
    let combined = vaddq_s64(vaddq_s64(acc0, acc1), vaddq_s64(acc2, acc3));
    let mut sum = vgetq_lane_s64::<0>(combined) + vgetq_lane_s64::<1>(combined);
    let base = chunks8 * 8;
    for i in base..len {
        let d = *a.get_unchecked(i) as i64 - *b.get_unchecked(i) as i64;
        sum += d * d;
    }
    sum
}

#[inline(always)]
fn l2_sq(a: &[i32], b: &[i32]) -> i64 {
    #[cfg(target_arch = "aarch64")]
    return unsafe { l2_sq_neon(a, b) };
    #[cfg(not(target_arch = "aarch64"))]
    _l2_sq_q16_scalar(a, b)
}

// ── Thread-local scratch buffers ─────────────────────────────────────────────

thread_local! {
    static CENTROID_SCRATCH: RefCell<Vec<(usize, i64)>> = RefCell::new(Vec::new());
    static CANDIDATE_SCRATCH: RefCell<Vec<(u32, i64)>>  = RefCell::new(Vec::new());
}

// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
pub struct IvfConfig {
    /// Number of centroids. Ignored when `auto_scale = true` (the default).
    pub n_list: usize,
    /// Number of centroid buckets probed during search. Ignored when `auto_scale = true`.
    pub n_probe: usize,
    /// When true (default), `build()` overrides n_list = max(16, sqrt(N)) and
    /// n_probe = max(1, sqrt(n_list)). Set to false only when fixed values are
    /// required (VALORI_IVF_N_LIST / VALORI_IVF_N_PROBE are set).
    #[serde(default = "default_auto_scale")]
    pub auto_scale: bool,
}

fn default_auto_scale() -> bool { true }

impl Default for IvfConfig {
    fn default() -> Self {
        Self { n_list: 100, n_probe: 10, auto_scale: true }
    }
}

/// IVF index with Q16.16 fixed-point centroids and stored vectors.
///
/// All internal distance computation uses i64 integer arithmetic — no f32 in
/// the hot path.  f32 values are converted to Q16.16 once at the public
/// boundary (build / insert / search) and never touched again.
///
/// With auto_scale=true (default), centroid count = max(16, sqrt(N)) and
/// n_probe = max(1, sqrt(n_list)). This keeps average bucket size near
/// sqrt(N) and scan cost at O(sqrt(N)) rather than O(N).
pub struct IvfIndex {
    pub config: IvfConfig,
    pub dim: usize,
    /// Centroids in Q16.16 fixed-point.
    pub centroids: Vec<Vec<i32>>,
    /// Inverted lists: per-centroid list of (record_id, Q16.16 vector).
    pub inverted_lists: Vec<Vec<(u32, Vec<i32>)>>,
    /// Record count at the most recent `build()` call. Used to detect when
    /// the index has drifted far enough from its build state to need a rebuild.
    pub n_at_last_build: usize,
}

impl IvfIndex {
    pub fn new(config: IvfConfig, dim: usize) -> Self {
        Self { config, dim, centroids: Vec::new(), inverted_lists: Vec::new(), n_at_last_build: 0 }
    }

    /// True when online inserts since the last `build()` have grown the dataset
    /// past 2× the build size — centroid quality has degraded enough to warrant
    /// a rebuild.
    pub fn needs_rebuild(&self, current_count: usize) -> bool {
        self.n_at_last_build > 0 && current_count > self.n_at_last_build * 2
    }

    /// Compute effective n_list and n_probe for a given dataset size.
    /// Classic FAISS rule: n_list ≈ sqrt(N), n_probe ≈ sqrt(n_list).
    fn effective_params(config: &IvfConfig, n: usize) -> (usize, usize) {
        if !config.auto_scale {
            return (config.n_list, config.n_probe);
        }
        let n_list  = ((n as f64).sqrt() as usize).max(16);
        let n_probe = ((n_list as f64).sqrt() as usize).max(1);
        (n_list, n_probe)
    }

    /// Returns (centroid_index, squared_distance_i64).
    fn find_nearest_centroid(&self, q_vec: &[i32]) -> (usize, i64) {
        if self.centroids.is_empty() { return (0, i64::MAX); }
        let mut best_idx = 0usize;
        let mut best_dist = i64::MAX;
        for (i, c) in self.centroids.iter().enumerate() {
            let d = l2_sq(q_vec, c);
            // Strict integer comparison — deterministic on every architecture.
            if d < best_dist || (d == best_dist && i < best_idx) {
                best_dist = d;
                best_idx = i;
            }
        }
        (best_idx, best_dist)
    }
}

impl VectorIndex for IvfIndex {
    fn build(&mut self, records: &[(u32, Vec<f32>)]) {
        if records.is_empty() { return; }

        let (eff_n_list, eff_n_probe) = Self::effective_params(&self.config, records.len());
        // Persist the effective values so snapshot/restore reproduces them.
        self.config.n_list  = eff_n_list;
        self.config.n_probe = eff_n_probe;

        // Convert to Q16.16 for kmeans — centroids come back as Vec<Vec<i32>>.
        self.centroids = deterministic_kmeans(records, eff_n_list, 20);
        self.inverted_lists = vec![Vec::new(); self.centroids.len()];
        for (id, vec) in records {
            let q_vec: Vec<i32> = vec.iter().map(|&v| f32_to_q16(v)).collect();
            let (c_idx, _) = self.find_nearest_centroid(&q_vec);
            self.inverted_lists[c_idx].push((*id, q_vec));
        }

        self.n_at_last_build = records.len();
    }

    fn insert(&mut self, id: u32, vec: &[f32]) {
        let q_vec: Vec<i32> = vec.iter().map(|&v| f32_to_q16(v)).collect();
        if self.centroids.is_empty() {
            if self.inverted_lists.is_empty() {
                self.inverted_lists.push(Vec::new());
            }
            self.inverted_lists[0].push((id, q_vec));
            return;
        }
        let (c_idx, _) = self.find_nearest_centroid(&q_vec);
        self.inverted_lists[c_idx].push((id, q_vec));
    }

    fn delete(&mut self, id: u32) {
        for list in self.inverted_lists.iter_mut() {
            list.retain(|(rid, _)| *rid != id);
        }
    }

    fn search(&self, query: &[f32], k: usize) -> Vec<(u32, f32)> {
        let q_query: Vec<i32> = query.iter().map(|&v| f32_to_q16(v)).collect();

        if self.centroids.is_empty() {
            if self.inverted_lists.is_empty() {
                return Vec::new();
            }
            return CANDIDATE_SCRATCH.with(|cell| {
                let mut candidates = cell.borrow_mut();
                candidates.clear();
                candidates.extend(self.inverted_lists[0].iter()
                    .map(|(id, q_vec)| (*id, l2_sq(&q_query, q_vec))));
                if candidates.len() > k {
                    candidates.select_nth_unstable_by(k - 1, |a, b| {
                        a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0))
                    });
                    candidates.truncate(k);
                }
                candidates.sort_unstable_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
                candidates.iter().map(|(id, d)| (*id, *d as f32)).collect()
            });
        }

        CENTROID_SCRATCH.with(|ccell| {
            CANDIDATE_SCRATCH.with(|qcell| {
                let mut centroid_dists = ccell.borrow_mut();
                let mut candidates = qcell.borrow_mut();

                // Build centroid distance list (SIMD) and partial-sort to find n_probe nearest.
                centroid_dists.clear();
                centroid_dists.extend(self.centroids.iter().enumerate()
                    .map(|(i, c)| (i, l2_sq(&q_query, c))));

                let probes = self.config.n_probe.min(centroid_dists.len());
                if probes < centroid_dists.len() {
                    // O(n) partial select — only guarantees the first `probes` are the nearest,
                    // not that they are sorted among themselves. We iterate all probes anyway.
                    centroid_dists.select_nth_unstable_by(probes - 1, |a, b| {
                        a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0))
                    });
                }

                // Scan selected buckets (SIMD per vector).
                candidates.clear();
                for i in 0..probes {
                    let c_idx = centroid_dists[i].0;
                    for (id, q_vec) in &self.inverted_lists[c_idx] {
                        candidates.push((*id, l2_sq(&q_query, q_vec)));
                    }
                }

                // O(n) partial select for top-k, then sort only k elements.
                if candidates.len() > k {
                    candidates.select_nth_unstable_by(k - 1, |a, b| {
                        a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0))
                    });
                    candidates.truncate(k);
                }
                candidates.sort_unstable_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

                // Convert i64 Q16.16² distances back to f32 only at the output boundary.
                candidates.iter().map(|(id, d)| (*id, *d as f32)).collect()
            })
        })
    }

    fn snapshot(&self) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        #[derive(Serialize)]
        struct IvfDump<'a> {
            config: &'a IvfConfig,
            centroids: &'a Vec<Vec<i32>>,
            inverted_lists: Vec<Vec<(u32, &'a Vec<i32>)>>,
        }

        // Sort each list by ID for snapshot determinism regardless of insertion order.
        let sorted_lists: Vec<Vec<(u32, &Vec<i32>)>> = self.inverted_lists.iter()
            .map(|list| {
                let mut refs: Vec<(u32, &Vec<i32>)> = list.iter().map(|(id, v)| (*id, v)).collect();
                refs.sort_by_key(|(id, _)| *id);
                refs
            })
            .collect();

        let dump = IvfDump {
            config: &self.config,
            centroids: &self.centroids,
            inverted_lists: sorted_lists,
        };

        Ok(bincode::serde::encode_to_vec(&dump, bincode::config::standard())?)
    }

    fn restore(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[derive(Deserialize)]
        struct IvfLoad {
            config: IvfConfig,
            centroids: Vec<Vec<i32>>,
            inverted_lists: Vec<Vec<(u32, Vec<i32>)>>,
        }
        let dump: IvfLoad = bincode::serde::decode_from_slice(data, bincode::config::standard())?.0;
        // n_at_last_build is derived from the total record count in the lists.
        let total: usize = dump.inverted_lists.iter().map(|l| l.len()).sum();
        self.config = dump.config;
        self.centroids = dump.centroids;
        self.inverted_lists = dump.inverted_lists;
        self.dim = if self.centroids.is_empty() { 0 } else { self.centroids[0].len() };
        self.n_at_last_build = total;
        Ok(())
    }
}

// ── Benchmarks ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod bench {
    use super::*;

    fn rand_vec(dim: usize, seed: u64) -> Vec<f32> {
        let mut s = seed ^ 0x9e3779b97f4a7c15;
        (0..dim).map(|_| {
            s ^= s << 13; s ^= s >> 7; s ^= s << 17;
            (s as i64 as f32) / (i64::MAX as f32)
        }).collect()
    }

    fn brute_top_k(corpus: &[(u32, Vec<f32>)], query: &[f32], k: usize) -> std::collections::HashSet<u32> {
        let mut dists: Vec<(u32, f32)> = corpus.iter().map(|(id, v)| {
            let d: f32 = v.iter().zip(query).map(|(a, b)| (a - b) * (a - b)).sum();
            (*id, d)
        }).collect();
        dists.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        dists.iter().take(k).map(|(id, _)| *id).collect()
    }

    /// IVF latency (p50/p95) + Recall@10 across dims and dataset sizes.
    /// Run with: cargo test -p valori-node --lib --release ivf_latency_benchmark -- --nocapture --ignored
    #[test]
    #[ignore]
    fn ivf_latency_benchmark() {
        const TRIALS: usize = 500;
        const K: usize = 10;
        const N_QUERIES: usize = 50;

        println!("\n=== IVF latency + recall benchmark ===");
        println!("{:<6} {:<8} {:<10} {:<10} {:<10} {:<10}",
            "dim", "N", "p50 µs", "p95 µs", "Recall@10", "n_probe");

        for &dim in &[128usize, 384, 768] {
            for &n in &[1_000usize, 5_000, 25_000, 50_000] {
                let corpus: Vec<(u32, Vec<f32>)> = (0..n)
                    .map(|i| (i as u32, rand_vec(dim, i as u64 * 7 + 1)))
                    .collect();
                let queries: Vec<Vec<f32>> = (0..N_QUERIES)
                    .map(|i| rand_vec(dim, i as u64 * 31 + 999_999))
                    .collect();

                let mut idx = IvfIndex::new(IvfConfig::default(), dim);
                idx.build(&corpus);
                let n_probe = idx.config.n_probe;

                // warm-up
                for q in &queries { idx.search(q, K); }

                let mut times: Vec<u128> = Vec::with_capacity(TRIALS);
                let mut recall_sum = 0.0f64;

                for trial in 0..TRIALS {
                    let q = &queries[trial % N_QUERIES];
                    let t0 = std::time::Instant::now();
                    let hits = idx.search(q, K);
                    times.push(t0.elapsed().as_micros());

                    if trial < N_QUERIES {
                        let truth = brute_top_k(&corpus, q, K);
                        let found = hits.iter().filter(|(id, _)| truth.contains(id)).count();
                        recall_sum += found as f64 / K as f64;
                    }
                }

                times.sort_unstable();
                let p50 = times[TRIALS / 2];
                let p95 = times[(TRIALS as f64 * 0.95) as usize];
                let recall = recall_sum / N_QUERIES as f64 * 100.0;

                println!("{:<6} {:<8} {:<10} {:<10} {:<9.1}% {:<10}",
                    dim, n, p50, p95, recall, n_probe);
            }
        }
    }

    /// n_probe sweep — recall@10 vs probe count at dim=384, N=25k.
    /// Run with: cargo test -p valori-node --lib --release ivf_probe_sweep -- --nocapture --ignored
    #[test]
    #[ignore]
    fn ivf_probe_sweep() {
        const N: usize = 25_000;
        const DIM: usize = 384;
        const K: usize = 10;
        const N_QUERIES: usize = 100;

        let corpus: Vec<(u32, Vec<f32>)> = (0..N)
            .map(|i| (i as u32, rand_vec(DIM, i as u64 * 7 + 1)))
            .collect();
        let queries: Vec<Vec<f32>> = (0..N_QUERIES)
            .map(|i| rand_vec(DIM, i as u64 * 31 + 999_999))
            .collect();
        let truths: Vec<std::collections::HashSet<u32>> = queries.iter()
            .map(|q| brute_top_k(&corpus, q, K))
            .collect();

        let mut idx = IvfIndex::new(IvfConfig { auto_scale: true, ..Default::default() }, DIM);
        idx.build(&corpus);
        let n_list = idx.config.n_list;

        println!("\n=== IVF n_probe sweep  dim={DIM}  N={N}  n_list={n_list}  k={K} ===");
        println!("{:<10} {:<12} {:<10}", "n_probe", "Recall@10", "p50 µs");

        for &probe in &[1, 2, 4, 8, 16, 32, 64, n_list] {
            idx.config.n_probe = probe.min(n_list);
            let mut recall_sum = 0.0f64;
            let mut times: Vec<u128> = Vec::with_capacity(N_QUERIES);
            for (q, truth) in queries.iter().zip(&truths) {
                let t0 = std::time::Instant::now();
                let hits = idx.search(q, K);
                times.push(t0.elapsed().as_micros());
                let found = hits.iter().filter(|(id, _)| truth.contains(id)).count();
                recall_sum += found as f64 / K as f64;
            }
            times.sort_unstable();
            let recall = recall_sum / N_QUERIES as f64 * 100.0;
            println!("{:<10} {:<11.1}% {:<10}", probe.min(n_list), recall, times[N_QUERIES / 2]);
        }
    }

    // ── helpers shared by the diagnostics below ───────────────────────────────

    fn nearest_centroid_f32(centroids: &[Vec<i32>], q: &[i32]) -> usize {
        centroids.iter().enumerate()
            .map(|(i, c)| (i, l2_sq(q, c)))
            .min_by_key(|&(_, d)| d)
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    fn nearest_neighbor(corpus: &[(u32, Vec<f32>)], query: &[f32]) -> u32 {
        corpus.iter()
            .map(|(id, v)| {
                let d: f32 = v.iter().zip(query).map(|(a, b)| (a - b) * (a - b)).sum();
                (*id, d)
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|(id, _)| id)
            .unwrap()
    }

    // ── Diagnostic 1: k-means quality ────────────────────────────────────────
    // For each vector: is its true nearest neighbor in the same centroid bucket?
    // A perfect quantizer would score 100%. Random partitioning ~= 1/n_list.
    // cargo test -p valori-node --lib --release ivf_kmeans_quality -- --nocapture --ignored
    #[test]
    #[ignore]
    fn ivf_kmeans_quality() {
        const DIM: usize = 384;
        const N: usize = 10_000;

        let corpus: Vec<(u32, Vec<f32>)> = (0..N)
            .map(|i| (i as u32, rand_vec(DIM, i as u64 * 7 + 1)))
            .collect();

        println!("\n=== k-means quality  dim={DIM}  N={N} ===");
        println!("{:<8} {:<8} {:<16} {:<12}", "n_list", "iters", "same-bucket %", "build ms");

        for &n_list in &[64usize, 128, 256, 512] {
            for &iters in &[20usize, 50, 100] {
                let t0 = std::time::Instant::now();
                let centroids = deterministic_kmeans(&corpus, n_list, iters);
                let build_ms = t0.elapsed().as_millis();

                // Assign every vector to its centroid bucket.
                let mut assignments: Vec<usize> = corpus.iter().map(|(_, v)| {
                    let q: Vec<i32> = v.iter().map(|&x| f32_to_q16(x)).collect();
                    nearest_centroid_f32(&centroids, &q)
                }).collect();

                // Build the inverted list for lookup.
                let mut inv: Vec<Vec<u32>> = vec![Vec::new(); centroids.len()];
                for (id, bucket) in assignments.iter().enumerate() {
                    inv[*bucket].push(id as u32);
                }

                // For a sample of vectors, check if the true NN lands in the same bucket.
                let sample = 500.min(N);
                let mut same_bucket = 0usize;
                for i in 0..sample {
                    let (query_id, query_vec) = &corpus[i];
                    let nn_id = corpus.iter()
                        .filter(|(id, _)| *id != *query_id)
                        .map(|(id, v)| {
                            let d: f32 = v.iter().zip(query_vec).map(|(a, b)| (a - b) * (a - b)).sum();
                            (*id, d)
                        })
                        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                        .map(|(id, _)| id)
                        .unwrap();
                    let my_bucket = assignments[i];
                    if inv[my_bucket].contains(&nn_id) {
                        same_bucket += 1;
                    }
                }

                println!("{:<8} {:<8} {:<15.1}% {:<12}",
                    n_list, iters, same_bucket as f64 / sample as f64 * 100.0, build_ms);
            }
        }
    }

    // ── Diagnostic 2: n_list sweep ────────────────────────────────────────────
    // Fixed n_probe = n_list / 4. Sweeps n_list to find the sweet spot.
    // cargo test -p valori-node --lib --release ivf_nlist_sweep -- --nocapture --ignored
    #[test]
    #[ignore]
    fn ivf_nlist_sweep() {
        const DIM: usize = 384;
        const N: usize = 25_000;
        const K: usize = 10;
        const N_QUERIES: usize = 100;

        let corpus: Vec<(u32, Vec<f32>)> = (0..N)
            .map(|i| (i as u32, rand_vec(DIM, i as u64 * 7 + 1)))
            .collect();
        let queries: Vec<Vec<f32>> = (0..N_QUERIES)
            .map(|i| rand_vec(DIM, i as u64 * 31 + 999_999))
            .collect();
        let truths: Vec<std::collections::HashSet<u32>> = queries.iter()
            .map(|q| brute_top_k(&corpus, q, K))
            .collect();

        println!("\n=== IVF n_list sweep  dim={DIM}  N={N}  k={K}  n_probe=n_list/4 ===");
        println!("{:<8} {:<8} {:<12} {:<10} {:<10}", "n_list", "n_probe", "Recall@10", "p50 µs", "build ms");

        for &n_list in &[64usize, 128, 256, 512] {
            let n_probe = (n_list / 4).max(1);
            let mut idx = IvfIndex::new(IvfConfig {
                n_list, n_probe, auto_scale: false,
            }, DIM);
            let t0 = std::time::Instant::now();
            idx.build(&corpus);
            let build_ms = t0.elapsed().as_millis();

            // warm-up
            for q in &queries { idx.search(q, K); }

            let mut recall_sum = 0.0f64;
            let mut times: Vec<u128> = Vec::with_capacity(N_QUERIES);
            for (q, truth) in queries.iter().zip(&truths) {
                let t0 = std::time::Instant::now();
                let hits = idx.search(q, K);
                times.push(t0.elapsed().as_micros());
                let found = hits.iter().filter(|(id, _)| truth.contains(id)).count();
                recall_sum += found as f64 / K as f64;
            }
            times.sort_unstable();
            println!("{:<8} {:<8} {:<11.1}% {:<10} {:<10}",
                n_list, n_probe, recall_sum / N_QUERIES as f64 * 100.0,
                times[N_QUERIES / 2], build_ms);
        }
    }

    // ── Diagnostic 3: k-means iteration sweep ─────────────────────────────────
    // Recall@10 vs iteration count — isolates whether 20 iters is the bottleneck.
    // cargo test -p valori-node --lib --release ivf_kmeans_iter_sweep -- --nocapture --ignored
    #[test]
    #[ignore]
    fn ivf_kmeans_iter_sweep() {
        const DIM: usize = 384;
        const N: usize = 25_000;
        const N_LIST: usize = 158;  // auto-scale value for N=25k
        const N_PROBE: usize = 32;  // fixed probe count for fair comparison
        const K: usize = 10;
        const N_QUERIES: usize = 100;

        let corpus: Vec<(u32, Vec<f32>)> = (0..N)
            .map(|i| (i as u32, rand_vec(DIM, i as u64 * 7 + 1)))
            .collect();
        let queries: Vec<Vec<f32>> = (0..N_QUERIES)
            .map(|i| rand_vec(DIM, i as u64 * 31 + 999_999))
            .collect();
        let truths: Vec<std::collections::HashSet<u32>> = queries.iter()
            .map(|q| brute_top_k(&corpus, q, K))
            .collect();

        println!("\n=== k-means iteration sweep  dim={DIM}  N={N}  n_list={N_LIST}  n_probe={N_PROBE}  k={K} ===");
        println!("{:<8} {:<12} {:<10} {:<10}", "iters", "Recall@10", "p50 µs", "build ms");

        for &iters in &[5usize, 10, 20, 50, 100] {
            // Build index manually so we can control iteration count.
            let centroids = deterministic_kmeans(&corpus, N_LIST, iters);
            let t0 = std::time::Instant::now();
            let mut inverted_lists: Vec<Vec<(u32, Vec<i32>)>> = vec![Vec::new(); centroids.len()];
            for (id, vec) in &corpus {
                let q: Vec<i32> = vec.iter().map(|&v| f32_to_q16(v)).collect();
                let best = nearest_centroid_f32(&centroids, &q);
                inverted_lists[best].push((*id, q));
            }
            let build_ms = t0.elapsed().as_millis();

            let mut idx = IvfIndex {
                config: IvfConfig { n_list: N_LIST, n_probe: N_PROBE, auto_scale: false },
                dim: DIM,
                centroids,
                inverted_lists,
                n_at_last_build: N,
            };

            // warm-up
            for q in &queries { idx.search(q, K); }

            let mut recall_sum = 0.0f64;
            let mut times: Vec<u128> = Vec::with_capacity(N_QUERIES);
            for (q, truth) in queries.iter().zip(&truths) {
                let t0 = std::time::Instant::now();
                let hits = idx.search(q, K);
                times.push(t0.elapsed().as_micros());
                let found = hits.iter().filter(|(id, _)| truth.contains(id)).count();
                recall_sum += found as f64 / K as f64;
            }
            times.sort_unstable();
            println!("{:<8} {:<11.1}% {:<10} {:<10}",
                iters, recall_sum / N_QUERIES as f64 * 100.0,
                times[N_QUERIES / 2], build_ms);
        }
    }

    // ── Diagnostic 4: cluster balance ─────────────────────────────────────────
    // Min/avg/max/stddev of bucket sizes. Unbalanced = bad quantizer.
    // cargo test -p valori-node --lib --release ivf_cluster_balance -- --nocapture --ignored
    #[test]
    #[ignore]
    fn ivf_cluster_balance() {
        const DIM: usize = 384;
        const N: usize = 25_000;

        let corpus: Vec<(u32, Vec<f32>)> = (0..N)
            .map(|i| (i as u32, rand_vec(DIM, i as u64 * 7 + 1)))
            .collect();

        println!("\n=== cluster balance  dim={DIM}  N={N} ===");
        println!("{:<8} {:<8} {:<8} {:<8} {:<8} {:<8} {:<8}",
            "n_list", "iters", "min", "avg", "max", "stddev", "empty");

        for &n_list in &[64usize, 128, 256, 512] {
            for &iters in &[20usize, 50, 100] {
                let centroids = deterministic_kmeans(&corpus, n_list, iters);
                let mut counts = vec![0usize; centroids.len()];
                for (_, vec) in &corpus {
                    let q: Vec<i32> = vec.iter().map(|&v| f32_to_q16(v)).collect();
                    let best = nearest_centroid_f32(&centroids, &q);
                    counts[best] += 1;
                }
                let min = *counts.iter().min().unwrap();
                let max = *counts.iter().max().unwrap();
                let avg = N as f64 / centroids.len() as f64;
                let empty = counts.iter().filter(|&&c| c == 0).count();
                let variance = counts.iter()
                    .map(|&c| { let d = c as f64 - avg; d * d })
                    .sum::<f64>() / centroids.len() as f64;
                println!("{:<8} {:<8} {:<8} {:<8.1} {:<8} {:<8.1} {:<8}",
                    n_list, iters, min, avg, max, variance.sqrt(), empty);
            }
        }
    }

    // ── Export vectors for FAISS comparison ───────────────────────────────────
    // Writes /tmp/valori_ivf_bench.bin — then run benchmarks/ivf_faiss_compare.py
    // cargo test -p valori-node --lib --release ivf_export_bench_vectors -- --nocapture --ignored
    #[test]
    #[ignore]
    fn ivf_export_bench_vectors() {
        use std::io::Write;
        const DIM: usize = 384;
        const N_CORPUS: usize = 25_000;
        const N_QUERIES: usize = 100;
        let corpus: Vec<Vec<f32>> = (0..N_CORPUS)
            .map(|i| rand_vec(DIM, i as u64 * 7 + 1))
            .collect();
        let queries: Vec<Vec<f32>> = (0..N_QUERIES)
            .map(|i| rand_vec(DIM, i as u64 * 31 + 999_999))
            .collect();
        let path = "/tmp/valori_ivf_bench.bin";
        let mut f = std::fs::File::create(path).unwrap();
        let hdr: [u32; 3] = [DIM as u32, N_CORPUS as u32, N_QUERIES as u32];
        for &v in &hdr { f.write_all(&v.to_le_bytes()).unwrap(); }
        for vec in corpus.iter().chain(queries.iter()) {
            for &v in vec { f.write_all(&v.to_le_bytes()).unwrap(); }
        }
        println!("Wrote {N_CORPUS} corpus + {N_QUERIES} query vectors (dim={DIM}) to {path}");
        println!("Run: python3 benchmarks/ivf_faiss_compare.py");
    }
}
