// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Memory domain — shared bodies for `POST /v1/memory/upsert`, `POST /v1/memory/search`,
//! `POST /v1/memory/consolidate`, and `POST /v1/memory/contradict` (and aliases).
//!
//! Canonical behavior (both paths, enforced here):
//! * Unknown `collection` -> 404 Not Found.
//! * Consolidate sets metadata if provided in the payload on BOTH paths (previously omitted on cluster).
//! * Contradict checks similarity threshold and commits Contradicts edge identically on both paths.
//! * Upsert, consolidate, and contradict emit write receipts through `receipt_bridge`.
//! * Read consistency for search: cluster mode executes read-index check via `ensure_read_consistency`
//!   before searching, while standalone mode executes a zero-overhead local read.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use std::sync::Arc;

use crate::api::{
    MemoryConsolidateRequest, MemoryConsolidateResponse, MemoryContradictRequest,
    MemoryContradictResponse, MemorySearchHit, MemorySearchResponse, MemorySearchVectorRequest,
    MemoryUpsertResponse, MemoryUpsertVectorRequest,
};

/// Outcome of a memory vector upsert.
pub struct UpsertedMemory {
    pub memory_id: String,
    pub record_id: u32,
    pub document_node_id: u32,
    pub chunk_node_id: u32,
    pub log_index: Option<u64>,
    pub shard_id: u8,
    pub cluster: bool,
    pub state_before: String,
    pub state_after: String,
}

/// Outcome of a memory consolidation.
pub struct ConsolidatedMemory {
    pub old_record_id: u32,
    pub new_record_id: u32,
    pub supersedes_edge_id: u32,
    pub state_hash: String,
    pub log_index: Option<u64>,
    pub shard_id: u8,
    pub cluster: bool,
    pub state_before: String,
    pub state_after: String,
}

/// Outcome of a contradiction detection.
pub struct ContradictedMemory {
    pub record_a: u32,
    pub record_b: u32,
    pub similarity: f32,
    pub contradicts: bool,
    pub edge_id: Option<u32>,
    pub state_hash: String,
    pub log_index: Option<u64>,
    pub shard_id: u8,
    pub cluster: bool,
    pub state_before: String,
    pub state_after: String,
}

#[async_trait::async_trait]
pub trait MemoryOps: Send + Sync {
    /// Optional collection name -> namespace id (`None` = default).
    async fn resolve_collection(&self, name: Option<&str>) -> Option<u16>;

    /// Ensure read consistency for the given namespace before performing a search.
    /// In standalone mode, this is a no-op (always Ok(())).
    /// In cluster mode, if `consistency` != Some("local"), invokes `ensure_read_consistency`.
    async fn ensure_read_consistency(
        &self,
        ns: u16,
        consistency: Option<&str>,
    ) -> Result<(), Response>;

    /// Commit memory upsert vector: inserts vector record, creates doc/chunk nodes,
    /// links them with ParentOf edge, and sets optional metadata.
    async fn upsert_vector(
        &self,
        ns: u16,
        req: &MemoryUpsertVectorRequest,
    ) -> Result<UpsertedMemory, Response>;

    /// Perform vector search with optional recency decay and k candidates.
    /// Returns matching hits with metadata attached.
    async fn search_vector(
        &self,
        ns: u16,
        req: &MemorySearchVectorRequest,
    ) -> Result<Vec<MemorySearchHit>, Response>;

    /// Consolidate memory: soft-deletes old record, inserts new vector record,
    /// creates nodes, links with Supersedes edge, and sets optional metadata.
    async fn consolidate(
        &self,
        ns: u16,
        req: &MemoryConsolidateRequest,
    ) -> Result<ConsolidatedMemory, Response>;

    /// Contradict memory: checks similarity between record A and B.
    /// If similarity >= threshold, commits a Contradicts edge between node A and B.
    async fn contradict(
        &self,
        ns: u16,
        req: &MemoryContradictRequest,
    ) -> Result<ContradictedMemory, Response>;
}

