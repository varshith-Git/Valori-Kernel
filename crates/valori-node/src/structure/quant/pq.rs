// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use super::super::deterministic::kmeans::{deterministic_kmeans, l2_sq_q16, f32_to_q16};
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
    /// Codebooks in Q16.16 fixed-point: [subvector][centroid][sub_dim].
    pub codebooks: Vec<Vec<Vec<i32>>>,
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
            let mut sub_records: Vec<(u32, Vec<f32>)> = records.iter()
                .map(|(id, vec)| (*id, vec[start..end].to_vec()))
                .collect();
            sub_records.sort_by_key(|(id, _)| *id);
            // deterministic_kmeans returns Vec<Vec<i32>> (Q16.16).
            let centroids = deterministic_kmeans(&sub_records, self.config.n_centroids, 15);
            self.codebooks.push(centroids);
        }
    }

    pub fn snapshot(&self) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        #[derive(Serialize)]
        struct PqDump<'a> {
            config: &'a PqConfig,
            dim: usize,
            codebooks: &'a Vec<Vec<Vec<i32>>>,
        }
        let dump = PqDump { config: &self.config, dim: self.dim, codebooks: &self.codebooks };
        Ok(bincode::serde::encode_to_vec(&dump, bincode::config::standard())?)
    }

    pub fn restore(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[derive(Deserialize)]
        struct PqLoad {
            config: PqConfig,
            dim: usize,
            codebooks: Vec<Vec<Vec<i32>>>,
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
            let sub_vec: Vec<i32> = vec[start..start + self.sub_dim]
                .iter().map(|&v| f32_to_q16(v)).collect();
            let mut best_idx = 0usize;
            let mut best_dist = i64::MAX;
            if m < self.codebooks.len() {
                for (k, c) in self.codebooks[m].iter().enumerate() {
                    let d = l2_sq_q16(&sub_vec, c);
                    if d < best_dist || (d == best_dist && k < best_idx) {
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
                    // Convert Q16.16 back to f32 only at the output boundary.
                    out.extend(self.codebooks[m][c_idx].iter().map(|&v| v as f32 / 65536.0));
                } else {
                    out.extend(std::iter::repeat(0.0).take(self.sub_dim));
                }
            }
        }
        out
    }
}
