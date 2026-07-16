pub mod bq;
pub mod brute_force;
pub use bq::BinaryQuantizationIndex;
pub use brute_force::BruteForceIndex;

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::storage::pool::RecordPool;
use crate::types::id::RecordId;
use crate::types::vector::FxpVector;
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

/// Which kernel-native index variant is active.
///
/// Only `no_std`-compatible (fixed-point, alloc-only) variants live here.
/// `HNSW` and `IVF` are not yet implemented in the kernel; selecting them at
/// the node level maps to `BruteForce` in the kernel with an explicit log
/// warning — they are documented, not silent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexVariant {
    BruteForce,
    BinaryQuantization,
    // Hnsw,  // not yet kernel-native; node uses its own std-only HnswIndex
    // Ivf,   // not yet kernel-native; node uses its own std-only IvfIndex
}

/// Polymorphic kernel index. Wraps every `no_std`-compatible index in a single
/// enum so `KernelState` is not hard-wired to `BruteForceIndex`.
///
/// Extending: add a new variant here + a match arm in each `VectorIndex` method
/// below. The enum owns no rebuild logic — `KernelState::set_index_kind` handles
/// that so iterating the pool always happens in stable slot order.
#[derive(Clone)]
pub enum ActiveIndex {
    BruteForce(BruteForceIndex),
    BinaryQuantization(BinaryQuantizationIndex),
    // Hnsw(HnswIndex),
    // Ivf(IvfIndex),
}

impl Default for ActiveIndex {
    fn default() -> Self {
        ActiveIndex::BruteForce(BruteForceIndex::default())
    }
}

impl ActiveIndex {
    pub fn variant(&self) -> IndexVariant {
        match self {
            ActiveIndex::BruteForce(_) => IndexVariant::BruteForce,
            ActiveIndex::BinaryQuantization(_) => IndexVariant::BinaryQuantization,
        }
    }
}

impl VectorIndex for ActiveIndex {
    fn on_insert(&mut self, id: RecordId, vec: &FxpVector) {
        match self {
            ActiveIndex::BruteForce(i) => i.on_insert(id, vec),
            ActiveIndex::BinaryQuantization(i) => i.on_insert(id, vec),
        }
    }
    fn on_delete(&mut self, id: RecordId) {
        match self {
            ActiveIndex::BruteForce(i) => i.on_delete(id),
            ActiveIndex::BinaryQuantization(i) => i.on_delete(id),
        }
    }
    fn rebuild(&mut self, pool: &RecordPool) {
        match self {
            ActiveIndex::BruteForce(i) => i.rebuild(pool),
            ActiveIndex::BinaryQuantization(i) => i.rebuild(pool),
        }
    }
    fn search(
        &self,
        pool: &RecordPool,
        query: &FxpVector,
        results: &mut [SearchResult],
        filter: Option<u64>,
    ) -> usize {
        match self {
            ActiveIndex::BruteForce(i) => i.search(pool, query, results, filter),
            ActiveIndex::BinaryQuantization(i) => i.search(pool, query, results, filter),
        }
    }
}
