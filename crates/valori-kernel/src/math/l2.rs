// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Fixed-point L2 squared distance — scalar + SIMD paths.
//!
//! Dispatch order:
//!   aarch64  → NEON  (vld1q_s32 / vsubq_s32 / vmlaq_s32, 4×i32 lanes)
//!   x86_64   → AVX2  (ymm 8×i32 lanes) when the CPU supports it at runtime
//!   fallback → portable scalar loop
//!
//! The result is the same integer value regardless of path — SIMD is purely a
//! throughput optimisation, not an approximation.  No floating-point is used.

use crate::types::vector::FxpVector;

// ── public entry point ────────────────────────────────────────────────────────

/// Squared L2 distance between two Q16.16 vectors (returns i64, no fp).
#[inline(always)]
pub fn fxp_l2_sq(a: &FxpVector, b: &FxpVector) -> i64 {
    let a = a.as_slice();
    let b = b.as_slice();
    let len = a.len().min(b.len());

    // Cast &[FxpScalar] → &[i32]; FxpScalar is #[repr(transparent)] over i32.
    let a = unsafe { core::slice::from_raw_parts(a.as_ptr() as *const i32, len) };
    let b = unsafe { core::slice::from_raw_parts(b.as_ptr() as *const i32, len) };

    l2_sq_i32(a, b)
}

/// Squared L2 distance over a raw `&[i32]` slice (shared with IVF / k-means).
#[inline(always)]
pub fn l2_sq_i32(a: &[i32], b: &[i32]) -> i64 {
    let len = a.len().min(b.len());

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: aarch64 always has NEON — it is mandatory in ARMv8-A.
        return unsafe { l2_sq_neon(&a[..len], &b[..len]) };
    }

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            return unsafe { l2_sq_avx2(&a[..len], &b[..len]) };
        }
        if is_x86_feature_detected!("sse4.1") {
            return unsafe { l2_sq_sse41(&a[..len], &b[..len]) };
        }
    }

    l2_sq_scalar(&a[..len], &b[..len])
}

// ── scalar fallback ───────────────────────────────────────────────────────────

#[inline(always)]
pub fn l2_sq_scalar(a: &[i32], b: &[i32]) -> i64 {
    let mut sum: i64 = 0;
    for i in 0..a.len() {
        let diff = (a[i] as i64) - (b[i] as i64);
        sum = sum.saturating_add(diff * diff);
    }
    sum
}

// ── NEON (aarch64 / Apple Silicon) ───────────────────────────────────────────
//
// Strategy: process 4 × i32 per iteration using:
//   vld1q_s32   – load 4 × i32
//   vsubq_s32   – subtract lanes
//   vmlaq_s32   – multiply-accumulate lanes (acc += diff * diff)
//
// We widen to i64 for the final horizontal sum to avoid overflow at dim > 512.
// Each diff fits in i32 (Q16.16 range [-32767, 32767], diff in [-65534, 65534])
// and diff² fits in i32 (max ~4.3 × 10⁹ < 2³¹), so accumulating 4 lanes per
// cycle in i32 is safe for up to 2³¹ / 4.3×10⁹ ≈ 500 iterations before we
// risk overflow.  We drain to i64 every 128 iterations (500-vector chunks) to
// stay well within range.

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn l2_sq_neon(a: &[i32], b: &[i32]) -> i64 {
    use core::arch::aarch64::*;

    // Q16.16 diffs can reach ±131072; diff² up to ~1.7×10¹⁰ — overflows i32.
    // Use vmull_s32 (widening multiply: int32x2 × int32x2 → int64x2) to stay
    // in i64 throughout.  Two int64x2_t accumulators cover 4 lanes per loop.

    let len = a.len();
    let mut i = 0usize;
    let mut acc0 = vdupq_n_s64(0i64); // lanes 0,1
    let mut acc1 = vdupq_n_s64(0i64); // lanes 2,3

    while i + 4 <= len {
        let va   = vld1q_s32(a.as_ptr().add(i));
        let vb   = vld1q_s32(b.as_ptr().add(i));
        let diff = vsubq_s32(va, vb);

        // vmull_s32: int32x2_t × int32x2_t → int64x2_t (widening)
        let sq0 = vmull_s32(vget_low_s32(diff),  vget_low_s32(diff));
        let sq1 = vmull_s32(vget_high_s32(diff), vget_high_s32(diff));
        acc0 = vaddq_s64(acc0, sq0);
        acc1 = vaddq_s64(acc1, sq1);
        i += 4;
    }

    // Horizontal sum: add two int64x2 accumulators then sum lanes.
    let acc = vaddq_s64(acc0, acc1);
    let mut total = vgetq_lane_s64(acc, 0) + vgetq_lane_s64(acc, 1);

    // Scalar tail (len % 4 elements).
    while i < len {
        let diff = (a[i] as i64) - (b[i] as i64);
        total += diff * diff;
        i += 1;
    }

    total
}

