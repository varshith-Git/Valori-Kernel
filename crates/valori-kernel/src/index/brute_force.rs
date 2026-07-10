// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Brute-force index.

use crate::index::{SearchResult, VectorIndex};
use crate::storage::pool::RecordPool;
use crate::types::vector::FxpVector;
use crate::types::id::RecordId;
use crate::math::l2::fxp_l2_sq;
use alloc::collections::BinaryHeap;

/// A stateless brute-force index that scans the RecordPool.
#[derive(Default, Clone)]
pub struct BruteForceIndex;

impl VectorIndex for BruteForceIndex {
    fn on_insert(&mut self, _id: RecordId, _vec: &FxpVector) { }

    fn on_delete(&mut self, _id: RecordId) { }

    fn rebuild(&mut self, _pool: &RecordPool) { }

    fn search(
        &self,
        pool: &RecordPool,
        query: &FxpVector,
        results: &mut [SearchResult],
        filter: Option<u64>,
    ) -> usize {
        let k = results.len();
        if k == 0 { return 0; }

        // Max-heap (worst-on-top) of capacity k: O(log k) per candidate instead of O(k).
        // BinaryHeap<SearchResult> is a max-heap; peek() gives the *largest* score
        // (farthest record), which we evict when a closer candidate arrives.
        let mut heap: BinaryHeap<SearchResult> = BinaryHeap::with_capacity(k + 1);

        for record in pool.iter() {
            if let Some(req_tag) = filter {
                if record.tag != req_tag {
                    continue;
                }
            }

            let dist_sq = fxp_l2_sq(&record.vector, query);
            let candidate = SearchResult { score: dist_sq, id: record.id };

            if heap.len() < k {
                heap.push(candidate);
            } else if let Some(&worst) = heap.peek() {
                if candidate < worst {
                    heap.pop();
                    heap.push(candidate);
                }
            }
        }

        // Drain heap into results slice in ascending order (best first).
        let count = heap.len();
        let mut tmp: alloc::vec::Vec<SearchResult> = heap.into_iter().collect();
        tmp.sort_unstable();
        for (i, r) in tmp.into_iter().enumerate() {
            results[i] = r;
        }

        count
    }
}

impl BruteForceIndex {
    /// Helper: returns a fixed-size array of top-K results.
    pub fn search_topk<const K: usize>(
        &self,
        pool: &RecordPool,
        query: &FxpVector,
    ) -> [SearchResult; K] {
        let mut buf = [SearchResult::default(); K];
        // Use the trait method here or self implementation if we duplicated?
        // Let's call the trait method explicitly via UFCS or just impl logic?
        // To strictly avoid code dup, we could move implementation to a standalone fn or keep it here.
        // For simplicity: duplicate logic or re-use? 
        // We implemented the trait. Let's make this helper use the trait impl.
        VectorIndex::search(self, pool, query, &mut buf, None);
        buf
    }
}
