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
use valori_kernel::event::KernelEvent;
use valori_kernel::fxp::qformat::SCALE;
use valori_kernel::index::SearchResult as KernelSearchResult;
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
    /// Reused for the follower→leader read-index round trip on linearizable
    /// reads. Cloning a reqwest::Client is cheap and shares the connection pool.
    http: reqwest::Client,
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
        http: reqwest::Client::new(),
    };

    Router::new()
        .route("/records", post(insert_record))
        .route("/search", post(search))
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .route("/v1/delete", post(delete_record))
        .route("/v1/soft-delete", post(soft_delete_record))
        .route("/v1/vectors/batch_insert", post(batch_insert))
        .route("/v1/proof/state", get(state_proof))
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

// ── Shared Raft write helper ──────────────────────────────────────────────────

/// Submit a `ClientRequest` to the Raft leader and map the response.
/// Handles the ForwardToLeader redirect and generic Raft errors uniformly.
async fn raft_write<F>(
    raft: &Raft,
    req: ClientRequest,
    on_ok: F,
) -> Response
where
    F: FnOnce(valori_consensus::ClientResponse) -> Response,
{
    match raft.client_write(req).await {
        Ok(resp) => {
            if let Some(reason) = &resp.data.rejected {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({ "error": reason })),
                )
                    .into_response();
            }
            on_ok(resp.data)
        }
        Err(openraft::error::RaftError::APIError(
            openraft::error::ClientWriteError::ForwardToLeader(fwd),
        )) => not_leader_response(fwd.leader_node.as_ref()),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": format!("raft write failed: {e}") })),
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
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))).into_response();
        }
    };

    // ID is assigned by the state machine at apply time (AutoInsertRecord).
    // No per-node mutex or retry loop needed — the Raft log is the serialiser.
    raft_write(
        &state.raft,
        ClientRequest {
            event: KernelEvent::AutoInsertRecord {
                vector,
                metadata: req.metadata,
                tag: req.tag,
            },
            request_id: req.request_id,
        },
        |resp| {
            (
                StatusCode::OK,
                Json(InsertResponse {
                    id: resp.allocated_record_id.unwrap_or(0),
                    log_index: resp.log_index,
                    deduplicated: resp.deduplicated,
                }),
            )
                .into_response()
        },
    )
    .await
}

// ── Search ────────────────────────────────────────────────────────────────────

/// Read consistency level for a query.
///
/// `Linearizable` (the default) guarantees the result reflects every write
/// committed before the read began — via the read-index protocol. `Local`
/// serves immediately from this node's state, which may lag the leader
/// (eventually consistent) but skips the read-index round trip.
#[derive(Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Consistency {
    #[default]
    Linearizable,
    Local,
}

#[derive(Deserialize)]
struct SearchRequest {
    query: Vec<f32>,
    #[serde(default = "default_k")]
    k: usize,
    #[serde(default)]
    consistency: Consistency,
}

fn default_k() -> usize {
    10
}

// Wire-compatible with the standalone server's SearchHit { id, score }
// (api.rs) so one SDK client speaks to both standalone and cluster nodes.
#[derive(Serialize)]
struct SearchHit {
    id: u32,
    score: i64,
}

async fn search(
    State(state): State<DataPlaneState>,
    Json(req): Json<SearchRequest>,
) -> Response {
    let query = match to_fxp(&req.query) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))).into_response();
        }
    };

    // Linearizable reads (the default) establish a read index first, so the
    // local scan below reflects every write committed before this read began.
    if req.consistency == Consistency::Linearizable {
        if let Err(resp) = ensure_read_consistency(&state.raft, &state.http).await {
            return resp;
        }
    }

    let k = req.k.max(1);
    // Reads are served LOCALLY — replicas' RAM pays for itself.
    // search_l2 delegates to the kernel's BruteForceIndex, which is kept
    // up to date by every apply (on_insert / on_delete are called inside
    // KernelState::apply). Results arrive pre-sorted ascending by score.
    let results: Vec<SearchHit> = state
        .sm
        .with_state(|s| {
            let mut buf = vec![KernelSearchResult::default(); k];
            let n = s.search_l2(&query, &mut buf, None);
            buf[..n]
                .iter()
                .map(|r| SearchHit {
                    id: r.id.0,
                    score: r.score.0 as i64,
                })
                .collect()
        })
        .await;

    (StatusCode::OK, Json(serde_json::json!({ "results": results }))).into_response()
}

// ── Read consistency (read-index protocol) ──────────────────────────────────────

fn read_unavailable(msg: String) -> Response {
    (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({ "error": msg }))).into_response()
}

