// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! ExecutionHistory — durable record of completed executions and their receipts.
//!
//! This module is a stub in Phase A4. The full `ExecutionRecord` type is defined
//! here; the query and insert functions will be wired in Phase A7 when the
//! runtime and `ReceiptAssembler` are implemented.
use serde::{Deserialize, Serialize};

/// How long to retain the logical `ExecutionGraph` after completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRetentionPolicy {
    /// Seconds to retain the logical graph. 0 = retain indefinitely.
    pub logical_graph_ttl_secs: u64,
}

impl Default for ExecutionRetentionPolicy {
    fn default() -> Self {
        ExecutionRetentionPolicy {
            logical_graph_ttl_secs: 30 * 24 * 3600, // 30 days
        }
    }
}

/// The final status of an execution, as recorded in history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Complete,
    Failed,
    Cancelled,
}

/// A completed execution record stored in the MetadataDb.
///
/// Written by the runtime after `ReceiptAssembler` seals the `Receipt`.
/// The `graph_json` field stores the logical `ExecutionGraph` for replay / cache
/// reuse; it is pruned after `retention.logical_graph_ttl_secs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    /// UUID string for the execution.
    pub execution_id: String,
    /// BLAKE3 hex of the `Operation` this execution fulfilled.
    pub operation_hash: String,
    /// BLAKE3 hex of the `ExecutionGraph`.
    pub graph_hash: String,
    /// Terminal status.
    pub status: ExecutionStatus,
    /// BLAKE3 hex of the `Receipt`, if the execution completed successfully.
    pub receipt_hash: Option<String>,
    /// Unix seconds when the execution completed.
    pub completed_at: u64,
    /// How long to retain the logical graph JSON.
    pub retention: ExecutionRetentionPolicy,
    /// JSON-encoded `ExecutionGraph` — pruned when `logical_graph_ttl_secs` elapses.
    pub graph_json: Option<String>,
}

impl ExecutionRecord {
    /// Returns `true` when the logical graph JSON should be purged based on
    /// `completed_at + retention.logical_graph_ttl_secs <= now`.
    pub fn is_graph_expired(&self, now_secs: u64) -> bool {
        let ttl = self.retention.logical_graph_ttl_secs;
        if ttl == 0 {
            return false; // retain indefinitely
        }
        self.completed_at.saturating_add(ttl) <= now_secs
    }
}
