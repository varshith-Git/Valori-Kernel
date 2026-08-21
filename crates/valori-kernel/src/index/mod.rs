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

/// The mathematical distance definition used to score vectors — distinct from
/// *how* it is evaluated (Q16.16 fixed-point here; `f32` in the node-level
/// `valori-index` crate). Only `SquaredL2` exists today; Valori's determinism
/// guarantee depends on avoiding a square root, so `SquaredL2` (not `L2`) is
/// the metric, not an implementation shortcut.
///
/// This is data now, not a hard-coded call site — see
/// `crate::math::l2::fxp_l2_sq`, which every kernel-native index still calls
/// directly regardless of this enum's value. Adding a second variant here
/// does not change any arithmetic; it only makes the choice representable
/// (per-collection config) and is unsupported until a corresponding function
/// exists and every call site is threaded through it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Metric {
    #[default]
    SquaredL2,
}

impl Metric {
    /// Wire tag used by `KernelEvent::ConfigureNamespace` and the V8 snapshot
    /// section. Append-only — never renumber.
    pub const fn as_u8(self) -> u8 {
        match self {
            Metric::SquaredL2 => 0,
        }
    }

    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Metric::SquaredL2),
            _ => None,
        }
    }
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
