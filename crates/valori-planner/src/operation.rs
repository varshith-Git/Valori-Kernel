// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Operation — the immutable, content-addressed unit of user intent.
//!
//! An Operation captures *what* the user wants to do (kind + planning-relevant
//! parameters) but not the actual data (vectors, text). Two searches with the
//! same parameters (k=5, collection="default", rerank=true) share the same
//! OperationHash and can reuse the same cached ExecutionGraph regardless of
//! which query vector was used.
use serde::{Deserialize, Serialize};
use valori_core::id::ExecutionId;

/// Content-addressed hash of an `Operation`.
/// `OperationHash = BLAKE3(kind_discriminant ‖ bincode(inputs) ‖ bincode(policy))`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperationHash(pub [u8; 32]);

impl std::fmt::Display for OperationHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in &self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

impl OperationHash {
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

/// Unique identifier for an Operation instance.
/// Reuses `ExecutionId` (128-bit, no OS dep) so `valori-planner` stays uuid-free.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperationId(pub ExecutionId);

impl OperationId {
    pub fn new() -> Self {
        OperationId(ExecutionId::new_random())
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

// ── OperationKind ─────────────────────────────────────────────────────────────

/// The kind of work an Operation requests.
///
/// Adding a new variant is backward-compatible. Removing one is a breaking change
/// that requires a `KernelABI` bump.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    // ── Wired: endpoint → Operation → Planner → ExecutionGraph → Executor ──────
    Ingest,
    Search,
    MemoryUpsert,
    Consolidate,
    Contradict,
    HealthCheck,
    /// Hard or soft deletion of one record.
    Delete,
    /// Batch insert of multiple vectors in one HTTP call.
    BatchInsert,

    // ── Planned: endpoint exists in valori-node but still calls logic directly ──
    // These will be migrated to the planner pipeline in a future phase.
    // Do not delete — they are integration points, not dead code.
    // Migration order: Snapshot → GraphRag → TreeBuild/Query/Hybrid → CommunityDetect/Search → MemorySearch
    GraphRag,
    MemorySearch,
    CommunityDetect,
    CommunitySearch,
    TreeBuild,
    TreeQuery,
    TreeHybrid,
    Snapshot,
}

// ── OperationInputs ───────────────────────────────────────────────────────────

/// Planning-relevant parameters for each operation kind.
///
/// These are NOT the actual data (vectors, text) — those flow through task inputs
/// at runtime. These are the configuration parameters that determine the task
/// graph topology: which tasks run, in what order, on which shard.
///
/// All variants must be deterministically serializable (no HashMap, no f32/f64).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OperationInputs {
    // ── Wired ────────────────────────────────────────────────────────────────────
    Ingest {
        strategy: String,
        collection: String,
        shard_id: u8,
        embed_enabled: bool,
    },
    Search {
        k: u32,
        collection: String,
        shard_id: u8,
        rerank: bool,
        decay: bool,
        metadata_filter: bool,
        consistency: ConsistencyLevel,
    },
    MemoryUpsert {
        collection: String,
        shard_id: u8,
    },
    Consolidate {
        shard_id: u8,
    },
    Contradict {
        shard_id: u8,
    },
    HealthCheck,
    /// Hard or soft deletion of one record by id.
    Delete {
        collection: String,
        shard_id: u8,
        /// `"hard"` for permanent delete, `"soft"` for tombstone.
        mode: String,
    },
    /// Batch insert of multiple vectors in one HTTP call.
    BatchInsert {
        count: u32,
        collection: String,
        shard_id: u8,
    },

    // ── Planned: endpoint exists in valori-node but still calls logic directly ──
    // To be migrated to the planner pipeline. Migration order:
    // Snapshot → GraphRag → TreeBuild/Query/Hybrid → CommunityDetect/Search → MemorySearch
    GraphRag {
        k: u32,
        depth: u32,
        collection: String,
        shard_id: u8,
    },
    MemorySearch {
        k: u32,
        collection: String,
        shard_id: u8,
        decay: bool,
    },
    CommunityDetect {
        collection: String,
        shard_id: u8,
        max_iter: u32,
    },
    CommunitySearch {
        k: u32,
        depth: u32,
        drill_in: bool,
        collection: String,
        shard_id: u8,
    },
    TreeBuild {
        shard_id: u8,
    },
    TreeQuery {
        k: u32,
        shard_id: u8,
    },
    TreeHybrid {
        k: u32,
        shard_id: u8,
        embed_enabled: bool,
    },
    Snapshot {
        shard_id: u8,
    },
}

/// Read consistency level, affecting whether a read-index check is needed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsistencyLevel {
    Local,
    Linearizable,
}

// ── ExecutionPolicy ───────────────────────────────────────────────────────────

/// Runtime execution constraints for an Operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionPolicy {
    /// Maximum wall-clock seconds for the entire operation. 0 = no timeout.
    pub timeout_secs: u32,
    /// Maximum retry attempts per task. 0 = no retries.
    pub retry_limit: u8,
    /// Resource budget enforced by the runtime.
    pub resource_budget: ResourceBudget,
}

impl Default for ExecutionPolicy {
    fn default() -> Self {
        ExecutionPolicy {
            timeout_secs: 30,
            retry_limit: 2,
            resource_budget: ResourceBudget::default(),
        }
    }
}

