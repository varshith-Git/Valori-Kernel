// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! PlannerCache — persistent cache of `ExecutionGraph`s keyed by the
//! `(OperationHash, PlannerFingerprintHash, PlanningContextHash)` triple.
//!
//! This module is a stub in Phase A4. The cache key and entry types are defined
//! here; the Planner integration (lookup before planning, insert after planning)
//! is wired in Phase A5.
use serde::{Deserialize, Serialize};

/// The three-component cache key for the planner.
///
/// All three must match exactly to reuse a cached `ExecutionGraph`.
/// See RFC-0001 §6 for the full cache specification.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlannerCacheKey {
    /// BLAKE3 hex of `BLAKE3(kind ‖ inputs ‖ policy)`.
    pub operation_hash: String,
    /// BLAKE3 hex of the `PlannerFingerprint`.
    pub planner_fingerprint_hash: String,
    /// BLAKE3 hex of the serialized `PlanningContext`.
    pub planning_context_hash: String,
}

impl PlannerCacheKey {
    /// Serialize to a single string key for use as a redb table key.
    /// Format: `"{op_hash}:{fp_hash}:{ctx_hash}"`.
    pub fn to_db_key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.operation_hash, self.planner_fingerprint_hash, self.planning_context_hash
        )
    }
}

/// A cached planner entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerCacheEntry {
    /// JSON-encoded `ExecutionGraph`. Opaque until `valori-planner` defines the type.
    pub graph_json: String,
    /// Unix seconds when this entry was inserted.
    pub cached_at: u64,
    /// Unix seconds after which this entry expires. 0 = never.
    pub expires_at: u64,
}

impl PlannerCacheEntry {
    pub fn is_expired(&self, now_secs: u64) -> bool {
        self.expires_at != 0 && now_secs >= self.expires_at
    }
}
