// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use serde::{Deserialize, Serialize};

// Note: We need to make sure valori-kernel exports NodeKind/EdgeKind publicly or we redefine/wrap them.
// Since valori-kernel is a dependency, we can use its types if they are pub.
// Assuming valori_kernel::types::enums::* is pub.


// ── Collections / namespace seam ─────────────────────────────────────────────
//
// The API accepts a `collection` string on every data-path request.
// Phase 4 wires this to a live NamespaceRegistry; today the only registered
// collection is "default" (NamespaceId 0).  Any other name returns 400 so
// clients get a clear error rather than silently landing in the wrong bucket.

pub use valori_kernel::types::id::{NamespaceId, DEFAULT_NS};

/// Name of the default (always-existing) collection.
pub const DEFAULT_COLLECTION: &str = "default";

// `resolve_namespace` is intentionally removed — server handlers call
// `engine.resolve_collection(name)` directly so the live registry is consulted.

/// Backward-compat alias used by handlers that only need validation.
pub fn validate_collection(collection: Option<&str>) -> Result<(), crate::errors::EngineError> {
    match collection {
        None | Some("default") => Ok(()),
        Some(other) => Err(crate::errors::EngineError::InvalidInput(format!(
            "unknown collection '{other}' — use POST /v1/namespaces to create it"
        ))),
    }
}

#[derive(Deserialize)]
pub struct InsertRecordRequest {
    pub values: Vec<f32>,
    #[serde(default)]
    pub collection: Option<String>,
    /// Optional raw text for BM25 hybrid reranking. When provided, stored
    /// in the reranker index alongside the vector so future searches can
    /// use term-frequency scoring to reorder results.
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Serialize)]
pub struct InsertRecordResponse {
    pub id: u32,
}

#[derive(Deserialize)]
pub struct DeleteRecordRequest {
    pub id: u32,
    #[serde(default)]
    pub collection: Option<String>,
}

#[derive(Serialize)]
pub struct DeleteRecordResponse {
    pub success: bool,
    /// Raft log index of the committed write — cluster path only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_index: Option<u64>,
}

#[derive(Deserialize)]
pub struct SearchRequest {
    pub query: Vec<f32>,
    pub k: usize,
    #[serde(default)]
    pub collection: Option<String>,
    /// ISO 8601 UTC timestamp — search the vector state as it existed at this moment.
    /// Requires the event log to be enabled (`VALORI_EVENT_LOG_PATH`).
    #[serde(default)]
    pub as_of: Option<String>,
    /// Log index — search the vector state after exactly this many committed events.
    /// Mutually exclusive with `as_of`; `as_of_log_index` takes precedence if both given.
    #[serde(default)]
    pub as_of_log_index: Option<u64>,
    /// Phase C4.1 — recency half-life in seconds. When set (> 0), results are
    /// re-ranked so older records decay: a record one half-life old has its L2
    /// distance doubled. `0`/absent uses the server default (or pure distance).
    /// Ignored for `as_of` / point-in-time queries.
    #[serde(default)]
    pub decay_half_life_secs: Option<u64>,
    /// BM25 hybrid reranking. When `true` (default), the server fetches
    /// `k × POOL_FACTOR` candidates by vector similarity and re-ranks them by
    /// a 50/50 blend of normalised vector score + BM25 term-frequency score
    /// before returning the top-k. Requires `query_text` to be set.
    /// Set to `false` to get pure vector ranking (legacy behaviour).
    #[serde(default = "default_rerank")]
    pub rerank: bool,
    /// The raw query string used for BM25 scoring. Required when `rerank=true`.
    /// Ignored when `rerank=false`.
    #[serde(default)]
    pub query_text: Option<String>,
    /// Optional JSON object whose key-value pairs must ALL be present (and equal)
    /// in a record's metadata for the record to be returned.
    /// Numeric values support optional range operators: `{"gte": 2020, "lte": 2024}`.
    /// Example: `{"author": "Alice", "year": {"gte": 2020}}`
    #[serde(default)]
    pub metadata_filter: Option<serde_json::Map<String, serde_json::Value>>,
}

fn default_rerank() -> bool { true }

/// Returns true when every key in `filter` is present in `meta` with a matching value.
/// Supports exact equality for strings/booleans/null, and range operators
/// (`eq`, `gt`, `gte`, `lt`, `lte`) for numbers.
pub fn matches_metadata_filter(
    meta: &serde_json::Value,
    filter: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    let obj = match meta.as_object() {
        Some(o) => o,
        None => return false,
    };
    for (key, expected) in filter {
        let actual = match obj.get(key) {
            Some(v) => v,
            None => return false,
        };
        if !value_matches(actual, expected) {
            return false;
        }
    }
    true
}

