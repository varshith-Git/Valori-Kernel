// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Capability traits — runtime-queryable authorizations to interact with subsystems.
//!
//! Capabilities are checked at plan time (`PlanningContext::capability_set`) and
//! at dispatch time (`CapabilityRegistry`). A task must not call a capability
//! directly — all side effects flow through `Effect` + `EffectBus`.
use async_trait::async_trait;
use bytes::Bytes;
use std::sync::Arc;

use crate::effect::KernelCommandBody;
use crate::error::EffectError;

// ── Base trait ────────────────────────────────────────────────────────────────

/// A named, runtime-queryable authorization to interact with a subsystem.
pub trait Capability: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn is_available(&self) -> bool;
}

// ── KernelCapability ──────────────────────────────────────────────────────────

/// Access to the kernel state machine — the only path for kernel writes.
///
/// The EffectBus calls `apply_command` on behalf of tasks; tasks never call
/// this directly (RFC-0004 §5).
///
/// Read-path methods (`graph_rag`, `memory_search`, etc.) are provided as
/// default no-ops so existing capability impls need not be updated until the
/// corresponding feature is wired. Override them in `EngineKernelCapability`.
#[async_trait]
pub trait KernelCapability: Capability {
    fn shard_count(&self) -> u8;
    /// Apply a kernel mutation command to the given shard.
    ///
    /// Returns a JSON value with at minimum `{"state_hash": "..."}`.
    /// Write commands also include `{"record_id": N}` or similar identifiers
    /// so the caller can thread the assigned ID back through the task output.
    async fn apply_command(
        &self,
        shard_id: u8,
        namespace_id: u16,
        body: &KernelCommandBody,
        request_id: &str,
    ) -> Result<serde_json::Value, EffectError>;
    /// Return BLAKE3 hex of the current state hash for a shard.
    fn state_hash(&self, shard_id: u8) -> String;

    // ── Read + admin operations (default = unavailable) ───────────────────────

    /// Persist the current kernel state to `path` (standalone only).
    /// Returns the state_hash hex after the snapshot is written.
    async fn save_snapshot(
        &self,
        _shard_id: u8,
        _path: Option<&str>,
    ) -> Result<String, EffectError> {
        Err(EffectError::CapabilityUnavailable("save_snapshot"))
    }

    /// Vector search + subgraph expansion (GraphRAG).
    /// Returns `{"hits":[…],"seed_nodes":[…],"subgraph":{"nodes":[…],"edges":[…]}}`.
    async fn graph_rag(
        &self,
        _shard_id: u8,
        _namespace_id: u16,
        _vector: Vec<f32>,
        _k: u32,
        _depth: u32,
    ) -> Result<serde_json::Value, EffectError> {
        Err(EffectError::CapabilityUnavailable("graph_rag"))
    }

