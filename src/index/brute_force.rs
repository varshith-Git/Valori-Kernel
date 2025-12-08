//! Brute-force index.

use crate::storage::pool::RecordPool;
use crate::types::vector::FxpVector;
use crate::types::id::RecordId;
use crate::types::scalar::FxpScalar;
use crate::math::l2::fxp_l2_sq;
use core::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SearchResult {
    // Determine sort order: Score ascending, then ID ascending (stable).
    pub score: FxpScalar,
    pub id: RecordId,
}

impl PartialOrd for SearchResult {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SearchResult {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.score.cmp(&other.score) {
            Ordering::Equal => self.id.cmp(&other.id),
            other_ord => other_ord,
        }
    }
}

/// A stateless brute-force index that scans the RecordPool.
#[derive(Default)]
pub struct BruteForceIndex;

impl BruteForceIndex {
    /// Hook called when a record is inserted.
    pub fn on_insert<const D: usize>(&mut self, _id: RecordId, _vec: &FxpVector<D>) { }

    /// Hook called when a record is deleted.
    pub fn on_delete(&mut self, _id: RecordId) { }

    /// Rebuilds the index from the pool (if needed).
    pub fn rebuild<const CAP: usize, const D: usize>(&mut self, _pool: &RecordPool<CAP, D>) { }

    /// Searches the pool for the k nearest neighbors to `query`.
    pub fn search<const CAP: usize, const D: usize>(
        &self,
        pool: &RecordPool<CAP, D>,
        query: &FxpVector<D>,
        results: &mut [SearchResult],
    ) -> usize {
        let k = results.len();
        if k == 0 { return 0; }

        // Initialize results with worst possible
        for r in results.iter_mut() {
            *r = SearchResult { score: FxpScalar(i32::MAX), id: RecordId(u32::MAX) };
        }

        let mut count = 0;

        for record in pool.iter() {
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

    /// Helper: returns a fixed-size array of top-K results.
    /// 
    /// If fewer than K results are found, the remaining slots contain
    /// `SearchResult { score: MAX, id: MAX }`.
    pub fn search_topk<const CAP: usize, const D: usize, const K: usize>(
        &self,
        pool: &RecordPool<CAP, D>,
        query: &FxpVector<D>,
    ) -> [SearchResult; K] {
        let mut buf = [SearchResult::default(); K];
        // Initialize default is not max, so we should rely on search to init?
        // search() does init with MAX.
        self.search(pool, query, &mut buf);
        buf
    }
}
