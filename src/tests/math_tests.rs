use crate::types::scalar::FxpScalar;
use crate::types::vector::FxpVector;
use crate::math::dot::fxp_dot;
use crate::math::l2::fxp_l2_sq;
use crate::fxp::ops::from_f32;

#[test]
fn test_fxp_dot() {
    // [1, 0] . [0, 1] = 0
    let v1 = FxpVector { data: [FxpScalar::ONE, FxpScalar::ZERO] };
    let v2 = FxpVector { data: [FxpScalar::ZERO, FxpScalar::ONE] };
    assert_eq!(fxp_dot(&v1, &v2), FxpScalar::ZERO);

    // [1, 2] . [3, 4] = 3 + 8 = 11
    let v3 = FxpVector { data: [
        from_f32(1.0),
        from_f32(2.0),
    ]};
    let v4 = FxpVector { data: [
        from_f32(3.0),
        from_f32(4.0),
    ]};
    let dot = fxp_dot(&v3, &v4);
    // 11.0
    // Check with tolerance because of multi-step fixed point
    let expected = from_f32(11.0);
    // Integer exactness check:
    // 1.0 * 3.0 = 3.0 exactly
    // 2.0 * 4.0 = 8.0 exactly
    // Sum = 11.0 exactly
    assert_eq!(dot, expected);
}

#[test]
fn test_fxp_l2_sq() {
    // 3-4-5 triangle scaled
    // a = [0, 0]
    // b = [3, 4]
    // dist sq = 3^2 + 4^2 = 9 + 16 = 25
    let v_zero = FxpVector::new_zeros();
    let v_pt = FxpVector { data: [
        from_f32(3.0),
        from_f32(4.0),
    ]};
    
    let dist_sq = fxp_l2_sq(&v_zero, &v_pt);
    assert_eq!(dist_sq, from_f32(25.0));

    // Distance between same vectors should be 0
    assert_eq!(fxp_l2_sq(&v_pt, &v_pt), FxpScalar::ZERO);
}
