// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Shared handlers for Phase 4: Collection index lifecycle.
//!
//! `POST /v1/namespaces/{name}/index`  — create, change, or drop an index
//! `GET  /v1/namespaces/{name}/index`  — read lifecycle status
//!
//! Both standalone (`server.rs`) and cluster (`cluster_server.rs`) wire these
//! handlers via the `IndexOps` trait. As of Phase 4.3 cluster mode supports
//! HNSW/IVF/BQ per-collection ANN indexes: the desired spec + generation is
//! replicated through Raft (SetMeta), and each node builds a node-local index
//! independently (node-local activation model).

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use valori_engine::index_manager::{
    CollectionIndexState, IndexBuildRequest, IndexSpec, IndexStatusResponse,
};

// ── Ops trait — lets the same handler body work for standalone and cluster ────

/// State-touching operations the index lifecycle handlers need.
///
/// Standalone impl: directly accesses `Engine` (locks `SharedEngine`).
/// Cluster impl (Phase 4.3): commits desired spec through Raft then builds
/// a node-local ANN index. `supports_ann_builds()` returns `true` for both.
#[async_trait::async_trait]
pub trait IndexOps: Send + Sync {
    /// Resolve a collection name to its namespace id. Returns `None` if unknown.
    async fn resolve(&self, name: &str) -> Option<u16>;

    /// Returns the current lifecycle state for `namespace_id`.
    async fn get_index_state(&self, namespace_id: u16) -> CollectionIndexState;

    /// Start a background build for `namespace_id`.
    /// Returns the new generation id on success.
    async fn start_build(&self, namespace_id: u16, spec: IndexSpec) -> Result<u32, String>;

    /// Drop the active index (revert to exact search).
    async fn drop_index(&self, namespace_id: u16) -> Result<(), String>;

    /// Whether this ops implementation supports ANN index builds.
    fn supports_ann_builds(&self) -> bool;
}

// ── Shared handlers ──────────────────────────────────────────────────────────

/// `POST /v1/namespaces/{name}/index`
///
/// Request body: `IndexBuildRequest { type: "hnsw"|"ivf"|"bq"|null, parameters: {} }`
///
/// Response: `IndexStatusResponse`
pub async fn create_or_change_index<O: IndexOps>(
    ops: &O,
    name: &str,
    payload: IndexBuildRequest,
) -> Response {
    let ns_id = match ops.resolve(name).await {
        Some(id) => id,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": format!("collection '{}' not found", name)
                })),
            )
                .into_response();
        }
    };

    // Drop request: null type
    if payload.index_type.is_none() {
        if !ops.supports_ann_builds() {
            return cluster_unsupported_response();
        }
        return match ops.drop_index(ns_id).await {
            Ok(()) => {
                let state = ops.get_index_state(ns_id).await;
                let resp = IndexStatusResponse::from_state(name, &state);
                (StatusCode::OK, Json(resp)).into_response()
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e})),
            )
                .into_response(),
        };
    }

    let index_type = payload.index_type.as_deref().unwrap();

    // Validate supported types
    match index_type {
        "hnsw" | "ivf" | "bq" => {}
        other => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!(
                        "unsupported index type '{}'; supported: hnsw, ivf, bq, or null to drop",
                        other
                    )
                })),
            )
                .into_response();
        }
    }

    if !ops.supports_ann_builds() {
        return cluster_unsupported_response();
    }

    let spec = IndexSpec {
        index_type: index_type.to_string(),
        parameters: payload.parameters,
    };

    match ops.start_build(ns_id, spec).await {
        Ok(_gen) => {
            let state = ops.get_index_state(ns_id).await;
            let resp = IndexStatusResponse::from_state(name, &state);
            (StatusCode::ACCEPTED, Json(resp)).into_response()
        }
        Err(e) => (StatusCode::CONFLICT, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

/// `GET /v1/namespaces/{name}/index`
pub async fn get_index_status<O: IndexOps>(ops: &O, name: &str) -> Response {
    let ns_id = match ops.resolve(name).await {
        Some(id) => id,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": format!("collection '{}' not found", name)
                })),
            )
                .into_response();
        }
    };
    let state = ops.get_index_state(ns_id).await;
    let resp = IndexStatusResponse::from_state(name, &state);
    (StatusCode::OK, Json(resp)).into_response()
}

fn cluster_unsupported_response() -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "ANN index management is not yet supported in cluster mode",
            "note": "cluster nodes use exact brute-force search for linearizable consistency; standalone mode supports HNSW, IVF, and BQ"
        })),
    )
        .into_response()
}
