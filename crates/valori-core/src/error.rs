// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Platform-wide error type and Result alias.
//!
//! `CoreError` covers only errors that can arise from core type operations
//! (ID parsing, version mismatches, unknown discriminants). Domain-specific
//! errors live in their own crates and may wrap `CoreError` as a variant.

use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// A numeric discriminant did not match any known enum variant.
    #[error("Unknown discriminant {0} for type {1}")]
    UnknownDiscriminant(u64, &'static str),

    /// An ID value exceeded its allowed range.
    #[error("ID out of range: {0}")]
    IdOutOfRange(u64),

    /// A version number is incompatible with the current implementation.
    #[error("Incompatible version: found {found}, expected {expected}")]
    IncompatibleVersion { found: u32, expected: u32 },

    /// Generic invalid-input guard — use sparingly, prefer specific variants.
    #[error("Invalid input: {0}")]
    InvalidInput(&'static str),
}

/// Convenience `Result` alias using `CoreError`.
pub type Result<T> = core::result::Result<T, CoreError>;
