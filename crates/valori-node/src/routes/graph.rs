// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Graph endpoints — shared bodies for
//! `POST /v1/graph/node`, `GET|DELETE /v1/graph/node/:id`, `GET /v1/graph/nodes`,
//! `POST /v1/graph/edge`, `GET /v1/graph/edges/:id`, `GET /v1/graph/subgraph`.
//!
//! Canonical behavior (both paths, enforced here):
//! * Invalid node/edge `kind` → 400. (Standalone previously coerced unknown
//!   kinds to the default variant silently; cluster rejected them.)
//! * Unknown `collection` → 404. (Standalone previously returned 400.)
//! * `GET /v1/graph/nodes` without a `collection` lists the DEFAULT namespace
//!   only. (Cluster previously listed every namespace's nodes when the param
//!   was absent — a tenant-isolation leak.)
//! * `DELETE /v1/graph/node/:id` exists on BOTH paths. (Previously
//!   standalone-only; the cluster commits `KernelEvent::DeleteNode` via Raft.)
//! * Create/delete responses carry `log_index` on the cluster path only
//!   (`skip_serializing_if` keeps the standalone wire format byte-identical).

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use valori_kernel::types::enums::{EdgeKind, NodeKind};
use valori_kernel::types::id::NodeId;
use valori_rag::graph::{Direction, GraphQuery, GraphQueryHit};

use crate::api::{
    CreateEdgeRequest, CreateEdgeResponse, CreateNodeRequest, CreateNodeResponse,
    DeleteNodeResponse, EdgeData, GetEdgesResponse, GetNodeResponse, GraphQueryHitDto,
    GraphQueryResponse, ListNodesResponse, NodeInfo,
};

/// A committed graph write: the allocated id plus, on the cluster path, the
/// Raft log index it was committed at.
pub struct CommittedGraphWrite {
    pub id: u32,
    pub log_index: Option<u64>,
}

/// The state-touching primitives each path provides. Reads return
/// `Result<…, Response>` so the cluster impl can surface its startup
/// readiness gate (B13) as an error response.
#[async_trait::async_trait]
pub trait GraphOps: Send + Sync {
    /// Optional collection name → namespace id (`None` = default).
    /// `None` result = unknown collection.
    async fn resolve_collection(&self, name: Option<&str>) -> Option<u16>;
    async fn create_node(
        &self,
        ns: u16,
        kind: NodeKind,
        record_id: Option<u32>,
    ) -> Result<CommittedGraphWrite, Response>;
    async fn create_edge(
        &self,
        ns: u16,
        from: u32,
        to: u32,
        kind: EdgeKind,
    ) -> Result<CommittedGraphWrite, Response>;
    /// The shared handler has already 404'd a missing node.
    async fn delete_node(&self, ns: u16, id: u32) -> Result<Option<u64>, Response>;
    /// `Ok(None)` = node not found.
    async fn get_node(&self, ns: u16, id: u32) -> Result<Option<GetNodeResponse>, Response>;
    /// `Ok(None)` = node not found; `Ok(Some(edges))` = its outgoing edges.
    async fn node_edges(&self, ns: u16, id: u32) -> Result<Option<Vec<EdgeData>>, Response>;
    /// Every live node in `ns` — the shared handler applies the kind filter
    /// and pagination.
    async fn list_nodes(&self, ns: u16) -> Result<Vec<NodeInfo>, Response>;
    /// BFS expansion — returns the `(nodes, edges)` JSON arrays produced by
    /// `graph_rag::expand_subgraph`.
    async fn subgraph(
        &self,
        ns: u16,
        root: u32,
        depth: u32,
    ) -> Result<(serde_json::Value, serde_json::Value), Response>;
    /// G1.1 — deterministic, filterable, depth-bounded graph query. `Ok(None)`
    /// = the query's `start` node does not exist in `ns` (including "exists,
    /// but in a different namespace" — the shared handler maps both to 404,
    /// never distinguishing them, per `valori_rag::graph::query_graph`'s own
    /// tenant-isolation-safe contract).
    async fn query(
        &self,
        ns: u16,
        query: GraphQuery,
    ) -> Result<Option<Vec<GraphQueryHit>>, Response>;
}

// ── Shared query types ────────────────────────────────────────────────────────