fn value_matches(actual: &serde_json::Value, expected: &serde_json::Value) -> bool {
    // If expected is an object with range operators, apply numeric comparison.
    if let Some(ops) = expected.as_object() {
        let has_op = ops.contains_key("eq")
            || ops.contains_key("gt") || ops.contains_key("gte")
            || ops.contains_key("lt") || ops.contains_key("lte");
        if has_op {
            let num = match actual.as_f64() {
                Some(n) => n,
                None => return false,
            };
            if let Some(v) = ops.get("eq") {
                if actual != v { return false; }
            }
            if let Some(v) = ops.get("gt").and_then(|v| v.as_f64()) {
                if !(num > v) { return false; }
            }
            if let Some(v) = ops.get("gte").and_then(|v| v.as_f64()) {
                if !(num >= v) { return false; }
            }
            if let Some(v) = ops.get("lt").and_then(|v| v.as_f64()) {
                if !(num < v) { return false; }
            }
            if let Some(v) = ops.get("lte").and_then(|v| v.as_f64()) {
                if !(num <= v) { return false; }
            }
            return true;
        }
    }
    // Exact equality for all other types.
    actual == expected
}

#[derive(Serialize)]
pub struct SearchHit {
    pub id: u32,
    pub score: f32,
    /// Phase C4.1 — applied decay factor in (0, 1]. Present only when decay is
    /// active. `score` stays the true (undecayed) L2 distance for honesty;
    /// ranking reflects `score / decay_factor`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decay_factor: Option<f32>,
    /// Age of the record in seconds at query time. Present only when decay is
    /// active and the record's creation time is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_secs: Option<u64>,
}

#[derive(Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchHit>,
    /// Present only for as-of searches: the log index of the replayed state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of_log_index: Option<u64>,
    /// Unix-second wall-clock timestamp of the `as_of_log_index` event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of_timestamp_unix: Option<u64>,
    /// ISO 8601 string of `as_of_timestamp_unix`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of_timestamp_iso: Option<String>,
    /// BLAKE3 hex hash of the kernel state at `as_of_log_index`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of_state_hash: Option<String>,
}

impl SearchResponse {
    pub fn simple(results: Vec<SearchHit>) -> Self {
        Self { results, as_of_log_index: None, as_of_timestamp_unix: None, as_of_timestamp_iso: None, as_of_state_hash: None }
    }
}

/// A single entry in the timeline — one committed kernel event with its metadata.
#[derive(Serialize)]
pub struct TimelineEntry {
    /// Sequential index within this entry's shard log (0-based).
    /// Used as a tie-breaker when two shards share the same `timestamp_unix`.
    pub log_index: u64,
    /// Shard that committed this event. Always 0 in standalone mode.
    pub shard_id: u32,
    /// Unix-second wall-clock timestamp when this event was committed.
    pub timestamp_unix: u64,
    /// ISO 8601 UTC string for `timestamp_unix`.
    pub timestamp_iso: String,
    /// Human-readable event kind.
    pub event_type: &'static str,
    /// Record ID if this is a record-level event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_id: Option<u32>,
    /// Node ID if this is a graph-node event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<u32>,
    /// Edge ID if this is a graph-edge event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_id: Option<u32>,
}