/// Maximum resource consumption allowed for one Operation.
/// Enforced by the runtime; tasks that exceed limits are cancelled.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceBudget {
    /// Maximum kernel write events. 0 = unlimited.
    pub max_kernel_writes: u32,
    /// Maximum embedding provider calls. 0 = unlimited.
    pub max_embed_calls: u32,
    /// Maximum LLM token budget (prompt + completion). 0 = unlimited.
    pub max_llm_tokens: u32,
}

impl Default for ResourceBudget {
    fn default() -> Self {
        ResourceBudget {
            max_kernel_writes: 10_000,
            max_embed_calls: 100,
            max_llm_tokens: 0,
        }
    }
}

// ── Operation ─────────────────────────────────────────────────────────────────

/// An immutable, content-addressed unit of user intent.
///
/// Created by the HTTP handler from the incoming request; passed to the Planner
/// which produces an `ExecutionGraph`. The `hash` is computed at creation and
/// never changes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Operation {
    pub id: OperationId,
    pub kind: OperationKind,
    pub inputs: OperationInputs,
    pub policy: ExecutionPolicy,
    /// `BLAKE3(kind_discriminant ‖ bincode(inputs) ‖ bincode(policy))`
    pub hash: OperationHash,
    /// Unix seconds — excluded from the hash.
    pub created_at: u64,
}

impl Operation {
    /// Create a new Operation, computing its hash from kind + inputs + policy.
    pub fn new(kind: OperationKind, inputs: OperationInputs, policy: ExecutionPolicy) -> Self {
        let hash = compute_operation_hash(kind, &inputs, &policy);
        let id = OperationId::new();
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Operation {
            id,
            kind,
            inputs,
            policy,
            hash,
            created_at,
        }
    }
}

/// Compute `BLAKE3(kind_byte ‖ bincode(inputs) ‖ bincode(policy))`.
///
/// Uses bincode v2 with standard config for deterministic encoding.
/// `created_at` and `id` are excluded — they must not affect the hash.
/// Explicit, stable byte discriminants for `OperationKind`.
///
/// These values are committed to in the `OperationHash` and must never change
/// without bumping the hash format. Adding a new variant always appends a new
/// value; never reassign an existing one.
fn kind_discriminant(kind: OperationKind) -> u8 {
    match kind {
        OperationKind::Ingest => 0,
        OperationKind::Search => 1,
        OperationKind::MemoryUpsert => 2,
        OperationKind::Consolidate => 3,
        OperationKind::Contradict => 4,
        OperationKind::HealthCheck => 5,
        OperationKind::Delete => 6,
        OperationKind::BatchInsert => 7,
        OperationKind::GraphRag => 8,
        OperationKind::MemorySearch => 9,
        OperationKind::CommunityDetect => 10,
        OperationKind::CommunitySearch => 11,
        OperationKind::TreeBuild => 12,
        OperationKind::TreeQuery => 13,
        OperationKind::TreeHybrid => 14,
        OperationKind::Snapshot => 15,
    }
}

pub fn compute_operation_hash(
    kind: OperationKind,
    inputs: &OperationInputs,
    policy: &ExecutionPolicy,
) -> OperationHash {
    let mut hasher = blake3::Hasher::new();

    hasher.update(&[kind_discriminant(kind)]);

    // Deterministic bincode encoding of inputs and policy.
    if let Ok(b) = bincode::serde::encode_to_vec(inputs, bincode::config::standard()) {
        hasher.update(&b);
    }
    if let Ok(b) = bincode::serde::encode_to_vec(policy, bincode::config::standard()) {
        hasher.update(&b);
    }

    OperationHash(*hasher.finalize().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic() {
        let inputs = OperationInputs::Search {
            k: 5,
            collection: "default".into(),
            shard_id: 0,
            rerank: true,
            decay: false,
            metadata_filter: false,
            consistency: ConsistencyLevel::Local,
        };
        let policy = ExecutionPolicy::default();
        let h1 = compute_operation_hash(OperationKind::Search, &inputs, &policy);
        let h2 = compute_operation_hash(OperationKind::Search, &inputs, &policy);
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_params_different_hash() {
        let policy = ExecutionPolicy::default();
        let a = compute_operation_hash(
            OperationKind::Search,
            &OperationInputs::Search {
                k: 5,
                collection: "default".into(),
                shard_id: 0,
                rerank: true,
                decay: false,
                metadata_filter: false,
                consistency: ConsistencyLevel::Local,
            },
            &policy,
        );
        let b = compute_operation_hash(
            OperationKind::Search,
            &OperationInputs::Search {
                k: 10,
                collection: "default".into(),
                shard_id: 0,
                rerank: true,
                decay: false,
                metadata_filter: false,
                consistency: ConsistencyLevel::Local,
            },
            &policy,
        );
        assert_ne!(a, b);
    }

    #[test]
    fn operation_new_computes_hash() {
        let op = Operation::new(
            OperationKind::HealthCheck,
            OperationInputs::HealthCheck,
            ExecutionPolicy::default(),
        );
        // Hash must be non-zero
        assert_ne!(op.hash.0, [0u8; 32]);
    }
}
