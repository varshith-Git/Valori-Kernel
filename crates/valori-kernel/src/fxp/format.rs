// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Fixed-point arithmetic format definitions — the `FxpFormat` seam.
//!
//! Phase 1.3 of the multi-node roadmap: precision is a first-class,
//! identity-defining parameter. The same event log replayed under Q8.8 vs
//! Q16.16 produces different rounding, different distances, and therefore a
//! different state hash — so the format ID lives in the wire header
//! (`valori-wire`), the snapshot header (V5+), and the state-hash domain.
//!
//! ## Status
//!
//! Only **Q16.16** is implemented by the engine today. The other formats
//! are declared so their IDs are reserved and the contract is explicit,
//! but constructing an engine with them is rejected everywhere a format
//! is parsed. Activating one later means: implement the arithmetic over
//! `Repr`/`Wide`, add fixtures, extend the test matrix — the storage and
//! hash plumbing is already format-aware and needs no migration.
//!
//! ## The `Wide` accumulator
//!
//! Dot products over dim-d vectors overflow `Repr`; every format names its
//! accumulator width explicitly. Q32.32 requires i128 — slower on most
//! targets, which is part of why formats are opt-in per use case.

/// Contract for a fixed-point arithmetic format.
///
/// EVOLUTION: `FORMAT_ID`s are append-only and never reused — they are
/// written into log headers, snapshot headers, and the hash domain.
pub trait FxpFormat {
    /// Storage representation of one scalar.
    type Repr: Copy + core::fmt::Debug;
    /// Accumulator type wide enough for a dot product over `Repr`.
    type Wide: Copy + core::fmt::Debug;
    /// Fractional bits (resolution = 2^-FRAC_BITS).
    const FRAC_BITS: u32;
    /// On-disk / hash-domain identifier. Append-only, never reused.
    const FORMAT_ID: u8;
    /// Canonical lowercase name as used in config (`VALORI_FORMAT`).
    const NAME: &'static str;
}

/// Q16.16 — i32 scalar, 16 fractional bits. The production format.
pub struct Q16_16;

impl FxpFormat for Q16_16 {
    type Repr = i32;
    type Wide = i64;
    const FRAC_BITS: u32 = 16;
    const FORMAT_ID: u8 = 1;
    const NAME: &'static str = "q16.16";
}

/// Q8.8 — i16 scalar, 8 fractional bits. Reserved for embedded/edge
/// deployments (half the memory, ~0.004 resolution). NOT yet implemented
/// by the engine.
pub struct Q8_8;

impl FxpFormat for Q8_8 {
    type Repr = i16;
    type Wide = i32;
    const FRAC_BITS: u32 = 8;
    const FORMAT_ID: u8 = 2;
    const NAME: &'static str = "q8.8";
}

/// Q32.32 — i64 scalar, 32 fractional bits. Reserved for high-precision
/// workloads (finance, scientific). NOT yet implemented by the engine.
pub struct Q32_32;

impl FxpFormat for Q32_32 {
    type Repr = i64;
    type Wide = i128;
    const FRAC_BITS: u32 = 32;
    const FORMAT_ID: u8 = 3;
    const NAME: &'static str = "q32.32";
}

/// The format the engine is compiled with. Everything that stamps a format
/// ID (snapshot header, hash domain) reads this constant; when the kernel
/// goes fully generic this becomes a type parameter instead.
pub const ACTIVE_FORMAT_ID: u8 = Q16_16::FORMAT_ID;

/// Resolve a format ID to its canonical name (known formats only).
pub fn format_name(id: u8) -> Option<&'static str> {
    match id {
        1 => Some(Q16_16::NAME),
        2 => Some(Q8_8::NAME),
        3 => Some(Q32_32::NAME),
        _ => None,
    }
}

/// Parse a config-supplied format name to its ID.
pub fn parse_format(name: &str) -> Option<u8> {
    match name.trim().to_ascii_lowercase().as_str() {
        "q16.16" => Some(Q16_16::FORMAT_ID),
        "q8.8" => Some(Q8_8::FORMAT_ID),
        "q32.32" => Some(Q32_32::FORMAT_ID),
        _ => None,
    }
}

// The trait constants and the legacy config constants must agree — the
// whole codebase still computes through `config::FRAC_BITS`.
const _: () = assert!(Q16_16::FRAC_BITS == crate::config::FRAC_BITS);
const _: () = assert!(1i32 << Q16_16::FRAC_BITS == crate::config::SCALE);
