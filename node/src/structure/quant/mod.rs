// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
pub mod pq;

/// Abstract interface for vector quantization.
pub trait Quantizer {
    /// Compress a high-precision vector into bytes.
    fn quantize(&self, vec: &[f32]) -> Vec<u8>;
    
    /// Decompress bytes back to vector (approximation).
    fn reconstruct(&self, data: &[u8]) -> Vec<f32>;
}

/// No-Op Quantizer (stores full f32 floats as bytes).
pub struct NoQuantizer;

impl Quantizer for NoQuantizer {
    fn quantize(&self, vec: &[f32]) -> Vec<u8> {
        // Naive serialization of f32s to bytes
        let mut out = Vec::with_capacity(vec.len() * 4);
        for val in vec {
            out.extend_from_slice(&val.to_le_bytes());
        }
        out
    }

    fn reconstruct(&self, data: &[u8]) -> Vec<f32> {
        let mut out = Vec::new();
        for chunk in data.chunks_exact(4) {
            let arr: [u8; 4] = chunk.try_into().unwrap();
            out.push(f32::from_le_bytes(arr));
        }
        out
    }
}

/// Scalar Quantizer (8-bit per dimension).
/// Maps range [-1.0, 1.0] to [0, 255].
pub struct ScalarQuantizer {}

impl Quantizer for ScalarQuantizer {
    fn quantize(&self, vec: &[f32]) -> Vec<u8> {
        vec.iter().map(|&v| {
            // Clamp to -1.0..1.0
            let clamped = v.max(-1.0).min(1.0);
            // Map to 0..255
            // (-1.0 + 1.0) / 2.0 * 255.0
            let norm = (clamped + 1.0) * 127.5;
            norm as u8
        }).collect()
    }

    fn reconstruct(&self, data: &[u8]) -> Vec<f32> {
        data.iter().map(|&b| {
            // Map 0..255 back to -1.0..1.0
            (b as f32 / 127.5) - 1.0
        }).collect()
    }
}
