// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! The `VectorIndex` trait — the single interface every index must implement.

/// Uniform interface for vector index structures.
///
/// All methods accept raw `f32` vectors; Q16.16 conversion is the index's
/// responsibility where needed (e.g. IVF centroids). Snapshot / restore use
/// an opaque `Vec<u8>` so the wire format can evolve independently of the
/// node's snapshot format.
pub trait VectorIndex {
    /// (Re-)build the index from a full record set. Called after snapshot restore
    /// and on explicit `rebuild_index` requests.
    fn build(&mut self, records: &[(u32, Vec<f32>)]);

    /// Approximate nearest-neighbor search. Returns `(record_id, l2_distance)` pairs,
    /// sorted ascending by distance, at most `k` results.
    fn search(&self, query: &[f32], k: usize) -> Vec<(u32, f32)>;

    /// Insert or update a single record. Must be O(log N) or better for live-write indexes.
    fn insert(&mut self, id: u32, vec: &[f32]);

    /// Remove a record. No-op if the id is not present.
    fn delete(&mut self, id: u32);

    /// Serialize index state to bytes for inclusion in a node snapshot.
    fn snapshot(&self) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>;

    /// Restore index state from bytes produced by `snapshot`.
    fn restore(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

/// Squared Euclidean distance between two f32 slices.
#[inline]
pub fn l2_distance_sq(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| (x - y) * (x - y)).sum()
}
