// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Deterministic K-Means clustering over Q16.16 fixed-point vectors.
//!
//! All distance computations use i64 integer arithmetic — no f32 in the hot
//! path, so results are bit-identical across x86/ARM/WASM regardless of SIMD
//! auto-vectorization or FPU rounding modes.

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

    let q_records: Vec<(u32, Vec<i32>)> = records
        .iter()
        .map(|(id, vec)| (*id, vec.iter().map(|&v| f32_to_q16(v)).collect()))
        .collect();

    if k >= q_records.len() {
        let mut sorted = q_records.clone();
        sorted.sort_by_key(|r| r.0);
        return sorted.into_iter().map(|r| r.1).collect();
    }

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
                if d < best_dist || (d == best_dist && c_idx < best_c) {
                    best_dist = d;
                    best_c = c_idx;
                }
            }
            assignments[i] = best_c;
        }

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

/// Squared L2 distance over Q16.16 fixed-point vectors (i64 arithmetic).
pub fn l2_sq_q16(a: &[i32], b: &[i32]) -> i64 {
    valori_kernel::math::l2::l2_sq_i32(a, b)
}

/// Convert an f32 to Q16.16 fixed-point (round-to-nearest, then clamp).
pub fn f32_to_q16(val: f32) -> i32 {
    let scaled = (val * 65536.0).round();
    if scaled.is_nan() {
        0
    } else {
        scaled.clamp(i32::MIN as f32, i32::MAX as f32) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_returns_empty() {
        assert!(deterministic_kmeans(&[], 3, 10).is_empty());
    }

    #[test]
    fn k_zero_returns_empty() {
        let records = vec![(0u32, vec![1.0f32])];
        assert!(deterministic_kmeans(&records, 0, 10).is_empty());
    }

    #[test]
    fn k_ge_n_returns_all() {
        let records: Vec<(u32, Vec<f32>)> = (0..5u32).map(|i| (i, vec![i as f32, 0.0])).collect();
        let centroids = deterministic_kmeans(&records, 10, 5);
        assert_eq!(centroids.len(), 5);
    }

    #[test]
    fn deterministic_same_result() {
        let records: Vec<(u32, Vec<f32>)> = (0..50u32)
            .map(|i| (i, vec![i as f32 * 0.01, 0.0, 0.0, 0.0]))
            .collect();
        let c1 = deterministic_kmeans(&records, 5, 10);
        let c2 = deterministic_kmeans(&records, 5, 10);
        assert_eq!(c1, c2);
    }

    #[test]
    fn f32_to_q16_roundtrip() {
        let val = 1.5f32;
        let q = f32_to_q16(val);
        let back = q as f32 / 65536.0;
        assert!((back - val).abs() < 1e-4);
    }
}
