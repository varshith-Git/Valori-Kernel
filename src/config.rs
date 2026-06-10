// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Configuration constants.

/// Number of fractional bits for Fixed-Point representation (Q16.16).
pub const FRAC_BITS: u32 = 16;

/// Scaling factor for Fixed-Point representation (1 << FRAC_BITS).
pub const SCALE: i32 = 1 << FRAC_BITS;

/// Maximum size in bytes for record metadata.
pub const MAX_METADATA_SIZE: usize = 65536;