// ── AVX2 (x86_64) ────────────────────────────────────────────────────────────
//
// Strategy: 8 × i32 per iteration using 256-bit YMM registers.
//   _mm256_loadu_si256  – unaligned 256-bit load
//   _mm256_sub_epi32    – 8-lane i32 subtract
//   _mm256_mullo_epi32  – 8-lane i32 multiply (low 32 bits of product)
//   _mm256_add_epi64    – accumulate into i64 pairs after widening
//
// We widen diff*diff from i32 → i64 using _mm256_cvtepi32_epi64 on the two
// 128-bit halves to avoid i32 overflow entirely.

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn l2_sq_avx2(a: &[i32], b: &[i32]) -> i64 {
    use core::arch::x86_64::*;

    let len = a.len();
    let mut i = 0usize;

    let mut acc = _mm256_setzero_si256();

    while i + 8 <= len {
        let va = _mm256_loadu_si256(a.as_ptr().add(i) as *const __m256i);
        let vb = _mm256_loadu_si256(b.as_ptr().add(i) as *const __m256i);
        let diff = _mm256_sub_epi32(va, vb);

        // Widen each i32 diff → i64 before squaring to avoid overflow.
        // _mm256_cvtepi32_epi64 takes a 128-bit input → 256-bit i64 output.
        let lo = _mm256_cvtepi32_epi64(_mm256_castsi256_si128(diff));
        let hi = _mm256_cvtepi32_epi64(_mm256_extracti128_si256(diff, 1));
        // _mm256_mul_epi32 multiplies lower 32 bits of each 64-bit lane.
        // Since lo/hi are sign-extended i32→i64, the lower 32 bits hold the
        // full i32 value, so this correctly computes diff² as i64.
        acc = _mm256_add_epi64(acc, _mm256_add_epi64(
            _mm256_mul_epi32(lo, lo),
            _mm256_mul_epi32(hi, hi),
        ));
        i += 8;
    }

    // Horizontal sum of 4 × i64 lanes.
    let lo128 = _mm256_castsi256_si128(acc);
    let hi128 = _mm256_extracti128_si256(acc, 1);
    let sum128 = _mm_add_epi64(lo128, hi128);
    let sum64 = _mm_add_epi64(sum128, _mm_srli_si128(sum128, 8));
    let mut total = _mm_cvtsi128_si64(sum64);

    // Scalar tail.
    while i < len {
        let diff = (a[i] as i64) - (b[i] as i64);
        total += diff * diff;
        i += 1;
    }

    total
}

// ── SSE4.1 (x86_64 fallback) ─────────────────────────────────────────────────
//
// 4 × i32 per iteration using 128-bit XMM registers.

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.1")]
unsafe fn l2_sq_sse41(a: &[i32], b: &[i32]) -> i64 {
    use core::arch::x86_64::*;

    let len = a.len();
    let mut i = 0usize;
    let mut acc = _mm_setzero_si128();

    while i + 4 <= len {
        let va = _mm_loadu_si128(a.as_ptr().add(i) as *const __m128i);
        let vb = _mm_loadu_si128(b.as_ptr().add(i) as *const __m128i);
        let diff = _mm_sub_epi32(va, vb);
        // Widen i32 → i64 in two halves.
        let lo = _mm_cvtepi32_epi64(diff);
        let hi = _mm_cvtepi32_epi64(_mm_srli_si128(diff, 8));
        acc = _mm_add_epi64(acc, _mm_add_epi64(
            _mm_mul_epi32(lo, lo),
            _mm_mul_epi32(hi, hi),
        ));
        i += 4;
    }

    // Horizontal sum of 2 × i64 lanes.
    let hi = _mm_srli_si128(acc, 8);
    let sum = _mm_add_epi64(acc, hi);
    let mut total = _mm_cvtsi128_si64(sum);

    while i < len {
        let diff = (a[i] as i64) - (b[i] as i64);
        total += diff * diff;
        i += 1;
    }

    total
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::scalar::FxpScalar;

    fn make_vec(vals: &[i32]) -> FxpVector {
        FxpVector { data: vals.iter().map(|&v| FxpScalar(v)).collect() }
    }

    #[test]
    fn zero_distance() {
        let v = make_vec(&[1000, 2000, -3000, 4000]);
        assert_eq!(fxp_l2_sq(&v, &v), 0);
    }

    #[test]
    fn known_distance() {
        // diff = [1, -1, 1, -1] → sum of squares = 4
        let a = make_vec(&[1, 0, 1, 0]);
        let b = make_vec(&[0, 1, 0, 1]);
        assert_eq!(fxp_l2_sq(&a, &b), 4);
    }

    #[test]
    fn large_dim_matches_scalar() {
        // dim=384, values in Q16.16 range, compare SIMD vs scalar
        let dim = 384usize;
        let a_raw: alloc::vec::Vec<i32> = (0..dim).map(|i| ((i * 1337 + 42) % 60000) as i32 - 30000).collect();
        let b_raw: alloc::vec::Vec<i32> = (0..dim).map(|i| ((i * 7919 + 11) % 60000) as i32 - 30000).collect();
        let scalar = l2_sq_scalar(&a_raw, &b_raw);
        let simd   = l2_sq_i32(&a_raw, &b_raw);
        assert_eq!(simd, scalar, "SIMD result must match scalar for dim={dim}");
    }

    #[test]
    fn odd_dim_tail_correct() {
        // dim=5 — exercises the scalar tail path
        let a = make_vec(&[100, 200, 300, 400, 500]);
        let b = make_vec(&[0,   0,   0,   0,   0  ]);
        let expected: i64 = 100*100 + 200*200 + 300*300 + 400*400 + 500*500;
        assert_eq!(fxp_l2_sq(&a, &b), expected);
    }

    #[test]
    fn dim_128_matches_scalar() {
        let dim = 128usize;
        let a_raw: alloc::vec::Vec<i32> = (0..dim).map(|i| (i as i32 * 511) - 32000).collect();
        let b_raw: alloc::vec::Vec<i32> = (0..dim).map(|i| (i as i32 * 317) - 16000).collect();
        let scalar = l2_sq_scalar(&a_raw, &b_raw);
        let simd   = l2_sq_i32(&a_raw, &b_raw);
        assert_eq!(simd, scalar);
    }
}
