//! Fixed-Point Scalar type.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct FxpScalar(pub i32);

impl FxpScalar {
    pub const ZERO: FxpScalar = FxpScalar(0);
    pub const ONE: FxpScalar = FxpScalar(crate::fxp::qformat::SCALE);
}
