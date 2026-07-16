// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
pub mod pq;

/// Abstract interface for vector quantization.
pub trait Quantizer {
    fn quantize(&self, vec: &[f32]) -> Vec<u8>;
    fn reconstruct(&self, data: &[u8]) -> Vec<f32>;
}

/// No-op quantizer: stores full f32 as little-endian bytes.
pub struct NoQuantizer;

impl Quantizer for NoQuantizer {
    fn quantize(&self, vec: &[f32]) -> Vec<u8> {
        let mut out = Vec::with_capacity(vec.len() * 4);
        for val in vec { out.extend_from_slice(&val.to_le_bytes()); }
        out
    }

    fn reconstruct(&self, data: &[u8]) -> Vec<f32> {
        data.chunks_exact(4)
            .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
            .collect()
    }
}

/// Scalar quantizer: maps [-1.0, 1.0] → [0, 255] (8-bit per dimension).
pub struct ScalarQuantizer;

impl Quantizer for ScalarQuantizer {
    fn quantize(&self, vec: &[f32]) -> Vec<u8> {
        vec.iter().map(|&v| {
            let clamped = v.max(-1.0).min(1.0);
            ((clamped + 1.0) * 127.5) as u8
        }).collect()
    }

    fn reconstruct(&self, data: &[u8]) -> Vec<f32> {
        data.iter().map(|&b| (b as f32 / 127.5) - 1.0).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_quantizer_roundtrip() {
        let q = NoQuantizer;
        let v = vec![0.5f32, -0.3, 1.0, 0.0];
        let enc = q.quantize(&v);
        let dec = q.reconstruct(&enc);
        for (a, b) in v.iter().zip(dec.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn scalar_quantizer_range() {
        let q = ScalarQuantizer;
        let v = vec![-1.0f32, 0.0, 1.0];
        let enc = q.quantize(&v);
        assert_eq!(enc[0], 0);
        assert_eq!(enc[2], 255);
        let dec = q.reconstruct(&enc);
        assert!((dec[1]).abs() < 0.02);
    }
}
