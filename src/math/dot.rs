//! Fixed-point dot product.

// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::types::vector::FxpVector;
use crate::types::scalar::FxpScalar;
use crate::fxp::qformat::FRAC_BITS;

/// Computes the dot product of two vectors using fixed-point arithmetic.
/// 
/// Uses an i64 accumulator to minimize overflow during summation,
/// but shifts each product term individually.
/// 
/// Returns a saturated result if the final sum exceeds the range of FxpScalar (i32).
pub fn fxp_dot<const D: usize>(a: &FxpVector<D>, b: &FxpVector<D>) -> FxpScalar {
    let mut sum: i64 = 0;

    for i in 0..D {
        let val_a = a.data[i].0 as i64;
        let val_b = b.data[i].0 as i64;
        
        // Multiply and shift
        let product = val_a * val_b;
        let term = product >> FRAC_BITS;
        
        sum = sum.saturating_add(term);
    }

    // Saturate result to i32
    let saturated = if sum > (i32::MAX as i64) {
        i32::MAX
    } else if sum < (i32::MIN as i64) {
        i32::MIN
    } else {
        sum as i32
    };

    FxpScalar(saturated)
}
