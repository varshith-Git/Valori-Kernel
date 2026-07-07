// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! 1-bit Binary Quantization index — f32 node-level implementation.
//!
//! Stage 1: binarize each dimension (> 0 → 1), pack into u64 words, scan via
//!          Hamming distance (XOR + popcount).
//! Stage 2: re-rank top candidates with exact f32 L2.

use super::index::VectorIndex;
use std::collections::HashMap;

const POOL_FACTOR: usize = 10;
const MIN_CANDIDATES: usize = 200;

pub struct BqIndex {
    dim: usize,
    words_per_vec: usize,
    codes: HashMap<u32, Vec<u64>>,
    vectors: HashMap<u32, Vec<f32>>,
}

impl BqIndex {
    pub fn new() -> Self {
        Self {
            dim: 0,
            words_per_vec: 0,
            codes: HashMap::new(),
            vectors: HashMap::new(),
        }
    }

    fn binarize(vec: &[f32]) -> Vec<u64> {
        let words = (vec.len() + 63) / 64;
        let mut code = vec![0u64; words];
        for (i, &v) in vec.iter().enumerate() {
            if v > 0.0 {
                code[i / 64] |= 1u64 << (i % 64);
            }
        }
        code
    }

    #[inline]
    fn hamming(a: &[u64], b: &[u64]) -> u32 {
        a.iter().zip(b.iter()).map(|(x, y)| (x ^ y).count_ones()).sum()
    }

    #[inline]
    fn l2_sq(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b.iter()).map(|(x, y)| (x - y) * (x - y)).sum()
    }
}

impl VectorIndex for BqIndex {
    fn build(&mut self, records: &[(u32, Vec<f32>)]) {
        self.codes.clear();
        self.vectors.clear();
        if let Some((_, first)) = records.first() {
            self.dim = first.len();
            self.words_per_vec = (self.dim + 63) / 64;
        }
        for (id, vec) in records {
            self.codes.insert(*id, Self::binarize(vec));
            self.vectors.insert(*id, vec.clone());
        }
    }

    fn insert(&mut self, id: u32, vec: &[f32]) {
        if self.dim == 0 && !vec.is_empty() {
            self.dim = vec.len();
            self.words_per_vec = (self.dim + 63) / 64;
        }
        self.codes.insert(id, Self::binarize(vec));
        self.vectors.insert(id, vec.to_vec());
    }

    fn delete(&mut self, id: u32) {
        self.codes.remove(&id);
        self.vectors.remove(&id);
    }

    fn search(&self, query: &[f32], k: usize) -> Vec<(u32, f32)> {
        if k == 0 || self.codes.is_empty() {
            return Vec::new();
        }

        let query_code = Self::binarize(query);
        let candidates_cap = (POOL_FACTOR * k).max(MIN_CANDIDATES);

        // Stage 1: coarse Hamming scan
        let mut candidates: Vec<(u32, u32)> = self
            .codes
            .iter()
            .map(|(&id, code)| (Self::hamming(&query_code, code), id))
            .collect();

        candidates.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        candidates.truncate(candidates_cap);

        // Stage 2: exact L2 rescore
        let mut results: Vec<(u32, f32)> = candidates
            .iter()
            .filter_map(|&(_, id)| {
                self.vectors.get(&id).map(|v| (id, Self::l2_sq(query, v)))
            })
            .collect();

        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.cmp(&b.0)));
        results.truncate(k);
        results
    }

    fn snapshot(&self) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Vec::new())
    }

    fn restore(&mut self, _data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}
