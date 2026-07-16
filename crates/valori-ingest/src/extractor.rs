// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`Extractor`] trait — bytes-in, [`Document`]-out.
//!
//! Separates the I/O concern (opening a file, fetching a URL) from the parsing
//! concern (decoding PDF/DOCX/HTML bytes into text + metadata). The Reader
//! handles I/O; the Extractor handles parsing.
//!
//! # Relationship to `Reader`
//!
//! ```text
//! DocumentSource ──► Reader ──► bytes ──► Extractor ──► Document ──► pipeline
//! ```
//!
//! `Reader` implementations may delegate to `Extractor` internally. Both traits
//! exist independently so either can be used without the other.

use crate::document::{Document, IngestError};

/// Converts raw bytes into a [`Document`].
///
/// Implementations are synchronous: all I/O is done before calling `extract`.
/// For CPU-bound formats (PDF, DOCX), callers should wrap the call in
/// `tokio::task::spawn_blocking`.
pub trait Extractor: Send + Sync {
    fn extract(&self, bytes: &[u8], source: Option<&str>) -> Result<Document, IngestError>;

    /// Static capability description for this extractor.
    fn capabilities(&self) -> ReaderCapabilities;
}

// ── Capabilities ──────────────────────────────────────────────────────────────

/// Static description of what a reader / extractor can handle.
///
/// Used by the desktop, CLI, and API to enumerate supported formats without
/// hardcoding knowledge of individual reader types.
#[derive(Debug, Clone)]
pub struct ReaderCapabilities {
    /// Lowercase file extensions handled (without dot).
    pub extensions: Vec<&'static str>,
    /// MIME types handled.
    pub mime_types: Vec<&'static str>,
    /// True if the extractor can process bytes in a streaming fashion.
    pub supports_streaming: bool,
    /// True if the extractor surfaces title, author, page count, etc.
    pub supports_metadata: bool,
    /// True if the extractor extracts embedded images.
    pub supports_images: bool,
}
