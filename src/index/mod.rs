pub mod brute_force;

use crate::storage::pool::RecordPool;
// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::types::vector::FxpVector;
use crate::types::id::RecordId;
use crate::types::scalar::FxpScalar;
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

pub trait VectorIndex<const MAX_RECORDS: usize, const D: usize> {
    fn on_insert(&mut self, id: RecordId, vec: &FxpVector<D>);
    fn on_delete(&mut self, id: RecordId);
    fn rebuild(&mut self, pool: &RecordPool<MAX_RECORDS, D>);
    fn search(
        &self,
        pool: &RecordPool<MAX_RECORDS, D>,
        query: &FxpVector<D>,
        results: &mut [SearchResult],
    ) -> usize;
}
