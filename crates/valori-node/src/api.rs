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
}

#[derive(Serialize)]
pub struct SearchHit {
    pub id: u32,
    pub score: f32,
}

#[derive(Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchHit>,
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
}

#[derive(Serialize)]
pub struct MemorySearchHit {
    pub memory_id: String,
    pub record_id: u32,
    pub score: f32,
    pub metadata: Option<serde_json::Value>,
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
