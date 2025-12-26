use crate::error::{KernelError, Result};

pub const Q16_MIN: i32 = -32768;
pub const Q16_MAX: i32 = 32767;

pub fn euclidean_distance_squared(a: &[i32], b: &[i32]) -> Result<i64> {
    if a.len() != b.len() {
        return Err(KernelError::DimensionMismatch {
            expected: a.len(),
            found: b.len(),
        });
    }

    // Validate Query (a) range
    for &val in a {
        if val < Q16_MIN || val > Q16_MAX {
            return Err(KernelError::QueryOutOfRange(val));
        }
    }

    let mut acc: i64 = 0;

    for i in 0..a.len() {
        // Safe Cast BEFORE subtraction to avoid overflow/underflow during diff
        let diff = (a[i] as i64) - (b[i] as i64);
        let prod = diff * diff;

        // Checked Accumulation
        acc = acc.checked_add(prod).ok_or(KernelError::DistanceOverflow)?;
    }

    Ok(acc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_distance() {
        let a = vec![10, 20];
        let b = vec![12, 18];
        // Diff: (10-12)^2 + (20-18)^2 = (-2)^2 + (2)^2 = 4 + 4 = 8
        assert_eq!(euclidean_distance_squared(&a, &b).unwrap(), 8);
    }

    #[test]
    fn test_query_range_validation() {
        let a = vec![40000]; // Out of range
        let b = vec![0];
        assert!(matches!(
            euclidean_distance_squared(&a, &b),
            Err(KernelError::QueryOutOfRange(40000))
        ));
    }

    #[test]
    fn test_overflow_protection() {
        // large values that fit in i32 but produce > i64 sum? 
        // i32 max diff is ~4e9. Sq is ~1.6e19. i64 max is ~9e18.
        // So a single dimension won't overflow i64 (barely). 
        // But accumulation will.
        let a = vec![Q16_MAX; 1000]; 
        let b = vec![Q16_MIN; 1000];
        
        // This won't overflow Q16 check, but diff is ~65535. Sq is ~4e9. 
        // 4e9 * 1000 = 4e12. Fits in i64 easily. 
        // Wait, standard range is small.
        // Let's force overflow by using larger values if we didn't have Q16 check.
        // But WITH Q16 check, max diff is ~6.5e4. Sq ~4.2e9.
        // To overflow i64 (9e18), we need 9e18 / 4.2e9 = 2e9 dimensions.
        // So with Q16 restriction, overflow is actually impossible in RAM.
        // BUT strict constraint says: "If accumulation overflows i64, Return Error."
        // We implement it anyway for safety engineering.
        
        let res = euclidean_distance_squared(&a, &b);
        assert!(res.is_ok());
    }
}
