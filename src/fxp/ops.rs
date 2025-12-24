//! Fixed-point operations.

// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::types::scalar::FxpScalar;
use crate::fxp::qformat::{FRAC_BITS, SCALE};

/// Basic fixed-point addition with saturation.
pub fn fxp_add(a: FxpScalar, b: FxpScalar) -> FxpScalar {
    FxpScalar(a.0.saturating_add(b.0))
}

/// Basic fixed-point subtraction with saturation.
pub fn fxp_sub(a: FxpScalar, b: FxpScalar) -> FxpScalar {
    FxpScalar(a.0.saturating_sub(b.0))
}

/// Fixed-point multiplication with scaling and saturation.
pub fn fxp_mul(a: FxpScalar, b: FxpScalar) -> FxpScalar {
    let product = (a.0 as i64) * (b.0 as i64);
    // Shift right by FRAC_BITS to normalize, with rounding if needed (simple implementation just truncates/shifts)
    // We stick to the rule: "Use i64 intermediates... then shift and saturate back to i32"
    let shifted = product >> FRAC_BITS;
    
    // Manual saturation to i32 range
    let saturated = if shifted > (i32::MAX as i64) {
        i32::MAX
    } else if shifted < (i32::MIN as i64) {
        i32::MIN
    } else {
        shifted as i32
    };

    FxpScalar(saturated)
}

/// Helper to convert f32 to FxpScalar (TEST/FFI ONLY).
#[cfg(any(test, feature = "std"))]
pub fn from_f32(f: f32) -> FxpScalar {
    // Note: this uses f32 which is allowed in tests/std.
    // Core kernel logic should avoid this path.
    let raw = (f * (SCALE as f32)) as i32;
    FxpScalar(raw)
}

/// Helper to convert FxpScalar to f32 (TEST/FFI ONLY).
#[cfg(any(test, feature = "std"))]
pub fn to_f32(s: FxpScalar) -> f32 {
    (s.0 as f32) / (SCALE as f32)
}
