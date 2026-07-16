// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Progress events — E4.2.
//!
//! `IngestPipeline::run_observed()` accepts an optional
//! `tokio::sync::mpsc::Sender<ProgressEvent>`. Receivers (desktop, CLI, daemon)
//! render live status without polling.

use crate::execution::StageName;

/// Coarse-grained event stream emitted during pipeline execution.
///
/// Consumers receive these in order; `Done` or `Failed` is always the last.
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    /// A stage is about to begin.
    StageStarted { stage: StageName },
    /// Chunk embed/write loop: how many chunks have been processed.
    ChunkProgress { completed: usize, total: usize },
    /// A stage finished successfully.
    StageCompleted { stage: StageName, duration_ms: u64 },
    /// Pipeline finished; carries the full result.
    Done { records_written: usize, chunks_produced: usize, total_duration_ms: u64 },
    /// Pipeline failed at a stage.
    Failed { stage: StageName, error: String },
}

impl std::fmt::Display for ProgressEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProgressEvent::StageStarted { stage } =>
                write!(f, "[{stage}] starting…"),
            ProgressEvent::ChunkProgress { completed, total } =>
                write!(f, "[embedder] {completed}/{total} chunks"),
            ProgressEvent::StageCompleted { stage, duration_ms } =>
                write!(f, "[{stage}] done ({duration_ms}ms)"),
            ProgressEvent::Done { records_written, chunks_produced, total_duration_ms } =>
                write!(f, "done chunks={chunks_produced} writes={records_written} time={total_duration_ms}ms"),
            ProgressEvent::Failed { stage, error } =>
                write!(f, "[{stage}] FAILED: {error}"),
        }
    }
}

/// Convenience alias so callers don't import `tokio::sync::mpsc` directly.
pub type ProgressSender = tokio::sync::mpsc::Sender<ProgressEvent>;

/// Silently drop the event if the channel is full or closed.
/// Pipeline execution is never blocked by a slow consumer.
pub(crate) async fn send(tx: &Option<&ProgressSender>, event: ProgressEvent) {
    if let Some(tx) = tx {
        let _ = tx.try_send(event);
    }
}
