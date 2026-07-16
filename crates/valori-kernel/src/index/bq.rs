// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! 1-bit Binary Quantization (BQ) index with two-stage exact L2 rescoring.

use crate::index::{SearchResult, VectorIndex};
use crate::math::l2::fxp_l2_sq;
use crate::storage::pool::RecordPool;
use crate::types::id::RecordId;
use crate::types::vector::FxpVector;
use core::cmp::Ordering;

/// A deterministic 1-bit Binary Quantization (BQ) index with two-stage exact L2 rescoring.
///
/// Stage 1 (Coarse BQ Scan):
///   - Quantizes query into a 1-bit bitstring packed in 64-bit words.
///   - Scans the flat bitstring arena using hardware POPCNT (`Hamming Distance`).
///   - Retains top `4 * k` candidate record IDs.
///
/// Stage 2 (Exact Rescore):
///   - Fetches exact Q16.16 `FxpVector`s from `RecordPool` only for the top candidates.
///   - Computes bit-exact integer squared L2 distance (`fxp_l2_sq`).
///   - Returns the top `k` results sorted deterministically by L2 distance (breaking ties by ID).
#[derive(Default, Clone, Debug)]
pub struct BinaryQuantizationIndex {
    /// Dimension of indexed vectors.
    pub dim: usize,
    /// Number of 64-bit words per vector bitstring: `(dim + 63) / 64`.
    pub words_per_vec: usize,
    /// Flat contiguous arena storing 1-bit quantized vector bitstrings.
    /// Vector for `RecordId(id)` is located at `&codes[id as usize * words_per_vec .. (id as usize + 1) * words_per_vec]`.
    pub codes: alloc::vec::Vec<u64>,
}

impl BinaryQuantizationIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Quantize a Q16.16 vector into a packed 1-bit bitstring.
    /// Bit `i` is 1 if `vec.data[i] > 0`, 0 otherwise.
    pub fn encode_vector(&self, vec: &FxpVector) -> alloc::vec::Vec<u64> {
        let dim = vec.len();
        let words = (dim + 63) / 64;
        let mut code = alloc::vec![0u64; words];
        for (i, scalar) in vec.data.iter().enumerate() {
            if scalar.0 > 0 {
                code[i / 64] |= 1u64 << (i % 64);
            }
        }
        code
    }

    /// Compute Hamming distance between two packed u64 bitstrings using POPCNT.
    #[inline]
    pub fn hamming_distance(a: &[u64], b: &[u64]) -> u32 {
        let mut dist = 0u32;
        for (x, y) in a.iter().zip(b.iter()) {
            dist += (x ^ y).count_ones();
        }
        dist
    }

    #[inline]
    fn cmp_cand(a: &(u32, RecordId), b: &(u32, RecordId)) -> Ordering {
        match a.0.cmp(&b.0) {
            Ordering::Equal => a.1.cmp(&b.1),
            ord => ord,
        }
    }
}

impl VectorIndex for BinaryQuantizationIndex {
    fn on_insert(&mut self, id: RecordId, vec: &FxpVector) {
        if self.dim == 0 && !vec.data.is_empty() {
            self.dim = vec.len();
            self.words_per_vec = (self.dim + 63) / 64;
        }
        if self.words_per_vec == 0 || vec.len() != self.dim {
            return;
        }

        let required_words = (id.0 as usize + 1) * self.words_per_vec;
        if self.codes.len() < required_words {
            self.codes.resize(required_words, 0);
        }

        let code = self.encode_vector(vec);
        let start = id.0 as usize * self.words_per_vec;
        self.codes[start..start + self.words_per_vec].copy_from_slice(&code);
    }

    fn on_delete(&mut self, id: RecordId) {
        if self.words_per_vec == 0 {
            return;
        }
        let start = id.0 as usize * self.words_per_vec;
        if start + self.words_per_vec <= self.codes.len() {
            self.codes[start..start + self.words_per_vec].fill(0);
        }
    }

    fn rebuild(&mut self, pool: &RecordPool) {
        self.codes.clear();
        self.dim = 0;
        self.words_per_vec = 0;

        for opt in pool.raw_records() {
            if let Some(r) = opt {
                if !r.vector.data.is_empty() {
                    self.dim = r.vector.len();
                    self.words_per_vec = (self.dim + 63) / 64;
                    break;
                }
            }
        }

        if self.words_per_vec == 0 {
            return;
        }

        self.codes
            .resize(pool.total_slots() * self.words_per_vec, 0);
        for (idx, opt) in pool.raw_records().iter().enumerate() {
            if let Some(r) = opt {
                if r.is_searchable() && r.vector.len() == self.dim {
                    let code = self.encode_vector(&r.vector);
                    let start = idx * self.words_per_vec;
                    self.codes[start..start + self.words_per_vec].copy_from_slice(&code);
                }
            }
        }
    }

