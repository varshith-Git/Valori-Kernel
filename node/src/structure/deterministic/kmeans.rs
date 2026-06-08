
// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
/// Deterministic K-Means clustering over Q16.16 fixed-point vectors.
///
/// All distance computations use i64 integer arithmetic — no f32 in the hot
/// path, so results are bit-identical across x86/ARM/WASM regardless of SIMD
/// auto-vectorization or FPU rounding modes.
///
/// # Arguments
/// * records    - (ID, f32 vector) pairs; converted to Q16.16 at entry.
/// * k          - Number of centroids.
/// * iterations - Fixed Lloyd's iterations (20 recommended).
///
/// # Returns
/// * Vec<Vec<i32>> - K centroids in Q16.16, sorted by centroid index.
pub fn deterministic_kmeans(
    records: &[(u32, Vec<f32>)],
    k: usize,
    iterations: usize,
) -> Vec<Vec<i32>> {
    if records.is_empty() || k == 0 {
        return Vec::new();
    }

    let dim = records[0].1.len();
    for (_, v) in records.iter() {
        assert_eq!(v.len(), dim, "All vectors must share the same dimension");
    }

    // Convert all input vectors to Q16.16 once at the boundary.
    let q_records: Vec<(u32, Vec<i32>)> = records
        .iter()
        .map(|(id, vec)| (*id, vec.iter().map(|&v| f32_to_q16(v)).collect()))
        .collect();

    if k >= q_records.len() {
        let mut sorted = q_records.clone();
        sorted.sort_by_key(|r| r.0);
        return sorted.into_iter().map(|r| r.1).collect();
    }

    // Deterministic seed: FNV-1a over Q16.16 bytes + id.
    fn hash_vec_id(id: u32, vec: &[i32]) -> u64 {
        let mut hash: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;
        for &val in vec {
            for byte in val.to_le_bytes() {
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

    let mut scored: Vec<(u64, u32, &[i32])> = q_records
        .iter()
        .map(|(id, vec)| (hash_vec_id(*id, vec), *id, vec.as_slice()))
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let mut centroids: Vec<Vec<i32>> = scored.iter().take(k).map(|(_, _, v)| v.to_vec()).collect();

    for _ in 0..iterations {
        let mut assignments = vec![0usize; q_records.len()];

        for (i, (_, vec)) in q_records.iter().enumerate() {
            let mut best_dist = i64::MAX;
            let mut best_c = 0usize;
            for (c_idx, centroid) in centroids.iter().enumerate() {
                let d = l2_sq_q16(vec, centroid);
                // Strict integer comparison — no epsilon, no hardware sensitivity.
                if d < best_dist || (d == best_dist && c_idx < best_c) {
                    best_dist = d;
                    best_c = c_idx;
                }
            }
            assignments[i] = best_c;
        }

        // Accumulate in i128 to avoid overflow on large clusters.
        let mut sums: Vec<Vec<i128>> = vec![vec![0i128; dim]; k];
        let mut counts: Vec<usize> = vec![0; k];

        for (i, (_, vec)) in q_records.iter().enumerate() {
            let c = assignments[i];
            counts[c] += 1;
            for (d, &val) in vec.iter().enumerate() {
                sums[c][d] = sums[c][d].saturating_add(val as i128);
            }
        }

        for c in 0..k {
            if counts[c] > 0 {
                for d in 0..dim {
                    let avg = sums[c][d] / counts[c] as i128;
                    centroids[c][d] = avg.clamp(i32::MIN as i128, i32::MAX as i128) as i32;
                }
            }
        }
    }

    centroids
}

/// Squared L2 distance over Q16.16 fixed-point vectors.
/// Returns an i64; exact, no floating-point involved.
pub fn l2_sq_q16(a: &[i32], b: &[i32]) -> i64 {
    a.iter().zip(b).map(|(x, y)| {
        let d = (*x as i64) - (*y as i64);
        d * d
    }).sum()
}

/// Convert an f32 to Q16.16 fixed-point (deterministic: round-to-nearest, then clamp).
pub fn f32_to_q16(val: f32) -> i32 {
    let scaled = (val * 65536.0).round();
    if scaled.is_nan() {
        0
    } else {
        scaled.clamp(i32::MIN as f32, i32::MAX as f32) as i32
    }
}
