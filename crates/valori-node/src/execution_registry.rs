// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `ExecutionRegistry` — bounded, in-memory store of real ingest execution
//! records.
//!
//! Replaces the fabricated DAG `GET /v1/operations/:id/execution` used to
//! return ("Make Execution Explorer Real"). No persistence, no database —
//! a fixed-capacity ring buffer (FIFO eviction). Restart the node, lose the
//! recent-execution history; that's an accepted tradeoff for an execution
//! *explorer* (what just happened), not the audit log (the BLAKE3-chained
//! `events.log` is the durable record, untouched by any of this).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use valori_ingest::execution::{PipelineResult, StageMetrics, StageName};

/// Default bounded capacity — oldest entries evicted first once exceeded.
pub const DEFAULT_CAPACITY: usize = 1024;

/// One stage, with its human-facing label alongside the full metrics —
/// enough to render either a DAG step or a timeline row from the same data.
#[derive(Debug, Clone, Serialize)]
pub struct StageView {
    /// User-facing description ("Read document", "Generate embeddings", …) —
    /// never an internal crate/struct name.
    pub label: &'static str,
    pub stage: StageName,
    pub started_at_ms: u64,
    pub duration_ms: u64,
    pub success: bool,
    pub warnings: Vec<String>,
    pub metrics: StageMetrics,
    pub error: Option<String>,
}

/// A completed ingest execution, keyed by `operation_id` — the real payload
/// for `GET /v1/operations/:id/execution`.
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionRecord {
    pub operation_id: String,
    pub document_source: String,
    pub collection: String,
    pub stages: Vec<StageView>,
    pub chunks_produced: usize,
    pub records_written: usize,
    pub total_duration_ms: u64,
    pub success: bool,
    pub error: Option<String>,
    /// Present when the operation's receipt was emitted before this record
    /// was built — always true for the standalone `/v1/ingest` path.
    pub receipt_id: Option<String>,
    pub state_hash_before: Option<String>,
    pub state_hash_after: Option<String>,
}

impl ExecutionRecord {
    pub fn from_pipeline_result(
        operation_id: String,
        collection: String,
        result: &PipelineResult,
        receipt_id: Option<String>,
        state_hash_before: Option<String>,
        state_hash_after: Option<String>,
    ) -> Self {
        let stages = result
            .stages
            .iter()
            .map(|s| StageView {
                label: s.stage.label(),
                stage: s.stage.clone(),
                started_at_ms: s.started_at_ms,
                duration_ms: s.duration_ms,
                success: s.success,
                warnings: s.warnings.clone(),
                metrics: s.metrics.clone(),
                error: s.error.clone(),
            })
            .collect();

        Self {
            operation_id,
            document_source: result.document_source.clone(),
            collection,
            stages,
            chunks_produced: result.chunks_produced,
            records_written: result.records_written,
            total_duration_ms: result.total_duration_ms,
            success: result.success,
            error: result.error.clone(),
            receipt_id,
            state_hash_before,
            state_hash_after,
        }
    }
}

/// Bounded FIFO registry of recent execution records. Cheap to construct
/// (`Default`), cheap to share (`Arc` around it, same pattern as
/// `ReceiptStore`/`TaskRegistry`).
pub struct ExecutionRegistry {
    entries: Mutex<VecDeque<Arc<ExecutionRecord>>>,
    capacity: usize,
}

impl ExecutionRegistry {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(capacity.min(4096))),
            capacity,
        }
    }

    /// Record a completed execution, evicting the oldest entry if at capacity.
    pub fn insert(&self, record: ExecutionRecord) {
        let mut entries = self.entries.lock().unwrap();
        if entries.len() >= self.capacity {
            entries.pop_front();
        }
        entries.push_back(Arc::new(record));
    }

    /// Look up by `operation_id`. `None` means either it never ran through
    /// this pipeline (e.g. a WAL-event `op-N` id from `/v1/operations`, which
    /// isn't an ingest-level operation) or it aged out of the ring buffer.
    pub fn get(&self, operation_id: &str) -> Option<Arc<ExecutionRecord>> {
        let entries = self.entries.lock().unwrap();
        entries
            .iter()
            .rev()
            .find(|r| r.operation_id == operation_id)
            .cloned()
    }
}

impl Default for ExecutionRegistry {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_result(source: &str) -> PipelineResult {
        PipelineResult {
            document_id: "doc".into(),
            document_source: source.into(),
            document_mime: "text/plain".into(),
            stages: vec![],
            writes: vec![],
            chunks_produced: 1,
            records_written: 1,
            total_duration_ms: 10,
            success: true,
            error: None,
        }
    }

    #[test]
    fn get_returns_none_for_unknown_id() {
        let registry = ExecutionRegistry::default();
        assert!(registry.get("nope").is_none());
    }

    #[test]
    fn insert_then_get_round_trips() {
        let registry = ExecutionRegistry::default();
        let record = ExecutionRecord::from_pipeline_result(
            "ingest-1".into(),
            "default".into(),
            &fake_result("doc.md"),
            Some("receipt-1".into()),
            Some("aaa".into()),
            Some("bbb".into()),
        );
        registry.insert(record);
        let got = registry.get("ingest-1").unwrap();
        assert_eq!(got.document_source, "doc.md");
        assert_eq!(got.receipt_id.as_deref(), Some("receipt-1"));
    }

    #[test]
    fn capacity_evicts_oldest_first() {
        let registry = ExecutionRegistry::new(2);
        for i in 0..3 {
            registry.insert(ExecutionRecord::from_pipeline_result(
                format!("ingest-{i}"),
                "default".into(),
                &fake_result("doc.md"),
                None,
                None,
                None,
            ));
        }
        assert!(
            registry.get("ingest-0").is_none(),
            "oldest entry should have been evicted"
        );
        assert!(registry.get("ingest-1").is_some());
        assert!(registry.get("ingest-2").is_some());
    }
}