    /// Vector search with optional decay, rerank, and metadata filter.
    /// Returns `[{"memory_id":…,"record_id":…,"score":…,"metadata":…}]`.
    async fn memory_search(
        &self,
        _shard_id: u8,
        _namespace_id: u16,
        _vector: Vec<f32>,
        _k: u32,
        _decay_half_life_secs: Option<f64>,
        _rerank: bool,
        _query_text: Option<String>,
        _metadata_filter: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, EffectError> {
        Err(EffectError::CapabilityUnavailable("memory_search"))
    }

    /// Run label-propagation community detection over a namespace.
    /// Returns `{"collection":…,"community_count":…,"receipt":…}`.
    async fn community_detect(
        &self,
        _shard_id: u8,
        _namespace_id: u16,
        _max_iter: u32,
    ) -> Result<serde_json::Value, EffectError> {
        Err(EffectError::CapabilityUnavailable("community_detect"))
    }

    /// Search communities by vector proximity.
    /// Returns `{"hits":[…],"communities":[…]}`.
    async fn community_search(
        &self,
        _shard_id: u8,
        _namespace_id: u16,
        _vector: Vec<f32>,
        _k: u32,
        _depth: u32,
        _drill_in: bool,
    ) -> Result<serde_json::Value, EffectError> {
        Err(EffectError::CapabilityUnavailable("community_search"))
    }

    /// Build a TreeIndex from markdown text.
    /// Returns `{"cache_key":…,"chunk_count":…,"tree":…}`.
    async fn tree_build(
        &self,
        _text: String,
        _doc_name: String,
    ) -> Result<serde_json::Value, EffectError> {
        Err(EffectError::CapabilityUnavailable("tree_build"))
    }

    /// Semantic tree traversal query.
    /// `tree_json`: either cached tree or full tree value.
    /// Returns `{"chunks":[…],"receipt":…}`.
    async fn tree_query(
        &self,
        _tree_json: serde_json::Value,
        _query: String,
        _k: u32,
        _prev_hash: Option<String>,
    ) -> Result<serde_json::Value, EffectError> {
        Err(EffectError::CapabilityUnavailable("tree_query"))
    }

    /// Hybrid vector + tree-RAG search.
    /// Returns `{"hits":[…],"tree_chunks":[…],"receipt":…}`.
    async fn tree_hybrid(
        &self,
        _shard_id: u8,
        _namespace_id: u16,
        _query: String,
        _k: u32,
        _params: serde_json::Value,
    ) -> Result<serde_json::Value, EffectError> {
        Err(EffectError::CapabilityUnavailable("tree_hybrid"))
    }
}

// ── EmbedCapability ───────────────────────────────────────────────────────────

/// Text → vector embedding via the configured provider (Ollama / OpenAI / custom).
#[async_trait]
pub trait EmbedCapability: Capability {
    fn model_name(&self) -> &str;
    fn dim(&self) -> usize;
    async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, EffectError>;
}

// ── LlmCapability ─────────────────────────────────────────────────────────────

/// LLM text completion (entity extraction, summarization, etc.).
#[async_trait]
pub trait LlmCapability: Capability {
    fn default_model(&self) -> &str;
    async fn complete(&self, prompt: String, model: &str) -> Result<String, EffectError>;
}

// ── StorageCapability ─────────────────────────────────────────────────────────

/// Object store access (S3 / local file) for snapshot offload.
#[async_trait]
pub trait StorageCapability: Capability {
    async fn write_object(&self, key: &str, bytes: Bytes) -> Result<(), EffectError>;
    async fn read_object(&self, key: &str) -> Result<Bytes, EffectError>;
    async fn list_objects(&self, prefix: &str) -> Result<Vec<String>, EffectError>;
    async fn delete_object(&self, key: &str) -> Result<(), EffectError>;
}

// ── HttpCapability ────────────────────────────────────────────────────────────

/// Outbound HTTP GET for document ingestion from URLs.
#[async_trait]
pub trait HttpCapability: Capability {
    async fn get(&self, url: &str) -> Result<Bytes, EffectError>;
}

// ── ProofCapability ───────────────────────────────────────────────────────────

/// Append a receipt fragment to the BLAKE3 audit chain and read proofs.
#[async_trait]
pub trait ProofCapability: Capability {
    /// Append a receipt fragment JSON to the audit log for the given shard.
    async fn append_fragment(&self, shard_id: u8, fragment_json: &str) -> Result<(), EffectError>;
    /// Return the current event log proof for a shard.
    async fn event_log_proof(&self, shard_id: u8) -> Result<String, EffectError>;
}

// ── SchedulerCapability ───────────────────────────────────────────────────────

/// Schedule a deferred or recurring operation.
#[async_trait]
pub trait SchedulerCapability: Capability {
    async fn schedule_once(
        &self,
        delay_secs: u64,
        operation_json: String,
    ) -> Result<String, EffectError>;
}

// ── CapabilityRegistry ────────────────────────────────────────────────────────

/// All capabilities available to this node at runtime.
///
/// `embed`, `llm`, `storage`, `http`, `proof`, and `scheduler` are optional —
/// absent means the corresponding capability is unconfigured. The Planner
/// checks `PlanningContext::capability_set` before producing a graph that requires
/// an optional capability, so `None` at dispatch time should not happen for
/// correctly-planned operations.
pub struct CapabilityRegistry {
    pub kernel: Arc<dyn KernelCapability>,
    pub embed: Option<Arc<dyn EmbedCapability>>,
    pub llm: Option<Arc<dyn LlmCapability>>,
    pub storage: Option<Arc<dyn StorageCapability>>,
    pub http: Option<Arc<dyn HttpCapability>>,
    pub proof: Option<Arc<dyn ProofCapability>>,
    pub scheduler: Option<Arc<dyn SchedulerCapability>>,
}

impl CapabilityRegistry {
    pub fn embed(&self) -> Result<&Arc<dyn EmbedCapability>, EffectError> {
        self.embed
            .as_ref()
            .ok_or(EffectError::CapabilityUnavailable("embed"))
    }

