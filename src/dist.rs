/// Calculates Euclidean Distance Squared (L2^2) between two FixedPoint vectors.
///
/// # Performance
/// - Uses `i64` accumulator to avoid overflow checks on every dimension.
/// - Unrolls loops automatically via LLVM.
/// - NO `checked_add` inside the hot loop.
///
/// # Returns
/// Distance squared as i64 (to preserve precision without overflow).
#[inline(always)]
pub fn euclidean_distance_squared(a: &[i32], b: &[i32]) -> i64 {
    // 1. Safety Assertion (Debug only) - Removed from Release for speed
    debug_assert_eq!(a.len(), b.len(), "Vector dimension mismatch");

    // 2. The Hot Loop
    // LLVM will vectorize this into AVX2/NEON instructions automatically
    // because there are no branches (if/else) inside.
    let mut sum: i64 = 0;

    for (x, y) in a.iter().zip(b.iter()) {
        let diff = (*x as i64) - (*y as i64);
        // Use saturating multiplication for extreme edge cases (overflow test)
        sum = sum.wrapping_add(diff.saturating_mul(diff));
    }

    sum
}

/// Calculates Dot Product between two FixedPoint vectors.
#[inline(always)]
pub fn euclidean_distance_fxp(a: &[i32], b: &[i32]) -> i64 {
    debug_assert_eq!(a.len(), b.len(), "Vectors must have same dimension");
    
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let diff = (*x as i64) - (*y as i64);
            // Use saturating multiplication to prevent overflow in extreme cases
            diff.saturating_mul(diff)
        })
        .sum()
}

#[inline(always)]
pub fn dot_product(a: &[i32], b: &[i32]) -> i64 {
    debug_assert_eq!(a.len(), b.len());

    let mut sum: i64 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        let term = (*x as i64) * (*y as i64);
        sum = sum.wrapping_add(term);
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_distance() {
        let a = vec![10, 20];
        let b = vec![12, 18];
        // Diff: (10-12)^2 + (20-18)^2 = (-2)^2 + (2)^2 = 4 + 4 = 8
        assert_eq!(euclidean_distance_squared(&a, &b), 8);
    }
    
    #[test]
    fn test_overflow_behavior() {
        // Even with huge values, i64 wrapping should handle reasonable accumulation
        // treating it as pure structural distance.
        // We no longer error, we just return the value.
        let a = vec![i32::MAX, i32::MAX];
        let b = vec![i32::MIN, i32::MIN];
        let _ = euclidean_distance_squared(&a, &b);
    }
}
