// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use super::index::VectorIndex;
use super::deterministic::kmeans::{deterministic_kmeans, l2_sq_q16, f32_to_q16};
use serde::{Serialize, Deserialize};


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
            let d = l2_sq_q16(q_vec, c);
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
            let mut candidates: Vec<(u32, i64)> = self.inverted_lists[0].iter()
                .map(|(id, q_vec)| (*id, l2_sq_q16(&q_query, q_vec)))
                .collect();
            candidates.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
            candidates.truncate(k);
            return candidates.into_iter().map(|(id, d)| (id, d as f32)).collect();
        }

        let mut centroid_dists: Vec<(usize, i64)> = self.centroids.iter().enumerate()
            .map(|(i, c)| (i, l2_sq_q16(&q_query, c)))
            .collect();

        // Sort by distance, break ties by index — exact integer comparison.
        centroid_dists.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

        let probes = self.config.n_probe.min(centroid_dists.len());
        let mut candidates: Vec<(u32, i64)> = Vec::new();

        for i in 0..probes {
            let c_idx = centroid_dists[i].0;
            for (id, q_vec) in &self.inverted_lists[c_idx] {
                candidates.push((*id, l2_sq_q16(&q_query, q_vec)));
            }
        }

        candidates.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        candidates.truncate(k);

        // Convert i64 Q16.16² distances back to f32 only at the output boundary.
        candidates.into_iter()
            .map(|(id, d)| (id, d as f32))
            .collect()
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
