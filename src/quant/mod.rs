// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::types::vector::FxpVector;

pub trait Quantizer {
    /// Encode a full-precision vector into a compressed representation.
    type Code;

    /// Deterministically quantize a vector.
    fn encode(&self, v: &FxpVector) -> Self::Code;

    /// Decode back to approximate fixed-point vector.
    fn decode(&self, code: &Self::Code) -> FxpVector;
}

/// A no-op quantizer: identity mapping, no compression.
#[derive(Clone, Debug, Default)]
pub struct NoQuantizer;

impl Quantizer for NoQuantizer {
    type Code = FxpVector;

    fn encode(&self, v: &FxpVector) -> Self::Code {
        v.clone()
    }

    fn decode(&self, code: &Self::Code) -> FxpVector {
        code.clone()
    }
}
