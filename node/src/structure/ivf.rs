use super::index::VectorIndex;
use super::deterministic::kmeans::deterministic_kmeans;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

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

pub struct IvfIndex {
    pub config: IvfConfig,
    pub dim: usize,
    pub centroids: Vec<Vec<f32>>,
    pub inverted_lists: Vec<Vec<(u32, Vec<f32>)>>,
}

impl IvfIndex {
    pub fn new(config: IvfConfig, dim: usize) -> Self {
        Self { config, dim, centroids: Vec::new(), inverted_lists: Vec::new() }
    }

    fn find_nearest_centroid(&self, vec: &[f32]) -> (usize, f32) {
        if self.centroids.is_empty() { return (0, f32::MAX); }
        let mut best_idx = 0usize;
        let mut best_dist = f32::MAX;
        for (i, c) in self.centroids.iter().enumerate() {
            let d = l2_sq(vec, c);
            if d < best_dist {
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
        self.centroids = deterministic_kmeans(records, self.config.n_list, 20);
        self.inverted_lists = vec![Vec::new(); self.centroids.len()];
        for (id, vec) in records {
            let (c_idx, _) = self.find_nearest_centroid(vec);
            self.inverted_lists[c_idx].push((*id, vec.clone()));
        }
    }

    fn insert(&mut self, id: u32, vec: &[f32]) {
        if self.centroids.is_empty() {
            if self.inverted_lists.is_empty() {
                self.inverted_lists.push(Vec::new());
                self.centroids.push(vec![0.0; vec.len()]);
            }
        }
        let (c_idx, _) = self.find_nearest_centroid(vec);
        self.inverted_lists[c_idx].push((id, vec.to_vec()));
    }

    fn search(&self, query: &[f32], k: usize) -> Vec<(u32, f32)> {
        let mut centroid_dists: Vec<(usize, f32)> = self.centroids.iter().enumerate()
            .map(|(i, c)| (i, l2_sq(query, c)))
            .collect();

        centroid_dists.sort_by(|a, b| {
            a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.cmp(&b.0))
        });

        let probes = self.config.n_probe.min(centroid_dists.len());
        let mut candidates: Vec<(u32, f32)> = Vec::new();

        for i in 0..probes {
            let c_idx = centroid_dists[i].0;
            for (id, vec) in &self.inverted_lists[c_idx] {
                let dist = l2_sq(query, vec);
                candidates.push((*id, dist));
            }
        }

        candidates.sort_by(|a, b| {
            a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.cmp(&b.0))
        });

        candidates.truncate(k);
        candidates
    }

    fn snapshot(&self) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        #[derive(Serialize)]
        struct IvfDump {
            config: IvfConfig,
            centroids: Vec<Vec<f32>>,
            inverted_lists: Vec<Vec<(u32, Vec<f32>)>>,
        }

        // Make owned copies for serialization
        // Crucial: Must be sorted by ID for determinism!
        // We sort the owned copy before serializing.
        let mut sorted_lists = self.inverted_lists.clone();
        for list in &mut sorted_lists {
             list.sort_by_key(|(id, _)| *id);
        }

        let dump = IvfDump {
            config: self.config.clone(),
            centroids: self.centroids.clone(),
            inverted_lists: sorted_lists,
        };

        Ok(bincode::serde::encode_to_vec(&dump, bincode::config::standard())?)
    }

    fn restore(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[derive(Deserialize)]
        struct IvfLoad {
            config: IvfConfig,
            centroids: Vec<Vec<f32>>,
            inverted_lists: Vec<Vec<(u32, Vec<f32>)>>,
        }
        let dump: IvfLoad = bincode::serde::decode_from_slice(data, bincode::config::standard())?.0;
        self.config = dump.config;
        self.centroids = dump.centroids;
        self.inverted_lists = dump.inverted_lists;
        self.dim = if self.centroids.is_empty() { 0 } else { self.centroids[0].len() };
        Ok(())
    }
}

fn l2_sq(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum()
}
