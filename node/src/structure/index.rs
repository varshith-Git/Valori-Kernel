use std::collections::HashMap;

// The Kernel exposes records as raw bytes/integers, 
// but the Node Layer can interpret them as Floats (f32) for HNSW libraries 
// IF we accept f32 non-determinism, OR we implement HNSW using fixed-point.
// For now, the user requested f32 signature in the trait for interface compatibility.

/// Abstract interface for deterministic vector indexing.
/// Implementations MUST ensure that `build` produces bit-identical structures on all platforms.
pub trait VectorIndex {
    /// Build the index from a batch of records.
    fn build(&mut self, records: &[(u32, Vec<f32>)]);

    /// Search the index for k-nearest neighbors.
    /// Returns a list of (RecordID, Score).
    /// Score interpretation is index-dependent (e.g. L2 squared distance).
    fn search(&self, query: &[f32], k: usize) -> Vec<(u32, f32)>;

    /// Insert a single item dynamically.
    fn insert(&mut self, id: u32, vec: &[f32]);

    /// Serializes the index to a binary blob.
    fn snapshot(&self) -> Vec<u8>;

    /// Restores the index from a binary blob.
    fn restore(&mut self, data: &[u8]);
}

/// A simple Brute-Force index (scan all).
/// Strictly deterministic.
pub struct BruteForceIndex {
    // We store a local copy of vectors as f32 for the shim.
    // Ideally we'd borrow from Kernel, but Kernel is no_std fixed-point.
    // This duplication is the price of the "HostShim" architecture requested.
    vectors: HashMap<u32, Vec<f32>>,
}

impl BruteForceIndex {
    pub fn new() -> Self {
        Self { vectors: HashMap::new() }
    }
}

impl VectorIndex for BruteForceIndex {
    fn build(&mut self, records: &[(u32, Vec<f32>)]) {
        for (id, vec) in records {
            self.vectors.insert(*id, vec.clone());
        }
    }

    fn insert(&mut self, id: u32, vec: &[f32]) {
        self.vectors.insert(id, vec.to_vec());
    }

    fn search(&self, query: &[f32], k: usize) -> Vec<(u32, f32)> {
        let mut scores: Vec<(u32, f32)> = self.vectors
            .iter()
            .map(|(id, vec)| {
                let dist = l2_distance_sq(query, vec);
                (*id, dist)
            })
            .collect();

        // Sort by distance ascending (lower is better)
        // Must sort by ID for tie-breaking determinism!
        scores.sort_by(|a, b| {
            a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });

        scores.truncate(k);
        scores
    }

    fn snapshot(&self) -> Vec<u8> {
        Vec::new()
    }

    fn restore(&mut self, _data: &[u8]) {
    }
}

fn l2_distance_sq(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum()
}
