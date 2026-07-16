//! Fixed-point operations.

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::fxp::qformat::FRAC_BITS;
#[cfg(any(test, feature = "std"))]
use crate::fxp::qformat::SCALE;
use crate::types::scalar::FxpScalar;

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

/// Canonical f32 → Q16.16 conversion (TEST/FFI ONLY).
///
/// This is the SINGLE SOURCE OF TRUTH for float-to-fixed-point conversion.
/// All external boundaries (FFI, bridges, adapters) MUST use this function
/// or replicate its exact semantics: multiply by SCALE, round to nearest,
/// clamp to i32 range.
///
/// Core kernel logic (no_std) should never call this — it only operates on
/// pre-converted FxpScalar values.
#[cfg(any(test, feature = "std"))]
pub fn from_f32(f: f32) -> FxpScalar {
    if !f.is_finite() {
        return FxpScalar(if f > 0.0 { i32::MAX } else { i32::MIN });
    }
    // Clamp in f32 domain first to keep the product in range, then cast.
    // i32::MAX as f32 rounds up to 2147483648.0; clamp to one ULP below.
    let scaled = (f * (SCALE as f32)).round();
    let clamped = scaled.clamp(i32::MIN as f32, 2_147_483_520.0_f32) as i32;
    FxpScalar(clamped)
}

/// Helper to convert FxpScalar to f32 (TEST/FFI ONLY).
#[cfg(any(test, feature = "std"))]
pub fn to_f32(s: FxpScalar) -> f32 {
    (s.0 as f32) / (SCALE as f32)
}
