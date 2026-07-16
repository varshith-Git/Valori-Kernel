// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Domain objects and error type shared across all ingest pipeline stages.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::metadata::DocumentMetadata;

// ── Document ──────────────────────────────────────────────────────────────────

/// Raw content + provenance entering the pipeline.
///
/// Produced by a [`crate::reader::Reader`]; stages downstream read from it and
/// never mutate it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// Deterministic BLAKE3 hex of `content` — stable across re-ingests of the
    /// same source; useful for dedup and update diffing.
    pub id: String,
    /// Human-readable origin (filename, URL, …). Defaults to `"text"`.
    pub source: String,
    /// MIME type hint set by the reader (`"text/plain"`, `"text/markdown"`,
    /// `"application/pdf"`, …).
    pub mime_type: String,
    /// Structured metadata extracted alongside content.
    /// Populated consistently by all readers and extractors.
    pub metadata: DocumentMetadata,
    /// The raw text content to be chunked and embedded.
    pub content: String,
}

impl Document {
    pub fn from_text(content: impl Into<String>, source: Option<&str>) -> Self {
        let content = content.into();
        let id = blake3_hex(content.as_bytes());
        Self {
            id,
            source: source.unwrap_or("text").to_string(),
            mime_type: "text/plain".to_string(),
            metadata: DocumentMetadata::default(),
            content,
        }
    }
}

// ── Chunk ─────────────────────────────────────────────────────────────────────

/// One segment produced by the chunker.
///
/// Typed wrapper that can carry additional fields (page, byte offsets, token
/// count, language, confidence) without changing stage signatures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    /// BLAKE3 hex of `text` — deterministic, useful for dedup.
    pub id: String,
    /// 0-based position in the chunk sequence.
    pub index: usize,
    /// Section title from tree strategy; empty string for other strategies.
    pub title: String,
    /// Text to embed.
    pub text: String,
    /// Extensible metadata (page, offset, language, …) — empty today.
    pub metadata: Value,
}

impl Chunk {
    pub fn new(index: usize, title: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let id = blake3_hex(text.as_bytes());
        Self {
            id,
            index,
            title: title.into(),
            text,
            metadata: Value::Object(Default::default()),
        }
    }
}

// ── Embedding ─────────────────────────────────────────────────────────────────

/// Dense float vector produced by the embedder for one chunk.
///
/// Carries provenance so downstream code knows which model produced it —
/// critical when models change (MiniLM 384d → Voyage 1024d → BGE 768d) and
/// old embeddings must not be mixed with new ones.
#[derive(Debug, Clone)]
pub struct Embedding {
    /// References the originating chunk.
    pub chunk_id: String,
    /// Model identifier that produced this vector (e.g. `"openai/text-embedding-3-small"`).
    pub model_id: String,
    /// Number of dimensions. Redundant with `values.len()` but lets callers
    /// validate without materialising the vector.
    pub dimensions: usize,
    pub values: Vec<f32>,
}

// ── WriteResult ───────────────────────────────────────────────────────────────

/// Outcome of persisting one chunk + embedding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteResult {
    /// Record identifier assigned by the store.
    pub record_id: String,
    /// Graph node created for this chunk, if the writer creates one
    /// (`KernelWriter` does; `NoopWriter` and other writers may not).
    #[serde(default)]
    pub chunk_node_id: Option<u32>,
}

// ── Error hierarchy ───────────────────────────────────────────────────────────

/// Errors produced by any ingest pipeline stage.
///
/// The variant identifies which stage failed, making retries, logging, and
/// API responses uniform without each stage inventing its own error type.
#[derive(Debug)]
pub enum IngestError {
    Reader(String),
    Chunk(String),
    Embed(String),
    Writer(String),
    Validation(String),
    /// The pipeline was cancelled via a [`crate::cancel::CancellationToken`].
    Cancelled,
}

impl std::fmt::Display for IngestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IngestError::Reader(s) => write!(f, "reader error: {s}"),
            IngestError::Chunk(s) => write!(f, "chunk error: {s}"),
            IngestError::Embed(s) => write!(f, "embed error: {s}"),
            IngestError::Writer(s) => write!(f, "writer error: {s}"),
            IngestError::Validation(s) => write!(f, "validation error: {s}"),
            IngestError::Cancelled => write!(f, "pipeline cancelled"),
        }
    }
}

impl std::error::Error for IngestError {}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
