use std::cmp::Ordering;

/// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Deterministic K-Means clustering.
///
/// Guarantees bit-identical centroids given the same inputs (sorted by ID).
///
/// # Arguments
/// * records - List of (ID, Vector) tuples.
/// * k - Number of centroids to find.
/// * iterations - Fixed number of Lloyd's iterations (default 20 recommended).
///
/// # Returns
/// * Vec<Vec<f32>> - K centroids, sorted by centroid index.
pub fn deterministic_kmeans(
    records: &[(u32, Vec<f32>)],
    k: usize,
    iterations: usize,
) -> Vec<Vec<f32>> {
    if records.is_empty() || k == 0 {
        return Vec::new();
    }

    let dim = records[0].1.len();
    // ensure all records have same dim
    for (_, v) in records.iter() {
        assert_eq!(v.len(), dim, "All vectors must share the same dimension");
    }

    // If k >= records.len(), return first k vectors deterministically sorted by id.
    if k >= records.len() {
        let mut sorted_recs: Vec<_> = records.iter().cloned().collect();
        sorted_recs.sort_by_key(|r| r.0);
        return sorted_recs.into_iter().map(|r| r.1).collect();
    }

    // Helper: deterministic FNV-1a hashing over rounded Q16.16 bytes + id
    fn hash_vec_id(id: u32, vec: &[f32]) -> u64 {
        let mut hash: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        for &val in vec {
            // round-to-nearest and clamp to i32 range
            let scaled = (val * 65536.0).round();
            let clamped = if scaled.is_nan() {
                0i32
            } else {
                let s = scaled as i64;
                let s = s.max(i32::MIN as i64).min(i32::MAX as i64);
                s as i32
            };
            for byte in clamped.to_le_bytes() {
                hash ^= byte as u64;
                hash = hash.wrapping_mul(FNV_PRIME);
            }
        }

        for byte in id.to_le_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        hash
    }

    struct ScoredRecord<'a> {
        score: u64,
        id: u32,
        vec: &'a [f32],
    }

    let mut scored: Vec<ScoredRecord<'_>> = records
        .iter()
        .map(|(id, vec)| ScoredRecord {
            score: hash_vec_id(*id, vec),
            id: *id,
            vec: vec.as_slice(),
        })
        .collect();

    // sort by hash then id deterministically
    scored.sort_by(|a, b| a.score.cmp(&b.score).then_with(|| a.id.cmp(&b.id)));

    // initial centroids: take top-k hashed records (clone vectors)
    let mut centroids: Vec<Vec<f32>> = scored
        .iter()
        .take(k)
        .map(|s| s.vec.to_vec())
        .collect();

    const SCALE: f32 = 65536.0;

    // main Lloyd iterations
    for _ in 0..iterations {
        let mut assignments = vec![0usize; records.len()];

        // assign - Deterministic tie-breaking
        for (i, (_, vec)) in records.iter().enumerate() {
            let mut best_dist = f32::MAX;
            let mut best_c = 0usize;
            for (c_idx, centroid) in centroids.iter().enumerate() {
                let d = l2_sq(vec, centroid);
                if d < best_dist {
                    best_dist = d;
                    best_c = c_idx;
                } else if (d - best_dist).abs() < f32::EPSILON {
                    // tie-break deterministically by centroid index
                    if c_idx < best_c {
                        best_c = c_idx;
                    }
                }
            }
            assignments[i] = best_c;
        }

        // accumulate using i128 for safety
        let mut sums: Vec<Vec<i128>> = vec![vec![0i128; dim]; k];
        let mut counts: Vec<usize> = vec![0usize; k];

        for (i, (_, vec)) in records.iter().enumerate() {
            let c_idx = assignments[i];
            counts[c_idx] += 1;
            for (d_idx, &val) in vec.iter().enumerate() {
                let scaled = (val * SCALE).round() as i128; // Use .round() for determinism
                sums[c_idx][d_idx] = sums[c_idx][d_idx].saturating_add(scaled);
            }
        }

        for c_idx in 0..k {
            let count = counts[c_idx];
            if count > 0 {
                for d_idx in 0..dim {
                    let avg_fxp = sums[c_idx][d_idx] / (count as i128);
                    // clamp to i32 range and convert back to f32
                    let avg_fxp_i32 = avg_fxp.max(i32::MIN as i128).min(i32::MAX as i128) as i32;
                    centroids[c_idx][d_idx] = (avg_fxp_i32 as f32) / SCALE;
                }
            }
        }
    }

    centroids
}

fn l2_sq(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum()
}