#[cfg_attr(feature = "utoipa", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "utoipa", into_params(parameter_in = Query))]
#[derive(Deserialize)]
pub struct CollectionQuery {
    #[serde(default)]
    pub collection: Option<String>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "utoipa", into_params(parameter_in = Query))]
#[derive(Deserialize)]
pub struct ListNodesQuery {
    #[serde(default)]
    pub collection: Option<String>,
    /// Filter to a single node kind (0=Document, 1=Chunk, 2=Concept, …).
    pub kind: Option<u8>,
    /// Pagination — applied after the `kind` filter. Absent `limit` returns
    /// everything (backward compatible with clients that predate pagination).
    #[serde(default)]
    pub offset: usize,
    pub limit: Option<usize>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "utoipa", into_params(parameter_in = Query))]
#[derive(Deserialize)]
pub struct SubgraphQuery {
    pub root: u32,
    #[serde(default = "default_depth")]
    pub depth: u32,
    #[serde(default)]
    pub collection: Option<String>,
}
fn default_depth() -> u32 {
    2
}

#[cfg_attr(feature = "utoipa", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "utoipa", into_params(parameter_in = Query))]
/// G1.1 — `GET /v1/graph/query` request. Only `start` is required; every
/// other field has a documented default (see `valori_rag::graph::GraphQuery`).
#[derive(Deserialize)]
pub struct GraphQueryParams {
    pub start: u32,
    /// `"outgoing"` (default) | `"incoming"` | `"both"` — case-insensitive.
    #[serde(default)]
    pub direction: Option<String>,
    /// Restrict traversal to a single edge kind (same u8 encoding as
    /// `CreateEdgeRequest::kind`). Absent = no restriction.
    #[serde(default)]
    pub edge_kind: Option<u8>,
    /// Restrict traversal to a single node kind (same u8 encoding as
    /// `ListNodesQuery::kind`). Absent = no restriction.
    #[serde(default)]
    pub node_kind: Option<u8>,
    #[serde(default = "valori_rag::graph::default_query_depth")]
    pub depth: u32,
    #[serde(default = "valori_rag::graph::default_query_limit")]
    pub limit: usize,
    #[serde(default)]
    pub collection: Option<String>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn resolve<O: GraphOps>(ops: &O, collection: Option<&str>) -> Result<u16, Response> {
    ops.resolve_collection(collection)
        .await
        .ok_or_else(|| crate::error_codes::collection_not_found(collection))
}

fn node_not_found(id: u32) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": format!("node {id} not found") })),
    )
        .into_response()
}

// ── Shared handlers ───────────────────────────────────────────────────────────

pub async fn create_node<O: GraphOps>(
    ops: &O,
    req: CreateNodeRequest,
) -> Result<Json<CreateNodeResponse>, Response> {
    let kind = NodeKind::from_u8(req.kind).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("unknown node kind: {}", req.kind) })),
        )
            .into_response()
    })?;
    let ns = resolve(ops, req.collection.as_deref()).await?;
    let w = ops.create_node(ns, kind, req.record_id).await?;
    metrics::counter!("valori_graph_node_create_total", 1u64);
    Ok(Json(CreateNodeResponse {
        node_id: w.id,
        log_index: w.log_index,
    }))
}

pub async fn create_edge<O: GraphOps>(
    ops: &O,
    req: CreateEdgeRequest,
) -> Result<Json<CreateEdgeResponse>, Response> {
    let kind = EdgeKind::from_u8(req.kind).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("unknown edge kind: {}", req.kind) })),
        )
            .into_response()
    })?;
    let ns = resolve(ops, req.collection.as_deref()).await?;
    let w = ops.create_edge(ns, req.from, req.to, kind).await?;
    metrics::counter!("valori_graph_edge_create_total", 1u64);
    Ok(Json(CreateEdgeResponse {
        edge_id: w.id,
        log_index: w.log_index,
    }))
}

pub async fn get_node<O: GraphOps>(
    ops: &O,
    id: u32,
    q: CollectionQuery,
) -> Result<Json<GetNodeResponse>, Response> {
    let ns = resolve(ops, q.collection.as_deref()).await?;
    match ops.get_node(ns, id).await? {
        Some(node) => Ok(Json(node)),
        None => Err(node_not_found(id)),
    }
}

