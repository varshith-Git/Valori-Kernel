// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Composable ingest pipeline for the Valori platform.
//!
//! Four stages, each behind a trait:
//! - [`reader`]   — converts raw input into a [`Document`]
//! - [`chunker`]  — splits a [`Document`] into [`Chunk`]s
//! - [`embedder`] — turns chunks into [`Embedding`]s via a [`ModelProvider`]
//! - [`writer`]   — persists a chunk + embedding; returns a [`WriteResult`]
//!
//! [`IngestPipeline::builder()`] composes them.
//!
//! The `embed` and `handler` modules are kept for backward compatibility with
//! the node's direct embedding path.

pub mod cancel;
pub mod chunker;
pub mod config;
pub mod document;
pub mod embed;
pub mod embedder;
pub mod execution;
pub mod extractor;
pub mod extractor_registry;
pub mod extractors;
pub mod handler;
pub mod hooks;
pub mod metadata;
pub mod pipeline;
pub mod progress;
pub mod reader;
pub mod readers;
pub mod registry;
pub mod retry;
pub mod source;
pub mod validator;
pub mod writer;

// Domain objects
pub use document::{Chunk, Document, Embedding, IngestError, WriteResult};
pub use metadata::DocumentMetadata;
pub use source::DocumentSource;
pub use validator::{DocumentValidator, ValidationError, validate_utf8};

// Stage traits + canonical implementations
pub use chunker::{Chunker, DefaultChunker};
pub use embedder::{Embedder, ModelProviderEmbedder};
pub use reader::{Reader, TextReader};
pub use readers::{DocxReader, HtmlReader, MarkdownReader, PdfReader};
pub use registry::ReaderRegistry;
pub use extractor::{Extractor, ReaderCapabilities};
pub use extractors::{DocxExtractor, HtmlExtractor, MarkdownExtractor, PdfExtractor, TextExtractor};
pub use extractor_registry::ExtractorRegistry;
pub use writer::{NoopWriter, Writer};

// E4 observability
pub use cancel::CancellationToken;
pub use config::PipelineConfig;
pub use execution::{PipelineResult, StageName, StageMetrics, StageResult};
pub use hooks::{NoopHook, PipelineHook};
pub use progress::{ProgressEvent, ProgressSender};
pub use retry::RetryPolicy;

// Pipeline
pub use pipeline::{IngestPipeline, IngestPipelineBuilder};

// Backward-compatible flat re-exports (existing node call sites unchanged).
pub use chunker::{IngestChunk, chunk_content_hash, chunk_document};
pub use embed::{EmbedConfig, EmbedError, embed_batch};
pub use handler::{IngestDocumentRequest, IngestDocumentResponse, ingest_document};
