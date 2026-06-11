// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use super::index::VectorIndex;
use super::deterministic::kmeans::{deterministic_kmeans, l2_sq_q16, f32_to_q16};
use serde::{Serialize, Deserialize};


#[derive(Serialize, Deserialize, Clone)]
pub struct IvfConfig {
    pub n_list: usize,
    pub n_probe: usize,
}

impl Default for IvfConfig {
    fn default() -> Self {
        Self { n_list: 100, n_probe: 5 }
    }
}

/// IVF index with Q16.16 fixed-point centroids and stored vectors.
///
/// All internal distance computation uses i64 integer arithmetic — no f32 in
/// the hot path.  f32 values are converted to Q16.16 once at the public
/// boundary (build / insert / search) and never touched again.
pub struct IvfIndex {
    pub config: IvfConfig,
    pub dim: usize,
    /// Centroids in Q16.16 fixed-point.
    pub centroids: Vec<Vec<i32>>,
    /// Inverted lists: per-centroid list of (record_id, Q16.16 vector).
    pub inverted_lists: Vec<Vec<(u32, Vec<i32>)>>,
}

impl IvfIndex {
    pub fn new(config: IvfConfig, dim: usize) -> Self {
        Self { config, dim, centroids: Vec::new(), inverted_lists: Vec::new() }
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
        // Convert to Q16.16 for kmeans — centroids come back as Vec<Vec<i32>>.
        self.centroids = deterministic_kmeans(records, self.config.n_list, 20);
        self.inverted_lists = vec![Vec::new(); self.centroids.len()];
        for (id, vec) in records {
            let q_vec: Vec<i32> = vec.iter().map(|&v| f32_to_q16(v)).collect();
            let (c_idx, _) = self.find_nearest_centroid(&q_vec);
            self.inverted_lists[c_idx].push((*id, q_vec));
        }
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
        self.config = dump.config;
        self.centroids = dump.centroids;
        self.inverted_lists = dump.inverted_lists;
        self.dim = if self.centroids.is_empty() { 0 } else { self.centroids[0].len() };
        Ok(())
    }
}
