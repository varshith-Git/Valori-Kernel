// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Cluster-mode HTTP server — the data plane over Raft (v1).
//!
//! What a cluster node serves today:
//!
//! | Route | Behaviour |
//! |---|---|
//! | `POST /records` | insert → `client_write` through Raft; follower answers **307 + Location** to the leader |
//! | `POST /search` | brute-force k-NN over the replicated kernel — served locally on ANY node |
//! | `GET /health`, `GET /metrics` | cluster health / Prometheus |
//! | `/v1/cluster/*` | management plane (Phase 2.6) |
//!
//! Writes are async-native here (`Raft::client_write` directly) — the
//! sync `RaftCommitter` exists for the Engine seam, not for axum handlers.
//!
//! v1 scope, stated plainly: search is a brute-force scan of the kernel
//! state. The full Engine integration (HNSW/IVF indexes, graph endpoints,
//! batch, snapshots over the cluster) is the remaining Phase 2 follow-up;
//! this router makes a cluster node *usable* end to end, not feature-equal
//! with standalone.

use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use valori_consensus::types::Raft;
use valori_consensus::{ClientRequest, ValoriStateMachine};
use valori_kernel::dist::euclidean_distance_squared;
use valori_kernel::event::KernelEvent;
use valori_kernel::fxp::qformat::SCALE;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

use crate::cluster::ClusterHandle;
use crate::cluster_api::cluster_router;
use crate::events::event_log::EventLogWriter;

#[derive(Clone)]
struct DataPlaneState {
    raft: Arc<Raft>,
    sm: ValoriStateMachine,
    /// Serializes id-allocation + commit for local inserts. The kernel
    /// assigns sequential record ids, so concurrent inserts through one
    /// node race on the id read; each insert is a quorum round-trip
    /// anyway, so serializing costs little and eliminates 503s under
    /// burst load. The real fix — id allocation inside the state machine —
    /// is a kernel event-schema change, tracked as follow-up.
    insert_lock: Arc<tokio::sync::Mutex<()>>,
}

/// Bind a TCP port and serve the cluster data + management router on it.
///
/// Returns the actual bound address (useful when the caller passes port 0)
/// and a task handle. The caller must keep the handle alive; dropping it
/// aborts the server.
pub async fn serve_cluster_api(
    handle: &ClusterHandle,
    api_bind: &str,
    audit: Option<Arc<std::sync::Mutex<EventLogWriter>>>,
) -> Result<(std::net::SocketAddr, tokio::task::JoinHandle<()>), std::io::Error> {
    let router = build_cluster_router(handle, audit);
    let listener = tokio::net::TcpListener::bind(api_bind).await.map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!("cannot bind API to {api_bind}: {e}"),
        )
    })?;
    let addr = listener.local_addr()?;
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.ok();
    });
    Ok((addr, task))
}

/// The full router a cluster node serves: data plane + management plane.
pub fn build_cluster_router(
    handle: &ClusterHandle,
    audit: Option<Arc<std::sync::Mutex<EventLogWriter>>>,
) -> Router {
    let raft = Arc::new(handle.raft.clone());
    let state = DataPlaneState {
        raft: raft.clone(),
        sm: handle.state_machine.clone(),
        insert_lock: Arc::new(tokio::sync::Mutex::new(())),
    };

    Router::new()
        .route("/records", post(insert_record))
        .route("/search", post(search))
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .with_state(state)
        .merge(cluster_router(raft, audit))
}

async fn metrics() -> String {
    crate::telemetry::get_metrics()
}

async fn health(State(state): State<DataPlaneState>) -> Response {
    let m = state.raft.metrics().borrow().clone();
    match m.current_leader {
        Some(leader) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "ok", "leader": leader })),
        )
            .into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "status": "no-leader" })),
        )
            .into_response(),
    }
}

// ── Insert ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct InsertRequest {
    values: Vec<f32>,
    #[serde(default)]
    metadata: Option<Vec<u8>>,
    #[serde(default)]
    tag: u64,
    /// Client idempotency token (hex-free 16 bytes as array) — optional.
    #[serde(default)]
    request_id: Option<[u8; 16]>,
}

#[derive(Serialize)]
struct InsertResponse {
    id: u32,
    log_index: u64,
    deduplicated: bool,
}

