//! Fixed-point L2 squared distance.

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::types::vector::FxpVector;
use crate::types::scalar::FxpScalar;
use crate::fxp::ops::{fxp_sub, fxp_mul}; // Reuse existing ops which handle saturation
use crate::fxp::ops::fxp_add;

/// Computes the squared L2 distance between two vectors.
/// ||a - b||^2
pub fn fxp_l2_sq(a: &FxpVector, b: &FxpVector) -> FxpScalar {
    let mut sum = FxpScalar::ZERO;

    let len = a.data.len().min(b.data.len());
    for i in 0..len {
        let diff = fxp_sub(a.data[i], b.data[i]);
        let sq = fxp_mul(diff, diff);
        sum = fxp_add(sum, sq);
    }
    
    sum
}
