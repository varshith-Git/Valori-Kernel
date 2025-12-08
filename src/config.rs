//! Configuration constants.

/// Number of fractional bits for Fixed-Point representation (Q16.16).
pub const FRAC_BITS: u32 = 16;

/// Scaling factor for Fixed-Point representation (1 << FRAC_BITS).
pub const SCALE: i32 = 1 << FRAC_BITS;
