pub mod brute_force;

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::storage::pool::RecordPool;
use crate::types::vector::FxpVector;
use crate::types::id::RecordId;
use core::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SearchResult {
    // Determine sort order: Score ascending, then ID ascending (stable).
    // i64 to handle high-dimensional L2 without saturation at i32::MAX.
    pub score: i64,
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

pub trait VectorIndex {
    fn on_insert(&mut self, id: RecordId, vec: &FxpVector);
    fn on_delete(&mut self, id: RecordId);
    fn rebuild(&mut self, pool: &RecordPool);
    fn search(
        &self,
        pool: &RecordPool,
        query: &FxpVector,
        results: &mut [SearchResult],
        filter: Option<u64>,
    ) -> usize;
}