/// Block until this node may serve a linearizable read.
///
/// - **Leader**: `ensure_linearizable` confirms leadership via a quorum
///   heartbeat and waits for this node's apply to reach the read index.
/// - **Follower**: ask the leader for its read index (`/v1/cluster/read-index`),
///   then wait until this node's applied index catches up before returning.
///
/// On success the caller may scan local state and the result is linearizable.
async fn ensure_read_consistency(raft: &Raft, http: &reqwest::Client) -> Result<(), Response> {
    // Snapshot the metrics into owned values so no watch borrow is held across
    // an await point.
    let m = raft.metrics().borrow().clone();
    let my_id = m.id;
    let leader_id = match m.current_leader {
        Some(l) => l,
        None => {
            return Err(read_unavailable(
                "no elected leader — cannot serve a linearizable read".into(),
            ))
        }
    };

    if leader_id == my_id {
        // We are the leader: this confirms leadership and waits for apply.
        return raft
            .ensure_linearizable()
            .await
            .map(|_| ())
            .map_err(|e| read_unavailable(format!("linearizable read failed on leader: {e}")));
    }

    // Follower path: fetch the leader's read index, then wait to catch up.
    let leader_api = m
        .membership_config
        .nodes()
        .find(|(id, _)| **id == leader_id)
        .map(|(_, n)| n.api_addr.clone())
        .filter(|a| !a.is_empty());
    let leader_api = match leader_api {
        Some(a) => a,
        None => {
            return Err(read_unavailable(
                "leader API address unknown — cannot run the read-index protocol".into(),
            ))
        }
    };

    let url = format!("http://{leader_api}/v1/cluster/read-index");
    let read_index = match http
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => match r.json::<serde_json::Value>().await {
            Ok(v) => v.get("read_index").and_then(|x| x.as_u64()).unwrap_or(0),
            Err(e) => return Err(read_unavailable(format!("bad read-index reply from leader: {e}"))),
        },
        Ok(r) => {
            return Err(read_unavailable(format!(
                "leader rejected read-index ({})",
                r.status()
            )))
        }
        Err(e) => return Err(read_unavailable(format!("cannot reach leader for read-index: {e}"))),
    };

    // Wait until our local apply has reached the leader's read index.
    raft.wait(Some(std::time::Duration::from_secs(5)))
        .applied_index_at_least(Some(read_index), "linearizable-read")
        .await
        .map(|_| ())
        .map_err(|e| read_unavailable(format!("timed out catching up to read index {read_index}: {e}")))
}

// ── Delete ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct DeleteRequest {
    id: u32,
}

async fn delete_record(
    State(state): State<DataPlaneState>,
    Json(req): Json<DeleteRequest>,
) -> Response {
    raft_write(
        &state.raft,
        ClientRequest {
            event: KernelEvent::DeleteRecord { id: RecordId(req.id) },
            request_id: None,
        },
        |resp| {
            (StatusCode::OK, Json(serde_json::json!({
                "success": true,
                "log_index": resp.log_index,
            })))
                .into_response()
        },
    )
    .await
}

async fn soft_delete_record(
    State(state): State<DataPlaneState>,
    Json(req): Json<DeleteRequest>,
) -> Response {
    raft_write(
        &state.raft,
        ClientRequest {
            event: KernelEvent::SoftDeleteRecord { id: RecordId(req.id) },
            request_id: None,
        },
        |resp| {
            (StatusCode::OK, Json(serde_json::json!({
                "success": true,
                "log_index": resp.log_index,
            })))
                .into_response()
        },
    )
    .await
}

// ── Batch insert ──────────────────────────────────────────────────────────────
// Wire-compatible with the standalone server: request `{ batch: [[f32]] }`,
// response `{ ids: [u32] }`. Any rejected vector fails the whole batch with a
// 422 (the standalone engine is all-or-nothing too).

#[derive(Deserialize)]
struct BatchInsertRequest {
    batch: Vec<Vec<f32>>,
}

async fn batch_insert(
    State(state): State<DataPlaneState>,
    Json(req): Json<BatchInsertRequest>,
) -> Response {
    let mut ids = Vec::with_capacity(req.batch.len());

    for values in req.batch {
        let vector = match to_fxp(&values) {
            Ok(v) => v,
            Err(e) => {
                return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e })))
                    .into_response();
            }
        };

        match state
            .raft
            .client_write(ClientRequest {
                event: KernelEvent::AutoInsertRecord { vector, metadata: None, tag: 0 },
                request_id: None,
            })
            .await
        {
            Ok(resp) => {
                if let Some(reason) = &resp.data.rejected {
                    return (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({ "error": reason })))
                        .into_response();
                }
                ids.push(resp.data.allocated_record_id.unwrap_or(0));
            }
            Err(openraft::error::RaftError::APIError(
                openraft::error::ClientWriteError::ForwardToLeader(fwd),
            )) => return not_leader_response(fwd.leader_node.as_ref()),
            Err(e) => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({ "error": format!("raft write failed: {e}") })),
                )
                    .into_response();
            }
        }
    }

    (StatusCode::OK, Json(serde_json::json!({ "ids": ids }))).into_response()
}

// ── State proof ───────────────────────────────────────────────────────────────
// `final_state_hash` matches the standalone DeterministicProof field name the
// SDK reads, so `get_state_hash()` works unchanged against a cluster node.

async fn state_proof(State(state): State<DataPlaneState>) -> Response {
    let hash = state.sm.state_hash().await;
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    (StatusCode::OK, Json(serde_json::json!({ "final_state_hash": hex }))).into_response()
}
