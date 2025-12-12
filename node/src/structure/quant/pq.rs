use super::super::deterministic::kmeans::deterministic_kmeans;
use super::Quantizer;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct PqConfig {
    pub n_subvectors: usize,
    pub n_centroids: usize,
}

impl Default for PqConfig {
    fn default() -> Self {
        Self { n_subvectors: 4, n_centroids: 256 }
    }
}

pub struct ProductQuantizer {
    pub config: PqConfig,
    pub dim: usize,
    pub sub_dim: usize,
    pub codebooks: Vec<Vec<Vec<f32>>>, // [subvector][centroid][sub_dim]
}

impl ProductQuantizer {
    pub fn new(config: PqConfig, dim: usize) -> Self {
        let sub_dim = if dim == 0 { 0 } else { dim / config.n_subvectors };
        Self { config, dim, sub_dim, codebooks: Vec::new() }
    }

    pub fn build(&mut self, records: &[(u32, Vec<f32>)]) {
        if records.is_empty() { return; }
        self.codebooks.clear();
        for m in 0..self.config.n_subvectors {
            let start = m * self.sub_dim;
            let end = start + self.sub_dim;
            let sub_records: Vec<(u32, Vec<f32>)> = records.iter().map(|(id, vec)| {
                (*id, vec[start..end].to_vec())
            }).collect();

            let mut sorted_subs = sub_records;
            sorted_subs.sort_by_key(|(id, _)| *id);
            let centroids = deterministic_kmeans(&sorted_subs, self.config.n_centroids, 15);
            self.codebooks.push(centroids);
        }
    }

    pub fn snapshot(&self) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        #[derive(Serialize)]
        struct PqDump<'a> {
            config: &'a PqConfig,
            dim: usize,
            codebooks: &'a Vec<Vec<Vec<f32>>>,
        }
        let dump = PqDump { config: &self.config, dim: self.dim, codebooks: &self.codebooks };
        Ok(bincode::serde::encode_to_vec(&dump, bincode::config::standard())?)
    }

    pub fn restore(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[derive(Deserialize)]
        struct PqLoad {
            config: PqConfig,
            dim: usize,
            codebooks: Vec<Vec<Vec<f32>>>,
        }
        let dump: PqLoad = bincode::serde::decode_from_slice(data, bincode::config::standard())?.0;
        self.config = dump.config;
        self.dim = dump.dim;
        self.sub_dim = self.dim / self.config.n_subvectors;
        self.codebooks = dump.codebooks;
        Ok(())
    }
}

impl Quantizer for ProductQuantizer {
    fn quantize(&self, vec: &[f32]) -> Vec<u8> {
        let mut codes = Vec::with_capacity(self.config.n_subvectors);
        for m in 0..self.config.n_subvectors {
            let start = m * self.sub_dim;
            let sub_vec = &vec[start..start + self.sub_dim];
            let mut best_idx = 0usize;
            let mut best_dist = f32::MAX;
            if m < self.codebooks.len() {
                for (k, c) in self.codebooks[m].iter().enumerate() {
                    let d = l2_sq(sub_vec, c);
                    if d < best_dist {
                        best_dist = d;
                        best_idx = k;
                    }
                }
            }
            codes.push(best_idx as u8);
        }
        codes
    }

    fn reconstruct(&self, data: &[u8]) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.dim);
        for (m, &code) in data.iter().enumerate() {
            if m < self.codebooks.len() {
                let c_idx = code as usize;
                if c_idx < self.codebooks[m].len() {
                    out.extend_from_slice(&self.codebooks[m][c_idx]);
                } else {
                    out.extend(std::iter::repeat(0.0).take(self.sub_dim));
                }
            }
        }
        out
    }
}

fn l2_sq(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum()
}