    pub fn llm(&self) -> Result<&Arc<dyn LlmCapability>, EffectError> {
        self.llm
            .as_ref()
            .ok_or(EffectError::CapabilityUnavailable("llm"))
    }

    pub fn storage(&self) -> Result<&Arc<dyn StorageCapability>, EffectError> {
        self.storage
            .as_ref()
            .ok_or(EffectError::CapabilityUnavailable("storage"))
    }

    pub fn http(&self) -> Result<&Arc<dyn HttpCapability>, EffectError> {
        self.http
            .as_ref()
            .ok_or(EffectError::CapabilityUnavailable("http"))
    }

    pub fn proof(&self) -> Result<&Arc<dyn ProofCapability>, EffectError> {
        self.proof
            .as_ref()
            .ok_or(EffectError::CapabilityUnavailable("proof"))
    }

    pub fn scheduler(&self) -> Result<&Arc<dyn SchedulerCapability>, EffectError> {
        self.scheduler
            .as_ref()
            .ok_or(EffectError::CapabilityUnavailable("scheduler"))
    }
}

// ── NoOpKernelCapability ──────────────────────────────────────────────────────

/// A no-op `KernelCapability` for tests and disabled-kernel contexts.
pub struct NoOpKernelCapability {
    pub shard_count: u8,
}

impl Capability for NoOpKernelCapability {
    fn name(&self) -> &'static str {
        "kernel_noop"
    }
    fn is_available(&self) -> bool {
        true
    }
}

#[async_trait]
impl KernelCapability for NoOpKernelCapability {
    fn shard_count(&self) -> u8 {
        self.shard_count
    }
    async fn apply_command(
        &self,
        _shard_id: u8,
        _ns: u16,
        _body: &KernelCommandBody,
        _req_id: &str,
    ) -> Result<serde_json::Value, EffectError> {
        Ok(
            serde_json::json!({ "record_id": 0u32, "state_hash": "0000000000000000000000000000000000000000000000000000000000000000" }),
        )
    }
    fn state_hash(&self, _shard_id: u8) -> String {
        "0000000000000000000000000000000000000000000000000000000000000000".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_kernel_capability_returns_zero_hash() {
        let cap = NoOpKernelCapability { shard_count: 1 };
        assert!(cap.is_available());
        use crate::effect::KernelCommandBody;
        let body = KernelCommandBody::InsertRecord {
            values: vec![],
            text: None,
            metadata: None,
            tag: 0,
        };
        let v = cap.apply_command(0, 0, &body, "test-req").await.unwrap();
        assert!(v["state_hash"].as_str().unwrap().len() == 64);
    }

    #[test]
    fn registry_returns_error_for_absent_capability() {
        let reg = CapabilityRegistry {
            kernel: Arc::new(NoOpKernelCapability { shard_count: 1 }),
            embed: None,
            llm: None,
            storage: None,
            http: None,
            proof: None,
            scheduler: None,
        };
        assert!(matches!(
            reg.embed(),
            Err(EffectError::CapabilityUnavailable("embed"))
        ));
    }
}
