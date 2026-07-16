// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Wrapper for raw i32 representing Q16.16.Scalar type.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[repr(transparent)]
pub struct FxpScalar(pub i32);

impl FxpScalar {
    pub const ZERO: FxpScalar = FxpScalar(0);
    pub const ONE: FxpScalar = FxpScalar(crate::fxp::qformat::SCALE);
}
