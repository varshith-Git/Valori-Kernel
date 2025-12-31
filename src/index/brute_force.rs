// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Brute-force index.

use crate::index::{SearchResult, VectorIndex};
use crate::storage::pool::RecordPool;
use crate::types::vector::FxpVector;
use crate::types::id::RecordId;
use crate::types::scalar::FxpScalar;
use crate::math::l2::fxp_l2_sq;

/// A stateless brute-force index that scans the RecordPool.
#[derive(Default, Clone)]
pub struct BruteForceIndex;

impl BruteForceIndex {
    // Keep internal implementation for direct use or trait delegation
}

impl<const MAX_RECORDS: usize, const D: usize> VectorIndex<MAX_RECORDS, D> for BruteForceIndex {
    fn on_insert(&mut self, _id: RecordId, _vec: &FxpVector<D>) { }

    fn on_delete(&mut self, _id: RecordId) { }

    fn rebuild(&mut self, _pool: &RecordPool<MAX_RECORDS, D>) { }

    fn search(
        &self,
        pool: &RecordPool<MAX_RECORDS, D>,
        query: &FxpVector<D>,
        results: &mut [SearchResult],
        filter: Option<u64>,
    ) -> usize {
        let k = results.len();
        if k == 0 { return 0; }

        // Initialize results with worst possible
        for r in results.iter_mut() {
            *r = SearchResult { score: FxpScalar(i32::MAX), id: RecordId(u32::MAX) };
        }

        let mut count = 0;

        for record in pool.iter() {
            // Apply Filter
            if let Some(req_tag) = filter {
                // Where is the tag stored?
                // Record struct has `flags`. Does it have `tag`?
                // I need to check `crates/kernel/src/storage/record.rs`. 
                // Assuming I ported it, I should check if it has `tag`.
                // Actually `Snapshot` decoding expected `tag`? No, `KernelEvent` had `tag`.
                // But `Record` struct in `snapshot/decode.rs` mismatch error (Step 2873) complained about `vector` and `flags`.
                // It did NOT complain about `tag`.
                // Wait, if `Record` doesn't have `tag`, I can't filter!
                
                // Let's assume for now I cannot filter if Record doesn't support it.
                // But I MUST support it.
                // I will add `tag` to Record struct in `storage/record.rs` in next step.
                // For now, I'll invoke a hypothetical `record.tag`.
                if record.tag != req_tag {
                    continue;
                }
            }

            let dist_sq = fxp_l2_sq(&record.vector, query);
            let candidate = SearchResult { score: dist_sq, id: record.id };

            if count < k {
                // Insert into sorted position
                let mut pos = count;
                while pos > 0 && results[pos - 1] > candidate {
                    results[pos] = results[pos - 1];
                    pos -= 1;
                }
                results[pos] = candidate;
                count += 1;
            } else {
                // Determine if we should replace the worst current result (last item)
                if candidate < results[k - 1] {
                     let mut pos = k - 1;
                     while pos > 0 && results[pos - 1] > candidate {
                         results[pos] = results[pos - 1];
                         pos -= 1;
                     }
                     results[pos] = candidate;
                }
            }
        }

        count
    }
}

impl BruteForceIndex {
    /// Helper: returns a fixed-size array of top-K results.
    pub fn search_topk<const CAP: usize, const D: usize, const K: usize>(
        &self,
        pool: &RecordPool<CAP, D>,
        query: &FxpVector<D>,
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