fn to_fxp(values: &[f32]) -> Result<FxpVector, String> {
    let mut data = Vec::with_capacity(values.len());
    for &v in values {
        if !(-32768.0..=32767.99).contains(&v) {
            return Err("vector values must be between -32768.0 and 32767.99".into());
        }
        data.push(FxpScalar((v * SCALE as f32) as i32));
    }
    Ok(FxpVector { data })
}

fn not_leader_response(leader_node: Option<&valori_consensus::ValoriNode>) -> Response {
    let mut builder = Response::builder().status(StatusCode::TEMPORARY_REDIRECT);
    if let Some(n) = leader_node {
        if !n.api_addr.is_empty() {
            builder = builder.header(header::LOCATION, format!("http://{}", n.api_addr));
        }
    }
    builder
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            serde_json::json!({
                "error": "not-leader",
                "leader_api_addr": leader_node.map(|n| n.api_addr.clone()),
            })
            .to_string(),
        ))
        .unwrap()
}

async fn insert_record(
    State(state): State<DataPlaneState>,
    Json(req): Json<InsertRequest>,
) -> Response {
    let vector = match to_fxp(&req.values) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
    };

    // Hold the insert lock across allocate-id + commit (see DataPlaneState).
    // The retry loop below stays as belt-and-braces for cross-node races.
    let _guard = state.insert_lock.lock().await;
    for _attempt in 0..8 {
        let rid: RecordId = state.sm.with_state(|s| s.next_record_id()).await;
        let write = state
            .raft
            .client_write(ClientRequest {
                event: KernelEvent::InsertRecord {
                    id: rid,
                    vector: vector.clone(),
                    metadata: req.metadata.clone(),
                    tag: req.tag,
                },
                request_id: req.request_id,
            })
            .await;

        match write {
            Ok(resp) => {
                if let Some(reason) = &resp.data.rejected {
                    if reason.contains("InvalidOperation") {
                        continue; // id race — retry with a fresh id
                    }
                    return (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        Json(serde_json::json!({ "error": reason })),
                    )
                        .into_response();
                }
                return (
                    StatusCode::OK,
                    Json(InsertResponse {
                        id: rid.0,
                        log_index: resp.data.log_index,
                        deduplicated: resp.data.deduplicated,
                    }),
                )
                    .into_response();
            }
            Err(openraft::error::RaftError::APIError(
                openraft::error::ClientWriteError::ForwardToLeader(fwd),
            )) => return not_leader_response(fwd.leader_node.as_ref()),
            Err(e) => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({ "error": format!("raft write failed: {e}") })),
                )
                    .into_response()
            }
        }
    }

    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({ "error": "insert id contention — retry" })),
    )
        .into_response()
}

// ── Search ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SearchRequest {
    query: Vec<f32>,
    #[serde(default = "default_k")]
    k: usize,
}

fn default_k() -> usize {
    10
}

#[derive(Serialize)]
struct SearchHit {
    id: u32,
    distance_sq: i64,
}

async fn search(
    State(state): State<DataPlaneState>,
    Json(req): Json<SearchRequest>,
) -> Response {
    let query = match to_fxp(&req.query) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
    };
    let q: Vec<i32> = query.data.iter().map(|s| s.0).collect();

    // Reads are served LOCALLY on any node — this is where the replicas'
    // RAM pays for itself. Brute force in v1; index-backed in the Engine
    // integration.
    let mut hits: Vec<SearchHit> = state
        .sm
        .with_state(|s| {
            let mut out = Vec::new();
            for i in 0..s.total_record_slots() {
                if let Some(rec) = s.get_record(RecordId(i as u32)) {
                    if rec.flags & valori_kernel::storage::record::FLAG_SOFT_DELETED != 0 {
                        continue;
                    }
                    if rec.vector.data.len() != q.len() {
                        continue;
                    }
                    let v: Vec<i32> = rec.vector.data.iter().map(|s| s.0).collect();
                    out.push(SearchHit {
                        id: rec.id.0,
                        distance_sq: euclidean_distance_squared(&q, &v),
                    });
                }
            }
            out
        })
        .await;

    hits.sort_by_key(|h| h.distance_sq);
    hits.truncate(req.k);
    (StatusCode::OK, Json(serde_json::json!({ "hits": hits }))).into_response()
}
