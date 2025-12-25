use crate::types::scalar::FxpScalar;
// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::fxp::ops::{fxp_add, fxp_sub, fxp_mul, from_f32, to_f32};
use crate::config::SCALE;

const EPSILON: f32 = 1.0 / (SCALE as f32);

#[test]
fn test_fxp_conversions() {
    let f = 1.0;
    let s = from_f32(f);
    assert_eq!(s.0, SCALE);
    assert!((to_f32(s) - f).abs() <= EPSILON);

    let f = -2.5;
    let s = from_f32(f);
    assert_eq!(s.0, -2 * SCALE - SCALE / 2);
    assert!((to_f32(s) - f).abs() <= EPSILON);
}

#[test]
fn test_fxp_add() {
    let a = from_f32(1.5);
    let b = from_f32(2.25);
    let c = fxp_add(a, b);
    assert!((to_f32(c) - 3.75).abs() <= EPSILON);
}

#[test]
fn test_fxp_sub() {
    let a = from_f32(3.5);
    let b = from_f32(1.25);
    let c = fxp_sub(a, b);
    assert!((to_f32(c) - 2.25).abs() <= EPSILON);
}

#[test]
fn test_fxp_mul() {
    let a = from_f32(2.0);
    let b = from_f32(3.0);
    let c = fxp_mul(a, b);
    assert!((to_f32(c) - 6.0).abs() <= EPSILON);

    let a = from_f32(0.5);
    let b = from_f32(0.5);
    let c = fxp_mul(a, b);
    assert!((to_f32(c) - 0.25).abs() <= EPSILON);
    
    // Test negative
    let a = from_f32(-2.0);
    let b = from_f32(3.0);
    let c = fxp_mul(a, b);
    assert!((to_f32(c) - -6.0).abs() <= EPSILON);
}

#[test]
fn test_fxp_saturation() {
    let max = FxpScalar(i32::MAX);
    let one = FxpScalar(SCALE);
    
    // Add saturation
    let sat_add = fxp_add(max, one);
    assert_eq!(sat_add, FxpScalar(i32::MAX));

    let min = FxpScalar(i32::MIN);
    let sat_sub = fxp_sub(min, one);
    assert_eq!(sat_sub, FxpScalar(i32::MIN));

    // Mul saturation
    let large = from_f32(30000.0); // 30000 * 30000 = 900,000,000 -> fits in i32
    // But let's try larger: 50000.0 * 50000.0 = 2,500,000,000 -> exceeds i32 (2.14B)
    let big = from_f32(50000.0);
    let sat_mul = fxp_mul(big, big);
    assert_eq!(sat_mul, FxpScalar(i32::MAX)); // Should saturate positive

    let neg_big = from_f32(-50000.0);
    let sat_mul_neg = fxp_mul(big, neg_big);
    assert_eq!(sat_mul_neg, FxpScalar(i32::MIN)); // Should saturate negative
}
