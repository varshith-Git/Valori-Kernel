// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Wrapper for raw i32 representing Q16.16.Scalar type.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct FxpScalar(pub i32);

impl FxpScalar {
    pub const ZERO: FxpScalar = FxpScalar(0);
    pub const ONE: FxpScalar = FxpScalar(crate::fxp::qformat::SCALE);
}
