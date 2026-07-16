// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Exact brute-force index — O(N) search, zero preprocessing.
//!
//! Used for small collections (< 10 k records in Auto mode) and as a correctness
//! reference for approximate indexes. Snapshot is a no-op because the engine
//! rebuilds from the record pool on restore.

use std::collections::HashMap;
use crate::traits::{VectorIndex, l2_distance_sq};

pub struct BruteForceIndex {
    vectors: HashMap<u32, Vec<f32>>,
}

impl BruteForceIndex {
    pub fn new() -> Self {
        Self { vectors: HashMap::new() }
    }

    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }
}

impl Default for BruteForceIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl VectorIndex for BruteForceIndex {
    fn build(&mut self, records: &[(u32, Vec<f32>)]) {
        self.vectors.clear();
        for (id, vec) in records {
            self.vectors.insert(*id, vec.clone());
        }
    }

    fn insert(&mut self, id: u32, vec: &[f32]) {
        self.vectors.insert(id, vec.to_vec());
    }

    fn delete(&mut self, id: u32) {
        self.vectors.remove(&id);
    }

    fn search(&self, query: &[f32], k: usize) -> Vec<(u32, f32)> {
        let mut scores: Vec<(u32, f32)> = self
            .vectors
            .iter()
            .map(|(&id, vec)| (id, l2_distance_sq(query, vec)))
            .collect();
        scores.sort_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        scores.truncate(k);
        scores
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
        let mut idx = BruteForceIndex::new();
        idx.insert(1, &[1.0, 0.0]);
        idx.insert(2, &[0.0, 1.0]);
        idx.insert(3, &[2.0, 0.0]);

        let res = idx.search(&[1.0, 0.0], 2);
        assert_eq!(res[0].0, 1);
        assert_eq!(res.len(), 2);

        idx.delete(1);
        let res2 = idx.search(&[1.0, 0.0], 2);
        assert_ne!(res2[0].0, 1);
    }

    #[test]
    fn tie_break_by_id() {
        let mut idx = BruteForceIndex::new();
        idx.insert(10, &[0.0]);
        idx.insert(5, &[0.0]);
        let res = idx.search(&[0.0], 2);
        assert_eq!(res[0].0, 5, "lower id wins on tie");
    }

    #[test]
    fn build_replaces_existing() {
        let mut idx = BruteForceIndex::new();
        idx.insert(99, &[9.0, 9.0]);
        idx.build(&[(1, vec![1.0, 0.0]), (2, vec![0.0, 1.0])]);
        assert_eq!(idx.len(), 2);
    }
}
