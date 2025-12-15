// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::types::vector::FxpVector;

pub trait Quantizer<const D: usize> {
    /// Encode a full-precision vector into a compressed representation.
    type Code;

    /// Deterministically quantize a vector.
    fn encode(&self, v: &FxpVector<D>) -> Self::Code;

    /// Decode back to approximate fixed-point vector.
    fn decode(&self, code: &Self::Code) -> FxpVector<D>;
}

/// A no-op quantizer: identity mapping, no compression.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoQuantizer;

impl<const D: usize> Quantizer<D> for NoQuantizer {
    type Code = FxpVector<D>;

    fn encode(&self, v: &FxpVector<D>) -> Self::Code {
        *v
    }

    fn decode(&self, code: &Self::Code) -> FxpVector<D> {
        *code
    }
}