pub async fn delete_node<O: GraphOps>(
    ops: &O,
    id: u32,
    q: CollectionQuery,
) -> Result<Json<DeleteNodeResponse>, Response> {
    let ns = resolve(ops, q.collection.as_deref()).await?;
    if ops.get_node(ns, id).await?.is_none() {
        return Err(node_not_found(id));
    }
    let log_index = ops.delete_node(ns, id).await?;
    Ok(Json(DeleteNodeResponse {
        success: true,
        log_index,
    }))
}

pub async fn list_nodes<O: GraphOps>(
    ops: &O,
    q: ListNodesQuery,
) -> Result<Json<ListNodesResponse>, Response> {
    let ns = resolve(ops, q.collection.as_deref()).await?;
    let filtered: Vec<NodeInfo> = ops
        .list_nodes(ns)
        .await?
        .into_iter()
        .filter(|n| q.kind.is_none_or(|k| n.kind == k))
        .collect();
    let count = filtered.len();
    let nodes = match q.limit {
        Some(limit) => filtered.into_iter().skip(q.offset).take(limit).collect(),
        None => filtered,
    };
    Ok(Json(ListNodesResponse { nodes, count }))
}

pub async fn get_edges<O: GraphOps>(
    ops: &O,
    id: u32,
    q: CollectionQuery,
) -> Result<Json<GetEdgesResponse>, Response> {
    let ns = resolve(ops, q.collection.as_deref()).await?;
    match ops.node_edges(ns, id).await? {
        Some(edges) => Ok(Json(GetEdgesResponse { edges })),
        None => Err(node_not_found(id)),
    }
}

pub async fn get_subgraph<O: GraphOps>(
    ops: &O,
    q: SubgraphQuery,
) -> Result<Json<serde_json::Value>, Response> {
    let ns = resolve(ops, q.collection.as_deref()).await?;
    let (nodes, edges) = ops.subgraph(ns, q.root, q.depth).await?;
    let node_count = nodes.as_array().map_or(0, |a| a.len());
    let edge_count = edges.as_array().map_or(0, |a| a.len());
    metrics::histogram!("valori_graph_traversal_nodes", node_count as f64);
    metrics::histogram!("valori_graph_traversal_edges", edge_count as f64);
    Ok(Json(serde_json::json!({ "nodes": nodes, "edges": edges })))
}

// ── G1.1 — GET /v1/graph/query ────────────────────────────────────────────────

fn bad_request(msg: String) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": msg })),
    )
        .into_response()
}

pub async fn query<O: GraphOps>(
    ops: &O,
    q: GraphQueryParams,
) -> Result<Json<GraphQueryResponse>, Response> {
    let direction = match q
        .direction
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        None | Some("outgoing") => Direction::Outgoing,
        Some("incoming") => Direction::Incoming,
        Some("both") => Direction::Both,
        Some(other) => {
            return Err(bad_request(format!(
                "unknown direction '{other}' — expected outgoing, incoming, or both"
            )))
        }
    };
    let edge_kind = q
        .edge_kind
        .map(|v| EdgeKind::from_u8(v).ok_or_else(|| bad_request(format!("unknown edge kind: {v}"))))
        .transpose()?;
    let node_kind = q
        .node_kind
        .map(|v| NodeKind::from_u8(v).ok_or_else(|| bad_request(format!("unknown node kind: {v}"))))
        .transpose()?;

    let ns = resolve(ops, q.collection.as_deref()).await?;
    let query = GraphQuery {
        start: NodeId(q.start),
        direction,
        edge_kind,
        node_kind,
        max_depth: q.depth,
        limit: q.limit,
    };

    match ops.query(ns, query).await? {
        Some(hits) => {
            let count = hits.len();
            metrics::counter!("valori_graph_query_total", 1u64);
            metrics::histogram!("valori_graph_traversal_nodes", count as f64);
            let hits = hits
                .into_iter()
                .map(|h| GraphQueryHitDto {
                    node_id: h.node_id,
                    kind: h.kind as u8,
                    record_id: h.record_id,
                    depth: h.depth,
                })
                .collect();
            Ok(Json(GraphQueryResponse { hits, count }))
        }
        None => Err(node_not_found(q.start)),
    }
}