    fn search(
        &self,
        pool: &RecordPool,
        query: &FxpVector,
        results: &mut [SearchResult],
        filter: Option<u64>,
    ) -> usize {
        let k = results.len();
        if k == 0 {
            return 0;
        }

        for r in results.iter_mut() {
            *r = SearchResult {
                score: i64::MAX,
                id: RecordId(u32::MAX),
            };
        }

        if self.words_per_vec == 0 || query.len() != self.dim {
            return 0;
        }

        let query_code = self.encode_vector(query);
        let candidates_cap = (40 * k).max(400);
        let mut candidates: alloc::vec::Vec<(u32, RecordId)> =
            alloc::vec::Vec::with_capacity(candidates_cap + 1);

        // Stage 1: Coarse BQ Scan via Hamming Distance
        for record in pool.iter() {
            if !record.is_searchable() {
                continue;
            }
            if let Some(req_tag) = filter {
                if record.tag != req_tag {
                    continue;
                }
            }

            let start = record.id.0 as usize * self.words_per_vec;
            if start + self.words_per_vec > self.codes.len() {
                continue;
            }

            let cand_code = &self.codes[start..start + self.words_per_vec];
            let h_dist = Self::hamming_distance(&query_code, cand_code);
            let cand_item = (h_dist, record.id);

            if candidates.len() < candidates_cap {
                let pos =
                    candidates.partition_point(|x| Self::cmp_cand(x, &cand_item) == Ordering::Less);
                candidates.insert(pos, cand_item);
            } else if Self::cmp_cand(&cand_item, &candidates[candidates_cap - 1]) == Ordering::Less
            {
                let pos =
                    candidates.partition_point(|x| Self::cmp_cand(x, &cand_item) == Ordering::Less);
                candidates.pop();
                candidates.insert(pos, cand_item);
            }
        }

        // Stage 2: Exact Q16.16 Rescore
        let mut count = 0;
        for &(_, id) in &candidates {
            let record = match pool.get(id) {
                Some(r) if r.is_searchable() => r,
                _ => continue,
            };

            let dist_sq = fxp_l2_sq(&record.vector, query);
            let res = SearchResult {
                score: dist_sq,
                id: record.id,
            };

            if count < k {
                let mut pos = count;
                results[pos] = res;
                while pos > 0 && results[pos - 1] > results[pos] {
                    results.swap(pos, pos - 1);
                    pos -= 1;
                }
                count += 1;
            } else if res < results[k - 1] {
                let mut pos = k - 1;
                results[pos] = res;
                while pos > 0 && results[pos - 1] > results[pos] {
                    results.swap(pos, pos - 1);
                    pos -= 1;
                }
            }
        }

        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::scalar::FxpScalar;

    fn make_vec(vals: &[i32]) -> FxpVector {
        FxpVector {
            data: vals.iter().map(|&v| FxpScalar(v)).collect(),
        }
    }

    #[test]
    fn test_bq_encoding_and_hamming() {
        let idx = BinaryQuantizationIndex::new();
        // [10, -5, 3, -1, 0, 2] -> bits: 1, 0, 1, 0, 0, 1 -> binary 100101 -> 37
        let v1 = make_vec(&[10, -5, 3, -1, 0, 2]);
        let c1 = idx.encode_vector(&v1);
        assert_eq!(c1[0], 0b100101);

        // [ -1, 5, -3, 1, 0, -2] -> bits: 0, 1, 0, 1, 0, 0 -> binary 001010 -> 10
        let v2 = make_vec(&[-1, 5, -3, 1, 0, -2]);
        let c2 = idx.encode_vector(&v2);
        assert_eq!(c2[0], 0b001010);

        let dist = BinaryQuantizationIndex::hamming_distance(&c1, &c2);
        assert_eq!(dist, 5); // 5 bits differ
    }

    #[test]
    fn test_bq_rebuild_and_search() {
        let mut pool = RecordPool::new();
        let v1 = make_vec(&[100, 100, 100, 100]); // ID 0
        let v2 = make_vec(&[-100, -100, -100, -100]); // ID 1
        let v3 = make_vec(&[90, 90, 90, 90]); // ID 2

        pool.insert(v1, None, 0, 0).unwrap();
        pool.insert(v2, None, 0, 0).unwrap();
        pool.insert(v3, None, 0, 0).unwrap();

        let mut idx = BinaryQuantizationIndex::new();
        idx.rebuild(&pool);

        let query = make_vec(&[80, 80, 80, 80]);
        let mut results = [SearchResult::default(); 2];
        let count = idx.search(&pool, &query, &mut results, None);

        assert_eq!(count, 2);
        // ID 0 and ID 2 should be the closest (same direction as query)
        assert_eq!(results[0].id, RecordId(2)); // v3 is L2-closer to query than v1
        assert_eq!(results[1].id, RecordId(0));
    }
}
