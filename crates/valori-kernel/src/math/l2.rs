//! Fixed-point L2 squared distance.

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::types::vector::FxpVector;

/// Computes the squared L2 distance between two vectors as i64.
///
/// Accumulates in i64 to avoid i32 saturation at production dimensions (768+).
/// Uses raw integer subtraction — no Q16.16 shift — because the result is used
/// only for ordering, not for an absolute magnitude output.
pub fn fxp_l2_sq(a: &FxpVector, b: &FxpVector) -> i64 {
    let len = a.data.len().min(b.data.len());
    let mut sum: i64 = 0;
    for i in 0..len {
        let diff = (a.data[i].0 as i64) - (b.data[i].0 as i64);
        sum = sum.saturating_add(diff.saturating_mul(diff));
    }
    sum
}
