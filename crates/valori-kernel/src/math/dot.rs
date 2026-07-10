// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Fixed-point dot product — scalar + SIMD paths.
//!
//! Dispatch order mirrors math/l2.rs:
//!   aarch64 → NEON  (vmull_s32 widening, 4 lanes)
//!   x86_64  → AVX2  (8 lanes) → SSE4.1 (4 lanes)
//!   fallback → scalar
//!
//! The inner kernel accumulates raw i64 products, then shifts once at the end.
//! For Q16.16 vectors with dim ≤ 65536 and values in [-32768, 32767], the max
//! pre-shift sum is 65536 × 32767² ≈ 7×10¹³ which fits comfortably in i64.

use crate::types::vector::FxpVector;
use crate::types::scalar::FxpScalar;
use crate::fxp::qformat::FRAC_BITS;

// ── public entry point ────────────────────────────────────────────────────────

pub fn fxp_dot(a: &FxpVector, b: &FxpVector) -> FxpScalar {
    let a_s = a.as_slice();
    let b_s = b.as_slice();
    let len = a_s.len().min(b_s.len());
    // SAFETY: FxpScalar is #[repr(transparent)] over i32.
    let a_i = unsafe { core::slice::from_raw_parts(a_s.as_ptr() as *const i32, len) };
    let b_i = unsafe { core::slice::from_raw_parts(b_s.as_ptr() as *const i32, len) };

    let raw = dot_i32(a_i, b_i);
    let shifted = raw >> FRAC_BITS;
    let saturated = shifted.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    FxpScalar(saturated)
}

/// Raw i32-slice dot product (pre-shift). Used by cosine-similarity callers.
#[inline(always)]
pub fn dot_i32(a: &[i32], b: &[i32]) -> i64 {
    #[cfg(target_arch = "aarch64")]
    {
        return unsafe { dot_neon(a, b) };
    }

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            return unsafe { dot_avx2(a, b) };
        }
        if is_x86_feature_detected!("sse4.1") {
            return unsafe { dot_sse41(a, b) };
        }
    }

    dot_scalar(a, b)
}

// ── scalar fallback ───────────────────────────────────────────────────────────

#[inline(always)]
fn dot_scalar(a: &[i32], b: &[i32]) -> i64 {
    let mut sum: i64 = 0;
    for i in 0..a.len() {
        sum += (a[i] as i64) * (b[i] as i64);
    }
    sum
}

// ── NEON (aarch64) ────────────────────────────────────────────────────────────

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn dot_neon(a: &[i32], b: &[i32]) -> i64 {
    use core::arch::aarch64::*;

    let len = a.len();
    let mut i = 0usize;
    let mut acc0 = vdupq_n_s64(0i64);
    let mut acc1 = vdupq_n_s64(0i64);

    while i + 4 <= len {
        let va = vld1q_s32(a.as_ptr().add(i));
        let vb = vld1q_s32(b.as_ptr().add(i));
        // vmull_s32: int32x2 × int32x2 → int64x2 (widening, no overflow)
        acc0 = vaddq_s64(acc0, vmull_s32(vget_low_s32(va),  vget_low_s32(vb)));
        acc1 = vaddq_s64(acc1, vmull_s32(vget_high_s32(va), vget_high_s32(vb)));
        i += 4;
    }

    let acc = vaddq_s64(acc0, acc1);
    let mut total = vgetq_lane_s64(acc, 0) + vgetq_lane_s64(acc, 1);

    while i < len {
        total += (a[i] as i64) * (b[i] as i64);
        i += 1;
    }
    total
}

// ── AVX2 (x86_64) ────────────────────────────────────────────────────────────

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn dot_avx2(a: &[i32], b: &[i32]) -> i64 {
    use core::arch::x86_64::*;

    let len = a.len();
    let mut i = 0usize;
    let mut acc = _mm256_setzero_si256();

    while i + 8 <= len {
        let va = _mm256_loadu_si256(a.as_ptr().add(i) as *const __m256i);
        let vb = _mm256_loadu_si256(b.as_ptr().add(i) as *const __m256i);
        // Widen i32 → i64, then _mm256_mul_epi32 multiplies lower 32 bits of each i64 lane.
        let va_lo = _mm256_cvtepi32_epi64(_mm256_castsi256_si128(va));
        let va_hi = _mm256_cvtepi32_epi64(_mm256_extracti128_si256(va, 1));
        let vb_lo = _mm256_cvtepi32_epi64(_mm256_castsi256_si128(vb));
        let vb_hi = _mm256_cvtepi32_epi64(_mm256_extracti128_si256(vb, 1));
        acc = _mm256_add_epi64(acc, _mm256_add_epi64(
            _mm256_mul_epi32(va_lo, vb_lo),
            _mm256_mul_epi32(va_hi, vb_hi),
        ));
        i += 8;
    }

    let lo128 = _mm256_castsi256_si128(acc);
    let hi128 = _mm256_extracti128_si256(acc, 1);
    let sum128 = _mm_add_epi64(lo128, hi128);
    let sum64 = _mm_add_epi64(sum128, _mm_srli_si128(sum128, 8));
    let mut total = _mm_cvtsi128_si64(sum64);

    while i < len {
        total += (a[i] as i64) * (b[i] as i64);
        i += 1;
    }
    total
}