#[derive(Serialize)]
pub struct TimelineResponse {
    pub events: Vec<TimelineEntry>,
    pub total: usize,
    /// Inclusive lower bound filter applied (unix seconds), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_unix: Option<u64>,
    /// Inclusive upper bound filter applied (unix seconds), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_unix: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperationSummary {
    pub id: String,
    #[serde(rename = "type")]
    pub op_type: String,
    pub status: String,
    pub timing: String,
    pub timestamp_unix: u64,
    pub collection: String,
    pub details: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperationsListResponse {
    pub operations: Vec<OperationSummary>,
    pub total: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperationDetailResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub op_type: String,
    pub status: String,
    pub timing: String,
    pub timestamp_unix: u64,
    pub collection: String,
    pub overview: serde_json::Value,
    pub results: serde_json::Value,
    pub proof: serde_json::Value,
    pub metrics: serde_json::Value,
}

#[derive(Deserialize)]
pub struct CreateNodeRequest {
    pub record_id: Option<u32>,
    // NodeKind needs to be deserializable. 
    // valori-kernel NodeKind derives Copy, Clone, Debug, PartialEq. Does it derive Serialize/Deserialize?
    // If not, we need a mirror enum or manual impl.
    // The user didn't ask to modify kernel. 
    // So we must redefine or use `#[serde(remote = ...)]`?
    // Or just "kind": u8 ?
    // User request: "You can define NodeKind and EdgeKind via valori-kernel’s enums (they are #[repr(u8)] + serde)."
    // Ah, the user implied they *are* serde? 
    // Or I should make them serde in kernel?
    // "Do NOT modify valori-kernel".
    // "You can define NodeKind ... via valori-kernel's enums (they are #[repr(u8)] + serde)" -> Maybe the user thinks they are serde?
    // Or maybe "You can define [your own API types] via ..."
    // I will redefine them here for serde support if kernel ones don't have it.
    // Let's assume for now I wrap them: kind: u8 in JSON, mapped to enum.
    pub kind: u8, 
    #[serde(default)]
    pub collection: Option<String>,
}

#[derive(Serialize)]
pub struct CreateNodeResponse {
    pub node_id: u32,
    /// Raft log index of the committed write — cluster path only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_index: Option<u64>,
}

#[derive(Deserialize)]
pub struct CreateEdgeRequest {
    pub from: u32,
    pub to: u32,
    pub kind: u8,
    #[serde(default)]
    pub collection: Option<String>,
}

#[derive(Serialize)]
pub struct CreateEdgeResponse {
    pub edge_id: u32,
    /// Raft log index of the committed write — cluster path only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_index: Option<u64>,
}

#[derive(Serialize)]
pub struct GetNodeResponse {
    pub kind: u8,
    pub record_id: Option<u32>,
    pub namespace_id: u16,
}

#[derive(Serialize)]
pub struct EdgeData {
    pub edge_id: u32,
    pub to_node: u32,
    pub kind: u8,
}

#[derive(Serialize)]
pub struct GetEdgesResponse {
    pub edges: Vec<EdgeData>,
}

#[derive(Deserialize)]
pub struct MemoryUpsertVectorRequest {
    pub vector: Vec<f32>,
    #[serde(default)]
    pub collection: Option<String>,
    pub attach_to_document_node: Option<u32>,
    // Reserved for future use:
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct MemoryUpsertResponse {
    pub memory_id: String,
    pub record_id: u32,
    pub document_node_id: u32,
    pub chunk_node_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_index: Option<u64>,
}

#[derive(Deserialize)]
pub struct MemorySearchVectorRequest {
    pub query_vector: Vec<f32>,
    pub k: usize,
    #[serde(default)]
    pub collection: Option<String>,
    /// Phase C4.1 — recency half-life (seconds). When set (> 0), the agent-memory
    /// recall path re-ranks older memories down. See `SearchRequest`.
    #[serde(default)]
    pub decay_half_life_secs: Option<u64>,
    /// Phase S6 (cluster mode only; ignored standalone): `"local"` skips
    /// the read-index round trip (eventually consistent, faster). Absent
    /// or any other value defaults to linearizable, matching `/v1/search`.
    #[serde(default)]
    pub consistency: Option<String>,
    /// Phase I7 — restrict results to records whose stored metadata satisfies
    /// every key/value predicate. Same semantics as `SearchRequest::metadata_filter`.
    #[serde(default)]
    pub metadata_filter: Option<serde_json::Map<String, serde_json::Value>>,
    /// Phase C5 — when `true` (default) and `query_text` is provided, re-ranks
    /// candidates by hybrid BM25 + vector score before returning the top-k.
    #[serde(default = "crate::api::default_rerank")]
    pub rerank: bool,
    /// Phase C5 — raw query text for BM25 hybrid re-ranking. Required when
    /// `rerank=true`; ignored otherwise.
    #[serde(default)]
    pub query_text: Option<String>,
}

#[derive(Serialize)]
pub struct NodeInfo {
    pub node_id: u32,
    pub kind: u8,
    pub record_id: Option<u32>,
    pub namespace_id: u16,
}

#[derive(Serialize)]
pub struct ListNodesResponse {
    pub nodes: Vec<NodeInfo>,
    pub count: usize,
}

#[derive(Serialize)]
pub struct DeleteNodeResponse {
    pub success: bool,
    /// Raft log index of the committed write — cluster path only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_index: Option<u64>,
}

#[derive(Serialize, Deserialize)]
pub struct MemorySearchHit {
    pub memory_id: String,
    pub record_id: u32,
    pub score: f32,
    pub metadata: Option<serde_json::Value>,
    /// Phase C4.1 — applied decay factor in (0, 1]; present only when decay is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decay_factor: Option<f32>,
    /// Phase C4.1 — record age in seconds; present only when decay is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_secs: Option<u64>,
}

// ... existing content ...

#[derive(Serialize, Deserialize)]
pub struct MemorySearchResponse {
    pub results: Vec<MemorySearchHit>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct MetadataSetRequest {
    pub target_id: String,
    pub metadata: serde_json::Value,
}

#[derive(Serialize, Debug)]
pub struct MetadataSetResponse {
    pub success: bool,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct MetadataGetRequest {
    pub target_id: String,
}

#[derive(Serialize, Debug)]
pub struct MetadataGetResponse {
    pub target_id: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct SnapshotSaveRequest {
    // Optional path override. If None, uses configured snapshot path.
    pub path: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct SnapshotSaveResponse {
    pub success: bool,
    pub path: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct SnapshotRestoreRequest {
    // Path to load from.
    pub path: String,
}

#[derive(Serialize, Debug)]
pub struct SnapshotRestoreResponse {
    pub success: bool,
}

// Phase 26: Event log proof API
#[derive(Serialize, Debug)]
pub struct EventProofResponse {
    pub kernel_version: u32,
    pub event_log_hash: String,       // hex-encoded BLAKE3
    pub final_state_hash: String,     // hex-encoded BLAKE3  
    pub snapshot_hash: Option<String>, // hex-encoded BLAKE3 (if snapshot exists)
    pub event_count: u64,
    pub committed_height: u64,
}

// Phase 34: Batch Ingestion
#[derive(Deserialize, Serialize, Debug)]
pub struct BatchInsertRequest {
    pub batch: Vec<Vec<f32>>,
    #[serde(default)]
    pub collection: Option<String>,
    /// Optional per-vector metadata blobs (UTF-8 JSON strings).
    /// If present, must be the same length as `batch`.
    /// Each entry is committed inside the `InsertRecord` event and is
    /// therefore included in the BLAKE3 audit chain.
    #[serde(default)]
    pub metadata: Option<Vec<Option<String>>>,
    /// Per-item idempotency keys (32-hex strings = 16-byte UUIDs).
    /// If present, must be the same length as `batch`. A null entry means
    /// "no dedup key for this item". A repeated key causes that item to be
    /// skipped and the previously assigned ID is returned instead.
    #[serde(default)]
    pub request_ids: Option<Vec<Option<String>>>,
    /// Optional per-vector text strings for BM25 hybrid reranking.
    /// If present, must be the same length as `batch`. A null entry means
    /// no text is stored for that vector. Text is tokenised and indexed
    /// so that future /search calls with `rerank=true` can re-score results.
    #[serde(default)]
    pub texts: Option<Vec<Option<String>>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BatchInsertResponse {
    pub ids: Vec<u32>,
}

// ── Collection (namespace) management ────────────────────────────────────────

#[derive(Deserialize, Debug)]
pub struct CreateCollectionRequest {
    pub name: String,
}

#[derive(Serialize, Debug)]
pub struct CollectionInfo {
    pub name: String,
    pub id: u16,
}

#[derive(Serialize, Debug)]
pub struct CreateCollectionResponse {
    pub name: String,
    pub id: u16,
    pub created: bool,
}

#[derive(Serialize, Debug)]
pub struct ListCollectionsResponse {
    pub collections: Vec<CollectionInfo>,
}

// ── C4.2: Memory consolidation ───────────────────────────────────────────────

/// Replace an existing memory record with a new vector, committing a
/// SoftDeleteRecord + AutoInsertRecord + AutoCreateEdge(Supersedes) to the
/// BLAKE3 audit chain in one logical operation.
#[derive(Deserialize)]
pub struct MemoryConsolidateRequest {
    /// Record id of the memory being replaced.
    pub old_record_id: u32,
    /// New vector that replaces the old memory.
    pub new_vector: Vec<f32>,
    #[serde(default)]
    pub collection: Option<String>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct MemoryConsolidateResponse {
    /// The old record id (now soft-deleted).
    pub old_record_id: u32,
    /// The new record id.
    pub new_record_id: u32,
    /// The Supersedes edge id linking new → old.
    pub supersedes_edge_id: u32,
    /// BLAKE3 state hash after all three events are applied.
    pub state_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_index: Option<u64>,
}

// ── C4.3: Contradiction detection ────────────────────────────────────────────

/// Check whether two records contradict each other (by cosine similarity
/// threshold) and, if so, commit a Contradicts edge to the audit chain.
#[derive(Deserialize)]
pub struct MemoryContradictRequest {
    pub record_a: u32,
    pub record_b: u32,
    /// Cosine similarity threshold above which the records are deemed to
    /// contradict. Default 0.85 — tuned for claim-level NLI in Q16.16 space.
    #[serde(default)]
    pub threshold: Option<f32>,
    #[serde(default)]
    pub collection: Option<String>,
}

#[derive(Serialize)]
pub struct MemoryContradictResponse {
    pub record_a: u32,
    pub record_b: u32,
    pub similarity: f32,
    pub contradicts: bool,
    /// Edge id of the Contradicts edge, present only when contradicts=true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_id: Option<u32>,
    pub state_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_index: Option<u64>,
}
