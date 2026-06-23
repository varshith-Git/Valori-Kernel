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
    /// Sequential index (0-based) into the committed event log.
    pub log_index: u64,
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
}

#[derive(Serialize)]
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

#[derive(Serialize)]
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
