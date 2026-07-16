// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Execution record types — E4.1 / E4.8.
//!
//! Every `IngestPipeline::run_observed()` call produces a [`PipelineResult`]
//! containing per-stage timing, metrics, and the writes. This is the contract
//! the daemon, desktop, and CLI consume to build execution explorers and audit
//! trails.

use serde::{Deserialize, Serialize};

use crate::document::WriteResult;

// ── Stage identity ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageName {
    Reader,
    Validator,
    Chunker,
    Embedder,
    Writer,
}

impl std::fmt::Display for StageName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StageName::Reader    => write!(f, "reader"),
            StageName::Validator => write!(f, "validator"),
            StageName::Chunker   => write!(f, "chunker"),
            StageName::Embedder  => write!(f, "embedder"),
            StageName::Writer    => write!(f, "writer"),
        }
    }
}

impl StageName {
    /// User-facing label for execution-explorer UIs — describes the
    /// operation, never the implementation (no crate/struct names).
    pub fn label(&self) -> &'static str {
        match self {
            StageName::Reader    => "Read document",
            StageName::Validator => "Validate document",
            StageName::Chunker   => "Chunk document",
            StageName::Embedder  => "Generate embeddings",
            StageName::Writer    => "Write vectors",
        }
    }
}

// ── Per-stage metrics ─────────────────────────────────────────────────────────

/// Format-specific counters emitted by each stage. E4.1.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum StageMetrics {
    Reader {
        bytes_read: usize,
        mime: String,
    },
    Validator {
        /// Number of checks that were evaluated.
        checks_run: usize,
        warnings: Vec<String>,
    },
    Chunker {
        chunks_created: usize,
        avg_chunk_bytes: usize,
        max_chunk_bytes: usize,
    },
    Embedder {
        /// Number of embed-batch calls made (1 per batch).
        batch_count: usize,
        dimensions: usize,
        /// Wall-clock latency of all embed calls combined, milliseconds.
        latency_ms: u64,
        /// Provider kind (e.g. `"ollama"`, `"openai"`), parsed from the
        /// first embedding's `model_id` (`"{provider}/{model}"`).
        provider: String,
        /// Model name (e.g. `"nomic-embed-text"`).
        model: String,
    },
    Writer {
        records_written: usize,
        /// Chunk graph nodes created (one per written chunk that got a node —
        /// `KernelWriter` always does; other writers may not).
        graph_nodes_created: usize,
        /// Parent→chunk edges created. Today always equal to
        /// `graph_nodes_created` (`KernelWriter` creates exactly one edge per
        /// chunk node), tracked separately since that's an implementation
        /// detail of one `Writer`, not a pipeline invariant.
        graph_edges_created: usize,
    },
}

// ── Per-stage result ──────────────────────────────────────────────────────────

/// Timing + outcome for one pipeline stage. E4.1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResult {
    pub stage: StageName,
    /// Unix epoch milliseconds when the stage started.
    pub started_at_ms: u64,
    /// Wall-clock duration of the stage.
    pub duration_ms: u64,
    pub success: bool,
    /// Non-fatal warnings produced during the stage.
    pub warnings: Vec<String>,
    /// Stage-specific counters.
    pub metrics: StageMetrics,
    /// First error message if `success == false`.
    pub error: Option<String>,
}

// ── Pipeline result ───────────────────────────────────────────────────────────

/// Complete execution record for one `run_observed()` call. E4.8.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineResult {
    /// BLAKE3 id of the document produced by the reader.
    pub document_id: String,
    pub document_source: String,
    pub document_mime: String,
    /// Ordered list of stages that were executed.
    pub stages: Vec<StageResult>,
    /// WriteResults from the writer stage (one per chunk written).
    pub writes: Vec<WriteResult>,
    pub chunks_produced: usize,
    pub records_written: usize,
    /// Total wall-clock duration across all stages.
    pub total_duration_ms: u64,
    pub success: bool,
    /// Error message if the pipeline failed.
    pub error: Option<String>,
}

impl PipelineResult {
    /// Summary string — useful for CLI output and daemon logs.
    pub fn summary(&self) -> String {
        if self.success {
            format!(
                "ok  source={} chunks={} writes={} time={}ms",
                self.document_source,
                self.chunks_produced,
                self.records_written,
                self.total_duration_ms,
            )
        } else {
            format!(
                "err source={} error={}",
                self.document_source,
                self.error.as_deref().unwrap_or("unknown"),
            )
        }
    }

    /// Stage result for the given stage, if it ran.
    pub fn stage(&self, name: &StageName) -> Option<&StageResult> {
        self.stages.iter().find(|s| &s.stage == name)
    }

    /// Total warnings across all stages.
    pub fn all_warnings(&self) -> Vec<&str> {
        self.stages.iter()
            .flat_map(|s| s.warnings.iter().map(|w| w.as_str()))
            .collect()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub(crate) fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