// ── SSE4.1 (x86_64 fallback) ─────────────────────────────────────────────────

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.1")]
unsafe fn dot_sse41(a: &[i32], b: &[i32]) -> i64 {
    use core::arch::x86_64::*;

    let len = a.len();
    let mut i = 0usize;
    let mut acc = _mm_setzero_si128();

    while i + 4 <= len {
        let va = _mm_loadu_si128(a.as_ptr().add(i) as *const __m128i);
        let vb = _mm_loadu_si128(b.as_ptr().add(i) as *const __m128i);
        let va_lo = _mm_cvtepi32_epi64(va);
        let va_hi = _mm_cvtepi32_epi64(_mm_srli_si128(va, 8));
        let vb_lo = _mm_cvtepi32_epi64(vb);
        let vb_hi = _mm_cvtepi32_epi64(_mm_srli_si128(vb, 8));
        acc = _mm_add_epi64(acc, _mm_add_epi64(
            _mm_mul_epi32(va_lo, vb_lo),
            _mm_mul_epi32(va_hi, vb_hi),
        ));
        i += 4;
    }

    let hi = _mm_srli_si128(acc, 8);
    let sum = _mm_add_epi64(acc, hi);
    let mut total = _mm_cvtsi128_si64(sum);

    while i < len {
        total += (a[i] as i64) * (b[i] as i64);
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
    fn zero_dot_orthogonal() {
        // [1,0] · [0,1] = 0
        let a = make_vec(&[65536, 0]);
        let b = make_vec(&[0, 65536]);
        assert_eq!(fxp_dot(&a, &b), FxpScalar(0));
    }

    #[test]
    fn unit_self_dot() {
        // [1.0] · [1.0] in Q16.16: 65536 * 65536 >> 16 = 65536
        let a = make_vec(&[65536]);
        assert_eq!(fxp_dot(&a, &a), FxpScalar(65536));
    }

    #[test]
    fn known_dot() {
        // [2.0, 3.0] · [4.0, 5.0] = 8 + 15 = 23
        // In Q16.16: 2.0 = 131072, 3.0 = 196608, 4.0 = 262144, 5.0 = 327680
        // product pairs: 131072*262144 >> 16 = 524288 (= 8.0), 196608*327680 >> 16 = 983040 (= 15.0)
        // sum = 1507328 (= 23.0)
        let a = make_vec(&[131072, 196608]);
        let b = make_vec(&[262144, 327680]);
        assert_eq!(fxp_dot(&a, &b), FxpScalar(1507328));
    }

    #[test]
    fn large_dim_matches_scalar() {
        let dim = 384usize;
        let a_raw: alloc::vec::Vec<i32> = (0..dim).map(|i| ((i * 1337 + 42) % 30000) as i32 - 15000).collect();
        let b_raw: alloc::vec::Vec<i32> = (0..dim).map(|i| ((i * 7919 + 11) % 30000) as i32 - 15000).collect();
        let a = make_vec(&a_raw);
        let b = make_vec(&b_raw);
        // Compare SIMD result against scalar baseline
        let scalar_raw: i64 = a_raw.iter().zip(b_raw.iter()).map(|(&x, &y)| (x as i64) * (y as i64)).sum();
        let expected = FxpScalar((scalar_raw >> FRAC_BITS).clamp(i32::MIN as i64, i32::MAX as i64) as i32);
        assert_eq!(fxp_dot(&a, &b), expected, "SIMD dot must match scalar for dim={dim}");
    }

    #[test]
    fn odd_dim_tail() {
        // dim=5, exercises scalar tail
        let a = make_vec(&[65536, 65536, 65536, 65536, 65536]); // all 1.0
        let b = make_vec(&[65536, 65536, 65536, 65536, 65536]);
        // 1*1*5 = 5.0 in Q16.16 = 327680
        assert_eq!(fxp_dot(&a, &b), FxpScalar(327680));
    }
}