async fn resolve<O: MemoryOps>(ops: &O, collection: Option<&str>) -> Result<u16, Response> {
    ops.resolve_collection(collection).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!(
                    "unknown collection '{}'",
                    collection.unwrap_or("default")
                )
            })),
        )
            .into_response()
    })
}

pub async fn memory_upsert<O: MemoryOps>(
    ops: &O,
    receipts: &Arc<valori_effect::ReceiptStore>,
    req: MemoryUpsertVectorRequest,
) -> Result<Json<MemoryUpsertResponse>, Response> {
    let ns = resolve(ops, req.collection.as_deref()).await?;
    let u = ops.upsert_vector(ns, &req).await?;
    {
        use valori_planner::operation::{OperationInputs, OperationKind};
        let inputs = OperationInputs::MemoryUpsert {
            collection: req.collection.clone().unwrap_or_else(|| "default".into()),
            shard_id: u.shard_id,
        };
        crate::receipt_bridge::emit_write(
            receipts,
            OperationKind::MemoryUpsert,
            &inputs,
            ns,
            u.shard_id,
            u.log_index.unwrap_or(0),
            u.cluster,
            u.state_before,
            u.state_after,
        );
    }
    Ok(Json(MemoryUpsertResponse {
        memory_id: u.memory_id,
        record_id: u.record_id,
        document_node_id: u.document_node_id,
        chunk_node_id: u.chunk_node_id,
        log_index: u.log_index,
    }))
}

pub async fn memory_search<O: MemoryOps>(
    ops: &O,
    req: MemorySearchVectorRequest,
) -> Result<Json<MemorySearchResponse>, Response> {
    let ns = resolve(ops, req.collection.as_deref()).await?;
    ops.ensure_read_consistency(ns, req.consistency.as_deref())
        .await?;
    let results = ops.search_vector(ns, &req).await?;
    Ok(Json(MemorySearchResponse { results }))
}

pub async fn memory_consolidate<O: MemoryOps>(
    ops: &O,
    receipts: &Arc<valori_effect::ReceiptStore>,
    req: MemoryConsolidateRequest,
) -> Result<Json<MemoryConsolidateResponse>, Response> {
    let ns = resolve(ops, req.collection.as_deref()).await?;
    let c = ops.consolidate(ns, &req).await?;
    {
        use valori_planner::operation::{OperationInputs, OperationKind};
        let inputs = OperationInputs::Consolidate {
            shard_id: c.shard_id,
        };
        crate::receipt_bridge::emit_write(
            receipts,
            OperationKind::Consolidate,
            &inputs,
            ns,
            c.shard_id,
            c.log_index.unwrap_or(0),
            c.cluster,
            c.state_before,
            c.state_after,
        );
    }
    Ok(Json(MemoryConsolidateResponse {
        old_record_id: c.old_record_id,
        new_record_id: c.new_record_id,
        supersedes_edge_id: c.supersedes_edge_id,
        state_hash: c.state_hash,
        log_index: c.log_index,
    }))
}

pub async fn memory_contradict<O: MemoryOps>(
    ops: &O,
    receipts: &Arc<valori_effect::ReceiptStore>,
    req: MemoryContradictRequest,
) -> Result<Json<MemoryContradictResponse>, Response> {
    let ns = resolve(ops, req.collection.as_deref()).await?;
    let c = ops.contradict(ns, &req).await?;
    if c.contradicts && c.edge_id.is_some() {
        use valori_planner::operation::{OperationInputs, OperationKind};
        let inputs = OperationInputs::Contradict {
            shard_id: c.shard_id,
        };
        crate::receipt_bridge::emit_write(
            receipts,
            OperationKind::Contradict,
            &inputs,
            ns,
            c.shard_id,
            c.log_index.unwrap_or(0),
            c.cluster,
            c.state_before,
            c.state_after,
        );
    }
    Ok(Json(MemoryContradictResponse {
        record_a: c.record_a,
        record_b: c.record_b,
        similarity: c.similarity,
        contradicts: c.contradicts,
        edge_id: c.edge_id,
        state_hash: c.state_hash,
        log_index: c.log_index,
    }))
}
