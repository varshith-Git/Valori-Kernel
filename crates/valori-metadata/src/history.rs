// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! ExecutionHistory — placeholder module.
//!
//! `ExecutionRetentionPolicy` is the only live type here; it is used by
//! `valori-planner` as a runtime policy field on `ExecutionGraph`.
//! Durable execution persistence (`ExecutionRecord`, `ExecutionStatus`,
//! `insert_execution`, `get_execution`) was removed: the types were defined
//! in Phase A4 but never wired to a real consumer.
use serde::{Deserialize, Serialize};

/// How long to retain the logical `ExecutionGraph` after completion.
///
/// Carried as a field on `ExecutionGraph` in `valori-planner`. Always
/// constructed with `default()` today; configurable in future when the
/// execution history endpoint is built.
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
