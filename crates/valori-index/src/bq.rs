// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! 1-bit Binary Quantization index.
//!
//! Stage 1: binarize each dimension (> 0 → 1), pack into u64 words, scan via
//!          Hamming distance (XOR + popcount).
//! Stage 2: re-rank top candidates with exact f32 L2.

use crate::traits::VectorIndex;
use std::collections::HashMap;

const DEFAULT_POOL_FACTOR: usize = 10;
const DEFAULT_MIN_CANDIDATES: usize = 200;

/// S11.3: candidate-pool size was previously hardcoded (`POOL_FACTOR=10`,
/// `MIN_CANDIDATES=200`), with no way to test whether a larger
/// pre-rerank pool improves recall without a source change. This makes
/// it a runtime config, defaulting to the exact prior constants so
/// behavior is unchanged unless explicitly overridden.
#[derive(Clone, Copy, Debug)]
pub struct BqConfig {
    pub pool_factor: usize,
    pub min_candidates: usize,
}

impl Default for BqConfig {
    fn default() -> Self {
        Self {
            pool_factor: DEFAULT_POOL_FACTOR,
            min_candidates: DEFAULT_MIN_CANDIDATES,
        }
    }
}

pub struct BqIndex {
    dim: usize,
    words_per_vec: usize,
    config: BqConfig,
    codes: HashMap<u32, Vec<u64>>,
    vectors: HashMap<u32, Vec<f32>>,
}

impl BqIndex {
    pub fn new() -> Self {
        Self::new_with_config(BqConfig::default())
    }

    pub fn new_with_config(config: BqConfig) -> Self {
        Self {
            dim: 0,
            words_per_vec: 0,
            config,
            codes: HashMap::new(),
            vectors: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.vectors.len()
    }
    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
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
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x ^ y).count_ones())
            .sum()
    }

    #[inline]
    fn l2_sq(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b.iter()).map(|(x, y)| (x - y) * (x - y)).sum()
    }
}

impl Default for BqIndex {
    fn default() -> Self {
        Self::new()
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
        let candidates_cap = (self.config.pool_factor * k).max(self.config.min_candidates);

        let mut candidates: Vec<(u32, u32)> = self
            .codes
            .iter()
            .map(|(&id, code)| (Self::hamming(&query_code, code), id))
            .collect();

        candidates.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        candidates.truncate(candidates_cap);

        let mut results: Vec<(u32, f32)> = candidates
            .iter()
            .filter_map(|&(_, id)| self.vectors.get(&id).map(|v| (id, Self::l2_sq(query, v))))
            .collect();

        results.sort_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_search_delete() {
        let mut idx = BqIndex::new();
        idx.insert(1, &[1.0, 0.0, 0.0]);
        idx.insert(2, &[-1.0, 0.0, 0.0]);
        idx.insert(3, &[1.0, 1.0, 0.0]);

        let res = idx.search(&[1.0, 0.0, 0.0], 2);
        assert_eq!(res[0].0, 1);
        assert_eq!(res.len(), 2);

        idx.delete(1);
        let res2 = idx.search(&[1.0, 0.0, 0.0], 2);
        assert!(res2.iter().all(|(id, _)| *id != 1));
    }

    #[test]
    fn empty_search_returns_empty() {
        let idx = BqIndex::new();
        assert!(idx.search(&[1.0, 0.0], 5).is_empty());
    }

    #[test]
    fn build_then_search() {
        let mut idx = BqIndex::new();
        let corpus: Vec<(u32, Vec<f32>)> = (0..50u32)
            .map(|i| (i, vec![i as f32, 0.0, 0.0, 0.0]))
            .collect();
        idx.build(&corpus);
        let res = idx.search(&[0.0, 0.0, 0.0, 0.0], 3);
        assert_eq!(res.len(), 3);
    }

    #[test]
    fn custom_config_changes_candidate_pool_without_error() {
        // S11.3: a larger pool must not change correctness on a small
        // corpus (pool always covers the whole corpus either way) — this
        // only asserts the config plumbing works end-to-end.
        let cfg = BqConfig {
            pool_factor: 50,
            min_candidates: 40,
        };
        let mut idx = BqIndex::new_with_config(cfg);
        let corpus: Vec<(u32, Vec<f32>)> = (0..30u32)
            .map(|i| (i, vec![i as f32, 0.0, 0.0, 0.0]))
            .collect();
        idx.build(&corpus);
        let res = idx.search(&[0.0, 0.0, 0.0, 0.0], 3);
        assert_eq!(res.len(), 3);
        assert_eq!(res[0].0, 0);
    }

    #[test]
    fn default_config_matches_prior_constants() {
        let cfg = BqConfig::default();
        assert_eq!(cfg.pool_factor, 10);
        assert_eq!(cfg.min_candidates, 200);
    }
}
