// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Platform-wide error type and Result alias.
//!
//! `CoreError` covers only errors that can arise from core type operations
//! (currently: ID parsing). Domain-specific errors live in their own crates
//! and may wrap `CoreError` as a variant.

use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// Invalid input to a core type operation (e.g. ID parsing).
    #[error("Invalid input: {0}")]
    InvalidInput(&'static str),
}

/// Convenience `Result` alias using `CoreError`.
pub type Result<T> = core::result::Result<T, CoreError>;
