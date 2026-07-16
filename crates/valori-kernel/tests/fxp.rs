// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Tests for fxp/ops.rs — fixed-point arithmetic (Q16.16).

use valori_kernel::fxp::ops::{from_f32, fxp_add, fxp_mul, fxp_sub, to_f32};
use valori_kernel::types::scalar::FxpScalar;

const ONE: FxpScalar = FxpScalar(65536); // 1.0 in Q16.16
const HALF: FxpScalar = FxpScalar(32768); // 0.5
const NEG_ONE: FxpScalar = FxpScalar(-65536);
const ZERO: FxpScalar = FxpScalar(0);

// ─── fxp_add ────────────────────────────────────────────────────────────────

#[test]
fn add_basic() {
    assert_eq!(fxp_add(ONE, ONE), FxpScalar(131072)); // 2.0
}

#[test]
fn add_zero() {
    assert_eq!(fxp_add(ONE, ZERO), ONE);
    assert_eq!(fxp_add(ZERO, ONE), ONE);
}

#[test]
fn add_negative() {
    assert_eq!(fxp_add(ONE, NEG_ONE), ZERO);
}

#[test]
fn add_saturates_positive() {
    let max = FxpScalar(i32::MAX);
    let result = fxp_add(max, ONE);
    assert_eq!(result, FxpScalar(i32::MAX));
}

#[test]
fn add_saturates_negative() {
    let min = FxpScalar(i32::MIN);
    let result = fxp_add(min, NEG_ONE);
    assert_eq!(result, FxpScalar(i32::MIN));
}

// ─── fxp_sub ────────────────────────────────────────────────────────────────

#[test]
fn sub_basic() {
    assert_eq!(fxp_sub(ONE, HALF), HALF);
}

#[test]
fn sub_to_zero() {
    assert_eq!(fxp_sub(ONE, ONE), ZERO);
}

#[test]
fn sub_saturates_negative() {
    let min = FxpScalar(i32::MIN);
    let result = fxp_sub(min, ONE);
    assert_eq!(result, FxpScalar(i32::MIN));
}

#[test]
fn sub_saturates_positive() {
    let max = FxpScalar(i32::MAX);
    let result = fxp_sub(max, NEG_ONE);
    assert_eq!(result, FxpScalar(i32::MAX));
}

// ─── fxp_mul ────────────────────────────────────────────────────────────────

#[test]
fn mul_one_is_identity() {
    assert_eq!(fxp_mul(ONE, ONE), ONE);
    assert_eq!(fxp_mul(HALF, ONE), HALF);
}

#[test]
fn mul_zero() {
    assert_eq!(fxp_mul(ONE, ZERO), ZERO);
    assert_eq!(fxp_mul(ZERO, ONE), ZERO);
}

#[test]
fn mul_half_by_half() {
    // 0.5 × 0.5 = 0.25 = 16384 in Q16.16
    assert_eq!(fxp_mul(HALF, HALF), FxpScalar(16384));
}

#[test]
fn mul_negative() {
    // 1.0 × -1.0 = -1.0
    assert_eq!(fxp_mul(ONE, NEG_ONE), NEG_ONE);
    // -1.0 × -1.0 = 1.0
    assert_eq!(fxp_mul(NEG_ONE, NEG_ONE), ONE);
}

#[test]
fn mul_saturates_large() {
    // Very large values should saturate to i32 bounds rather than wrap.
    let big = FxpScalar(i32::MAX);
    let result = fxp_mul(big, big);
    // product >> 16 overflows i32 — must be clamped to i32::MAX
    assert_eq!(result, FxpScalar(i32::MAX));
}

// ─── from_f32 / to_f32 ──────────────────────────────────────────────────────

#[test]
fn from_f32_one() {
    assert_eq!(from_f32(1.0), ONE);
}

#[test]
fn from_f32_zero() {
    assert_eq!(from_f32(0.0), ZERO);
}

#[test]
fn from_f32_half() {
    assert_eq!(from_f32(0.5), HALF);
}

#[test]
fn from_f32_negative() {
    assert_eq!(from_f32(-1.0), NEG_ONE);
}

#[test]
fn from_f32_infinity_clamps() {
    assert_eq!(from_f32(f32::INFINITY), FxpScalar(i32::MAX));
    assert_eq!(from_f32(f32::NEG_INFINITY), FxpScalar(i32::MIN));
}

#[test]
fn from_f32_nan_clamps() {
    // NaN: `f > 0.0` is always false for NaN, so the else branch fires → MIN
    assert_eq!(from_f32(f32::NAN), FxpScalar(i32::MIN));
}

#[test]
fn to_f32_roundtrip() {
    let val = from_f32(0.25);
    let back = to_f32(val);
    assert!((back - 0.25).abs() < 1e-4, "got {back}");
}

#[test]
fn to_f32_negative_roundtrip() {
    let val = from_f32(-0.75);
    let back = to_f32(val);
    assert!((back - (-0.75)).abs() < 1e-4, "got {back}");
}
