// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use axum::{
    routing::{post, delete, get},
    Router,
    extract::{State, Path as AxumPath, Extension},
    Json,
    body::Body,
};
use tower_http::cors::{CorsLayer, Any};
use tokio_util::io::ReaderStream;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::engine::Engine;
use crate::api::*;
use crate::errors::EngineError;
use crate::api_keys::{ApiScope, AuthState, KeyStore, required_scope};
use crate::crypto_vault::{hex_to_key_id, key_id_to_hex, new_key_id};
use serde::{Deserialize, Serialize};

/// Phase 3.11: RwLock-backed engine — allows concurrent reads.
/// Read handlers call `.read().await`; write handlers call `.write().await`.
pub type SharedEngine = Arc<RwLock<Engine>>;

use valori_kernel::types::enums::{NodeKind, EdgeKind};
use axum::extract::Query;
use axum::middleware::Next;
use axum::response::{Response, IntoResponse};
use axum::http::StatusCode;
use axum::extract::Request as AxumRequest;
use axum::http::header::AUTHORIZATION;

async fn auth_guard_v2(
    Extension(auth): Extension<Arc<AuthState>>,
    req: AxumRequest,
    next: Next,
) -> Result<Response, StatusCode> {
    if !auth.has_any_auth() {
        return Ok(next.run(req).await);
    }
    let path = req.uri().path().to_string();
    let method = req.method().clone();
    let required = required_scope(&method, &path);

    let bearer = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let Some(token) = bearer else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    // Key store check first.
    if let Some(record) = auth.key_store.lookup(token) {
        if record.scope.satisfies(&required) {
            return Ok(next.run(req).await);
        }
        return Err(StatusCode::FORBIDDEN);
    }

    // Legacy static token fallback — treated as admin.
    if let Some(ref legacy) = auth.legacy_token {
        if token == legacy {
            return Ok(next.run(req).await);
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

fn make_cors_layer(origin: &Option<String>) -> Option<CorsLayer> {
    let origin = origin.as_deref()?;
    let layer = if origin == "*" {
        CorsLayer::permissive()
    } else {
        let hv: axum::http::HeaderValue = origin
            .parse()
            .expect("VALORI_CORS_ORIGIN is not a valid HTTP header value");
        CorsLayer::new()
            .allow_origin(hv)
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::DELETE,
                axum::http::Method::OPTIONS,
            ])
            .allow_headers(Any)
    };
    Some(layer)
}

/// Build a standalone HTTP router.  Existing callers pass `None` for `key_store`;
/// use [`build_router_with_keys`] from `main.rs` to enable Phase 3.5 key management.
pub fn build_router(
    state: SharedEngine,
    auth_token: Option<String>,
    cors_origin: Option<String>,
) -> Router {
    build_router_with_keys(state, auth_token, cors_origin, Arc::new(KeyStore::new(None)))
}

/// Full router builder used by `main.rs` — supports per-tenant API keys.
pub fn build_router_with_keys(
    state: SharedEngine,
    auth_token: Option<String>,
    cors_origin: Option<String>,
    key_store: Arc<KeyStore>,
) -> Router {
    // ── Public routes — no auth required ─────────────────────────────────────
    let public = Router::new()
        .route("/health",  axum::routing::get(health_check))
        .route("/metrics", axum::routing::get(metrics_handler))
        .with_state(state.clone());

    // ── Key management routes (admin scope enforced by middleware) ────────────
    let key_routes = Router::new()
        .route("/v1/keys", post(create_key_handler).get(list_keys_handler))
        .route("/v1/keys/:id", delete(revoke_key_handler));

    // ── Protected routes ──────────────────────────────────────────────────────
    let protected = Router::new()
        .route("/version", axum::routing::get(version_handler))
        .route("/records", post(insert_record))
        .route("/v1/delete", post(delete_record))
        .route("/v1/vectors/batch_insert", post(batch_insert))
        .route("/search", post(search))
        .route("/graph/node", post(create_node))
        .route("/graph/node/:id", axum::routing::get(get_node).delete(delete_node))
        .route("/graph/nodes", axum::routing::get(list_nodes))
        .route("/graph/edge", post(create_edge))
        .route("/graph/edges/:id", axum::routing::get(get_edges))
        .route("/graph/subgraph", axum::routing::get(get_subgraph))
        .route("/v1/graphrag", post(graphrag))
        .route("/v1/snapshot/download", axum::routing::get(snapshot))
        .route("/v1/snapshot/upload", post(restore))
        .route("/v1/snapshot/save", post(snapshot_save))
        .route("/v1/snapshot/restore", post(snapshot_restore))
        .route("/v1/memory/upsert_vector", post(memory_upsert_vector))
        .route("/v1/memory/search_vector", post(memory_search_vector))
        .route("/v1/memory/meta/set", post(meta_set))
        .route("/v1/memory/meta/get", axum::routing::get(meta_get))
        .route("/v1/proof/state", axum::routing::get(get_proof))
        .route("/v1/proof/event-log", axum::routing::get(get_event_proof))
        .route("/v1/replication/wal", axum::routing::get(get_wal_stream))
        .route("/v1/replication/events", axum::routing::get(get_replication_events))
        .route("/v1/replication/state", axum::routing::get(get_replication_state))
        .route("/timeline", axum::routing::get(get_timeline))
        .route("/v1/timeline", axum::routing::get(get_timeline))
        .route("/v1/namespaces", post(create_collection_handler).get(list_collections_handler))
        .route("/v1/namespaces/:name", delete(drop_collection_handler))
        .route("/v1/storage/snapshots", axum::routing::get(list_remote_snapshots))
        .route("/v1/storage/snapshots/upload", post(upload_snapshot_to_store))
        .route("/v1/storage/snapshots/restore", post(restore_from_store))
        .route("/v1/storage/wal", axum::routing::get(list_remote_wal))
        .route("/v1/storage/wal/archive", post(archive_wal_segment))
        // Crypto-shredding (Phase 3.6)
        .route("/v1/records/encrypted", post(insert_encrypted_handler))
        .route("/v1/crypto/shred/:key_id", delete(shred_key_handler))
        .route("/v1/crypto/status/:key_id", get(crypto_status_handler))
        // Index config (Phase 3.13)
        .route("/v1/index/config", axum::routing::get(index_config_handler))
        .merge(key_routes)
        .with_state(state);

    let auth = Arc::new(AuthState {
        key_store: key_store.clone(),
        legacy_token: auth_token,
    });
    if auth.has_any_auth() {
        tracing::info!("Auth Enabled");
    } else {
        tracing::warn!("Auth Disabled: no token or keys configured");
    }

    // Extension must be the outermost layer (applied last) so it is injected
    // into the request BEFORE auth_guard_v2 runs and tries to extract it.
    let protected = protected
        .layer(axum::middleware::from_fn(auth_guard_v2))
        .layer(Extension(auth));

    let mut router = Router::new().merge(public).merge(protected);
    if let Some(cors) = make_cors_layer(&cors_origin) {
        tracing::info!("CORS enabled: origin = {:?}", cors_origin);
        router = router.layer(cors);
    }
    router
}

/// `GET /health` — structured health report for load balancers and operators.
///
/// HTTP status codes:
/// * **200** `"ok"`       — all pools below 90 % capacity
/// * **200** `"degraded"` — at least one pool ≥ 90 %; still serving all requests
/// * **503** `"full"`     — at least one pool at 100 %; inserts are being rejected
///
/// This endpoint is **always unauthenticated** so that load-balancer health
/// probes and liveness checks work without a bearer token.
async fn health_check(
    State(state): State<SharedEngine>,
) -> impl IntoResponse {
    let engine = state.read().await;
    let h = engine.health();

    // Refresh Prometheus gauges on every health probe — cheap, and it means
    // the /metrics endpoint always reflects the latest state even between
    // heavy write bursts.
    engine.update_prometheus_metrics();

    let status_code = if h.status == "full" {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };

    (status_code, Json(h))
}

async fn version_handler() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

async fn delete_record(
    State(state): State<SharedEngine>,
    Json(payload): Json<DeleteRecordRequest>,
) -> Result<Json<DeleteRecordResponse>, EngineError> {
    let mut engine = state.write().await;
    engine.resolve_collection(payload.collection.as_deref())?;
    engine.delete_record(payload.id)?;

    Ok(Json(DeleteRecordResponse { success: true }))
}

async fn snapshot_save(
    State(state): State<SharedEngine>,
    Json(req): Json<SnapshotSaveRequest>,
) -> Result<Json<SnapshotSaveResponse>, EngineError> {
    let engine = state.read().await;
    let path = req.path.map(std::path::PathBuf::from);
    let used_path = engine.save_snapshot(path.as_deref())?;
    
    Ok(Json(SnapshotSaveResponse {
        success: true,
        path: used_path.to_string_lossy().to_string(),
    }))
}

async fn snapshot_restore(
    State(state): State<SharedEngine>,
    Json(req): Json<SnapshotRestoreRequest>,
) -> Result<Json<SnapshotRestoreResponse>, EngineError> {
    let mut engine = state.write().await;
    let path = std::path::PathBuf::from(req.path);
    
    if !path.exists() {
        return Err(EngineError::InvalidInput(format!("Snapshot not found at {:?}", path)));
    }
    
    let data = tokio::fs::read(&path).await.map_err(|e| EngineError::InvalidInput(e.to_string()))?;
    engine.restore(&data)?;
    
    Ok(Json(SnapshotRestoreResponse { success: true }))
}

async fn meta_set(
    State(state): State<SharedEngine>,
    Json(payload): Json<MetadataSetRequest>,
) -> Result<Json<MetadataSetResponse>, EngineError> {
    let engine = state.read().await;
    engine.metadata.set(payload.target_id, payload.metadata);
    if let Err(e) = engine.flush_metadata() {
        tracing::warn!("meta_set: failed to persist metadata sidecar: {:?}", e);
    }
    Ok(Json(MetadataSetResponse { success: true }))
}

async fn meta_get(
    State(state): State<SharedEngine>,
    Query(payload): Query<MetadataGetRequest>,
) -> Result<Json<MetadataGetResponse>, EngineError> {
    let engine = state.read().await;
    let val = engine.metadata.get(&payload.target_id);
    Ok(Json(MetadataGetResponse {
        target_id: payload.target_id,
        metadata: val,
    }))
}

async fn insert_record(
    State(state): State<SharedEngine>,
    Json(payload): Json<InsertRecordRequest>,
) -> Result<Json<InsertRecordResponse>, EngineError> {
    let mut engine = state.write().await;
    let ns = engine.resolve_collection(payload.collection.as_deref())?;
    let id = engine.insert_record_from_f32_ns(&payload.values, ns)?;
    Ok(Json(InsertRecordResponse { id }))
}

async fn batch_insert(
    State(state): State<SharedEngine>,
    Json(payload): Json<BatchInsertRequest>,
) -> Result<Json<BatchInsertResponse>, EngineError> {
    let mut engine = state.write().await;
    let ns = engine.resolve_collection(payload.collection.as_deref())?;
    let meta_bytes: Option<Vec<Option<Vec<u8>>>> = payload.metadata.as_ref().map(|m| {
        m.iter().map(|s| s.as_ref().map(|s| s.as_bytes().to_vec())).collect()
    });
    // Parse optional per-item idempotency keys from 32-hex strings to [u8;16].
    let parsed_request_ids: Option<Vec<Option<[u8; 16]>>> =
        payload.request_ids.as_ref().map(|rids| {
            rids.iter().map(|entry| {
                entry.as_deref().and_then(|hex| {
                    if hex.len() != 32 { return None; }
                    let mut bytes = [0u8; 16];
                    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
                        bytes[i] = u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
                    }
                    Some(bytes)
                })
            }).collect()
        });
    let ids = engine.insert_batch_ns(
        &payload.batch,
        meta_bytes.as_deref(),
        ns,
        parsed_request_ids.as_deref(),
    )?;
    Ok(Json(BatchInsertResponse { ids }))
}

async fn search(
    State(state): State<SharedEngine>,
    Json(payload): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, EngineError> {
    if payload.as_of.is_some() || payload.as_of_log_index.is_some() {
        return search_as_of(state, payload).await;
    }
    let engine = state.read().await;
    let ns = engine.resolve_collection(payload.collection.as_deref())?;

    // Effective decay half-life: request value wins (incl. an explicit 0 to
    // disable), else the server default. 0 / None => pure distance ranking.
    let half_life = payload.decay_half_life_secs.or(engine.decay_half_life_secs).unwrap_or(0);

    if half_life == 0 {
        let hits = if ns == 0 {
            engine.search_l2(&payload.query, payload.k)?
        } else {
            engine.search_l2_ns(&payload.query, payload.k, ns)?
        };
        let results = hits.into_iter()
            .map(|(id, score)| SearchHit { id, score, decay_factor: None, age_secs: None })
            .collect();
        return Ok(Json(SearchResponse::simple(results)));
    }

    // Decay path: over-fetch a bounded pool, re-rank by decayed distance,
    // then trim to k. This lets a fresh near-match overtake a stale better one.
    let pool = payload.k.saturating_mul(4).max(50).min(1000);
    let raw = if ns == 0 {
        engine.search_l2(&payload.query, pool)?
    } else {
        engine.search_l2_ns(&payload.query, pool, ns)?
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs()).unwrap_or(0);
    let candidates: Vec<crate::decay::DecayHit> = raw.into_iter()
        .map(|(id, score)| crate::decay::DecayHit {
            id,
            distance: score,
            created_at: engine.record_created_at(id),
        })
        .collect();
    let results = crate::decay::rerank(candidates, now, half_life, payload.k)
        .into_iter()
        .map(|h| SearchHit {
            id: h.id,
            score: h.distance,
            decay_factor: Some(h.factor),
            age_secs: h.age_secs,
        })
        .collect();
    Ok(Json(SearchResponse::simple(results)))
}

/// Point-in-time search: replay committed events up to the target index/timestamp,
/// run the search on the replayed state, and return the results with a BLAKE3 proof.
async fn search_as_of(
    state: SharedEngine,
    payload: SearchRequest,
) -> Result<Json<SearchResponse>, EngineError> {
    use valori_kernel::state::kernel::KernelState;
    use valori_kernel::index::SearchResult;
    use valori_kernel::types::scalar::FxpScalar;
    use valori_kernel::types::vector::FxpVector;
    use valori_kernel::fxp::qformat::SCALE;
    use valori_kernel::snapshot::blake3::hash_state_blake3;

    let engine = state.read().await;

    let committer = engine.event_committer.as_ref().ok_or_else(|| {
        EngineError::InvalidInput(
            "as-of search requires the event log (set VALORI_EVENT_LOG_PATH)".into(),
        )
    })?;
    let journal = committer.journal();

    // Determine target log index and the corresponding timestamp.
    let (target_idx, timestamp_unix) = if let Some(idx) = payload.as_of_log_index {
        let ts = journal.event_timestamp(idx as usize).unwrap_or(0);
        (idx as usize, ts)
    } else {
        // Parse the ISO 8601 timestamp.
        let unix = parse_iso8601(payload.as_of.as_deref().unwrap_or(""))
            .ok_or_else(|| EngineError::InvalidInput(
                "invalid as_of timestamp — expected ISO 8601 UTC, e.g. 2026-03-03T00:00:00Z".into(),
            ))?;
        match journal.find_log_index_at_or_before(unix) {
            Some(idx) => (idx, unix),
            None => {
                // No events at or before the requested time → empty state.
                return Ok(Json(SearchResponse {
                    results: vec![],
                    as_of_log_index: Some(0),
                    as_of_timestamp_unix: Some(unix),
                    as_of_timestamp_iso: Some(unix_to_iso8601(unix)),
                    as_of_state_hash: Some(bytes_to_hex(&[0u8; 32])),
                }));
            }
        }
    };

    let events = journal.committed();
    if target_idx >= events.len() {
        return Err(EngineError::InvalidInput(format!(
            "as_of_log_index {target_idx} is out of range (have {} events)",
            events.len()
        )));
    }

    // Replay events[0..=target_idx] into a fresh kernel.
    let mut replay = KernelState::new();
    for event in &events[0..=target_idx] {
        let _ = replay.apply_event(event);
    }

    // Resolve namespace in the *replayed* state via the engine's registry
    // (namespace registry is separate from kernel state and not replayed here).
    let ns = engine.resolve_collection(payload.collection.as_deref())?;

    // Convert f32 query to Q16.16 FxpVector.
    for &v in &payload.query {
        if v > 32767.99 || v < -32768.0 {
            return Err(EngineError::InvalidInput(
                "query values must be in [-32768.0, 32767.99]".into(),
            ));
        }
    }
    let fxp_data: Vec<FxpScalar> = payload.query.iter()
        .map(|&v| FxpScalar((v * SCALE as f32) as i32))
        .collect();
    let fxp_query = FxpVector { data: fxp_data };

    let k = payload.k;
    let mut results_buf = vec![SearchResult::default(); k];
    let found = if ns == 0 {
        replay.search_l2(&fxp_query, &mut results_buf, None)
    } else {
        replay.search_l2_ns(&fxp_query, &mut results_buf, ns)
    };
    let results: Vec<SearchHit> = results_buf[..found].iter().map(|r| {
        let score = r.score.0 as f32 / (SCALE as f32 * SCALE as f32);
        // Decay is a "now"-relative re-rank; it is intentionally NOT applied to
        // point-in-time (as_of) queries, which reconstruct a historical state.
        SearchHit { id: r.id.0, score, decay_factor: None, age_secs: None }
    }).collect();

    let state_hash_bytes = hash_state_blake3(&replay);
    let state_hash_hex = bytes_to_hex(&state_hash_bytes);

    Ok(Json(SearchResponse {
        results,
        as_of_log_index: Some(target_idx as u64),
        as_of_timestamp_unix: Some(timestamp_unix),
        as_of_timestamp_iso: Some(unix_to_iso8601(timestamp_unix)),
        as_of_state_hash: Some(state_hash_hex),
    }))
}

fn bytes_to_hex(b: &[u8]) -> String {
    b.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Parse a subset of ISO 8601 UTC: `YYYY-MM-DDTHH:MM:SSZ` or `YYYY-MM-DDTHH:MM:SS+00:00`.
/// Returns unix seconds since the epoch.
fn parse_iso8601(s: &str) -> Option<u64> {
    let s = s.trim();
    // Require at least "YYYY-MM-DDTHH:MM:SS"
    if s.len() < 19 { return None; }
    let year:  u64 = s[0..4].parse().ok()?;
    let month: u64 = s[5..7].parse().ok()?;
    let day:   u64 = s[8..10].parse().ok()?;
    let hour:  u64 = s[11..13].parse().ok()?;
    let min:   u64 = s[14..16].parse().ok()?;
    let sec:   u64 = s[17..19].parse().ok()?;
    if s.as_bytes().get(10) != Some(&b'T') { return None; }

    // Leap-year calculation for days-since-epoch.
    // Months → cumulative days (non-leap year).
    const DAYS_IN_MONTH: [u64; 13] = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let is_leap = |y: u64| y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);

    // Days from 1970-01-01 to start of `year`.
    let mut days: i64 = 0;
    for y in 1970..year {
        days += if is_leap(y) { 366 } else { 365 };
    }
    // Add days for completed months in current year.
    for m in 1..month {
        let extra = if m == 2 && is_leap(year) { 1 } else { 0 };
        days += DAYS_IN_MONTH[m as usize] as i64 + extra;
    }
    days += day as i64 - 1; // 1-indexed day

    if days < 0 { return None; }
    Some(days as u64 * 86400 + hour * 3600 + min * 60 + sec)
}

/// Format unix seconds as `YYYY-MM-DDTHH:MM:SSZ` (UTC only).
pub fn unix_to_iso8601(unix_secs: u64) -> String {
    let mut rem = unix_secs;
    let sec = rem % 60; rem /= 60;
    let min = rem % 60; rem /= 60;
    let hour = rem % 24; rem /= 24;

    // Days since 1970-01-01.
    let is_leap = |y: u64| y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    const DAYS_IN_MONTH: [u64; 13] = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

    let mut year = 1970u64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if rem < days_in_year { break; }
        rem -= days_in_year;
        year += 1;
    }
    let mut month = 1u64;
    loop {
        let dim = DAYS_IN_MONTH[month as usize] + if month == 2 && is_leap(year) { 1 } else { 0 };
        if rem < dim { break; }
        rem -= dim;
        month += 1;
    }
    let day = rem + 1;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

async fn create_node(
    State(state): State<SharedEngine>,
    Json(payload): Json<CreateNodeRequest>,
) -> Result<Json<CreateNodeResponse>, EngineError> {
    let mut engine = state.write().await;
    let ns = engine.resolve_collection(payload.collection.as_deref())?;
    let node_id = engine.create_node_for_record(payload.record_id, payload.kind, ns)?;
    Ok(Json(CreateNodeResponse { node_id }))
}

async fn create_edge(
    State(state): State<SharedEngine>,
    Json(payload): Json<CreateEdgeRequest>,
) -> Result<Json<CreateEdgeResponse>, EngineError> {
    let mut engine = state.write().await;
    engine.resolve_collection(payload.collection.as_deref())?;
    let edge_id = engine.create_edge(payload.from, payload.to, payload.kind)?;
    Ok(Json(CreateEdgeResponse { edge_id }))
}

async fn get_node(
    State(state): State<SharedEngine>,
    axum::extract::Path(id): axum::extract::Path<u32>,
) -> Result<Json<GetNodeResponse>, EngineError> {
    let engine = state.read().await;
    use valori_kernel::types::id::NodeId;
    match engine.state.get_node(NodeId(id)) {
        Some(node) => Ok(Json(GetNodeResponse {
            kind: node.kind as u8,
            record_id: node.record.map(|r| r.0),
            namespace_id: node.namespace_id,
        })),
        None => Err(EngineError::Kernel(valori_kernel::error::KernelError::NotFound)),
    }
}

async fn delete_node(
    State(state): State<SharedEngine>,
    axum::extract::Path(id): axum::extract::Path<u32>,
) -> Result<Json<DeleteNodeResponse>, EngineError> {
    let mut engine = state.write().await;
    engine.delete_node(id)?;
    Ok(Json(DeleteNodeResponse { success: true }))
}

#[derive(serde::Deserialize)]
struct ListNodesQuery {
    collection: Option<String>,
}

async fn list_nodes(
    State(state): State<SharedEngine>,
    Query(q): Query<ListNodesQuery>,
) -> Result<Json<ListNodesResponse>, EngineError> {
    let engine = state.read().await;
    let ns = engine.resolve_collection(q.collection.as_deref())?;
    let raw = engine.nodes_in_ns(ns);
    let nodes = raw
        .into_iter()
        .map(|(node_id, kind, record_id)| NodeInfo { node_id, kind, record_id, namespace_id: ns })
        .collect::<Vec<_>>();
    let count = nodes.len();
    Ok(Json(ListNodesResponse { nodes, count }))
}

async fn get_edges(
    State(state): State<SharedEngine>,
    axum::extract::Path(id): axum::extract::Path<u32>,
) -> Result<Json<GetEdgesResponse>, EngineError> {
    let engine = state.read().await;
    use valori_kernel::types::id::NodeId;
    
    let mut edges = Vec::new();
    if let Some(iter) = engine.state.outgoing_edges(NodeId(id)) {
        for edge in iter {
            edges.push(EdgeData {
                edge_id: edge.id.0,
                to_node: edge.to.0,
                kind: edge.kind as u8,
            });
        }
    }
    Ok(Json(GetEdgesResponse { edges }))
}

#[derive(serde::Deserialize)]
struct SubgraphQuery {
    root: u32,
    #[serde(default = "default_depth")]
    depth: u32,
}
fn default_depth() -> u32 { 2 }

async fn get_subgraph(
    State(state): State<SharedEngine>,
    Query(q): Query<SubgraphQuery>,
) -> impl IntoResponse {
    let engine = state.read().await;
    let (nodes_out, edges_out) =
        crate::graph_rag::expand_subgraph(&engine.state, &[q.root], q.depth);
    (StatusCode::OK, Json(serde_json::json!({ "nodes": nodes_out, "edges": edges_out })))
}

// ── Phase 3.15: native GraphRAG — KNN + subgraph expansion in one call ────────

#[derive(serde::Deserialize)]
struct GraphRagRequest {
    query_vector: Vec<f32>,
    k: usize,
    #[serde(default = "default_depth")]
    depth: u32,
    #[serde(default)]
    collection: Option<String>,
}

/// Retrieve the K nearest memories AND the knowledge subgraph around them, in a
/// single read against one consistent kernel snapshot. No second store, no sync.
async fn graphrag(
    State(state): State<SharedEngine>,
    Json(payload): Json<GraphRagRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let engine = state.read().await;
    let ns = engine.resolve_collection(payload.collection.as_deref())?;
    let hits = engine.search_l2_ns(&payload.query_vector, payload.k, ns)?;

    let mut seeds: Vec<u32> = Vec::new();
    let mut hits_out: Vec<serde_json::Value> = Vec::new();
    for (record_id, score) in &hits {
        let node_id = engine.record_to_node.get(record_id).copied();
        if let Some(nid) = node_id {
            seeds.push(nid);
        }
        let memory_id = format!("rec:{record_id}");
        let metadata = engine.metadata.get(&memory_id);
        hits_out.push(serde_json::json!({
            "memory_id": memory_id,
            "record_id": record_id,
            "score": score,
            "node_id": node_id,
            "metadata": metadata,
        }));
    }

    let (nodes, edges) = crate::graph_rag::expand_subgraph(&engine.state, &seeds, payload.depth);

    Ok(Json(serde_json::json!({
        "hits": hits_out,
        "seed_nodes": seeds,
        "subgraph": { "nodes": nodes, "edges": edges },
    })))
}

async fn snapshot(
    State(state): State<SharedEngine>,
) -> Result<Vec<u8>, EngineError> {
    let engine = state.read().await;
    engine.snapshot()
}

async fn restore(
    State(state): State<SharedEngine>,
    body: axum::body::Bytes,
) -> Result<(), EngineError> {
    let mut engine = state.write().await;
    engine.restore(&body)?;
    Ok(())
}

async fn memory_upsert_vector(
    State(state): State<SharedEngine>,
    Json(payload): Json<MemoryUpsertVectorRequest>,
) -> Result<Json<MemoryUpsertResponse>, EngineError> {
    let mut engine = state.write().await;
    let ns = engine.resolve_collection(payload.collection.as_deref())?;
    let record_id = engine.insert_record_from_f32_ns(&payload.vector, ns)?;

    let doc_node_id = if let Some(existing) = payload.attach_to_document_node {
        existing
    } else {
        engine.create_node_for_record(None, NodeKind::Document as u8, ns)?
    };

    let chunk_node_id = engine.create_node_for_record(Some(record_id), NodeKind::Chunk as u8, ns)?;
    engine.create_edge(doc_node_id, chunk_node_id, EdgeKind::ParentOf as u8)?;

    let memory_id = format!("rec:{}", record_id);
    if let Some(meta) = payload.metadata {
        engine.metadata.set(memory_id.clone(), meta);
        if let Err(e) = engine.flush_metadata() {
            tracing::warn!("memory_upsert: failed to persist metadata sidecar: {:?}", e);
        }
    }

    Ok(Json(MemoryUpsertResponse {
        memory_id,
        record_id,
        document_node_id: doc_node_id,
        chunk_node_id,
    }))
}

async fn memory_search_vector(
    State(state): State<SharedEngine>,
    Json(payload): Json<MemorySearchVectorRequest>,
) -> Result<Json<MemorySearchResponse>, EngineError> {
    let engine = state.read().await;
    let ns = engine.resolve_collection(payload.collection.as_deref())?;

    let half_life = payload.decay_half_life_secs.or(engine.decay_half_life_secs).unwrap_or(0);

    let results = if half_life == 0 {
        let hits = engine.search_l2_ns(&payload.query_vector, payload.k, ns)?;
        hits.into_iter()
            .map(|(record_id, score)| {
                let memory_id = format!("rec:{}", record_id);
                let metadata = engine.metadata.get(&memory_id);
                MemorySearchHit { memory_id, record_id, score, metadata,
                    decay_factor: None, age_secs: None }
            })
            .collect()
    } else {
        // Recency-aware recall: over-fetch, decay re-rank, trim to k.
        let pool = payload.k.saturating_mul(4).max(50).min(1000);
        let raw = engine.search_l2_ns(&payload.query_vector, pool, ns)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs()).unwrap_or(0);
        let candidates: Vec<crate::decay::DecayHit> = raw.into_iter()
            .map(|(id, score)| crate::decay::DecayHit {
                id, distance: score, created_at: engine.record_created_at(id),
            })
            .collect();
        crate::decay::rerank(candidates, now, half_life, payload.k)
            .into_iter()
            .map(|h| {
                let memory_id = format!("rec:{}", h.id);
                let metadata = engine.metadata.get(&memory_id);
                MemorySearchHit {
                    memory_id, record_id: h.id, score: h.distance, metadata,
                    decay_factor: Some(h.factor), age_secs: h.age_secs,
                }
            })
            .collect()
    };

    Ok(Json(MemorySearchResponse { results }))
}

async fn get_proof(
    State(state): State<SharedEngine>,
) -> impl IntoResponse {
    let engine = state.read().await;
    let proof = engine.get_proof();
    // Encode all 32 bytes as lowercase hex — same wire format as the cluster's
    // state_proof handler so external clients see an identical response shape.
    let hex: String = proof.final_state_hash.iter().map(|b| format!("{b:02x}")).collect();
    Json(serde_json::json!({ "final_state_hash": hex }))
}

async fn get_event_proof(
    State(state): State<SharedEngine>,
) -> Result<Json<EventProofResponse>, EngineError> {
    let engine = state.read().await;
    
    if let Some(ref committer) = engine.event_committer {
        let proof = engine.get_proof();
        let committed_height = committer.journal().committed_height();

        // Hash the actual event-log file with BLAKE3 (full 32 bytes → 64 hex chars).
        // Previously this was incorrectly set to the final_state_hash value, and both
        // hashes were truncated to 16 bytes then formatted without zero-padding,
        // yielding ≤32 hex chars instead of the correct 64.
        let event_log_path = committer.event_log().path().to_path_buf();
        let event_log_hash_bytes =
            crate::events::event_proof::compute_event_log_hash(&event_log_path)
                .unwrap_or([0u8; 32]);

        let response = EventProofResponse {
            kernel_version: 1,
            event_log_hash: event_log_hash_bytes.iter().map(|b| format!("{b:02x}")).collect(),
            final_state_hash: proof.final_state_hash.iter().map(|b| format!("{b:02x}")).collect(),
            snapshot_hash: None,
            event_count: committed_height,
            committed_height,
        };

        Ok(Json(response))
    } else {
        Err(EngineError::InvalidInput("Event log not enabled".to_string()))
    }
}

async fn get_wal_stream(
    State(state): State<SharedEngine>,
) -> Result<Body, EngineError> {
    let path = {
        let engine = state.read().await;
        engine.wal_path.clone()
    }.ok_or(EngineError::InvalidInput("No WAL configured".into()))?;

    let file = tokio::fs::File::open(&path).await
        .map_err(|e| EngineError::InvalidInput(e.to_string()))?;
    Ok(Body::from_stream(ReaderStream::new(file)))
}

#[derive(Deserialize)]
struct ReplicationParams {
    start_offset: Option<u64>,
}

async fn get_replication_events(
    State(state): State<SharedEngine>,
    Query(params): Query<ReplicationParams>,
) -> Result<Body, EngineError> {
    let start_offset = params.start_offset.unwrap_or(0);

    let (log_path, rx) = {
        let mut engine = state.write().await; // flush requires &mut
        if let Some(ref mut committer) = engine.event_committer {
            if let Err(e) = committer.flush_log() {
                tracing::error!("Failed to flush event log for replication: {}", e);
            }
            (committer.event_log().path().to_path_buf(), committer.subscribe())
        } else {
             return Err(EngineError::InvalidInput("Event log not enabled".to_string()));
        }
    };
    
    let rx_stream = crate::replication::spawn_replication_stream(log_path, rx, start_offset).await?;
    
    use futures::StreamExt;
    let body_stream = tokio_stream::wrappers::ReceiverStream::new(rx_stream).map(|res| {
        match res {
            Ok(json_line) => Ok(json_line),
            Err(e) => Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
        }
    });

    Ok(Body::from_stream(body_stream))
}

async fn get_replication_state() -> Json<serde_json::Value> {
    let status_str = crate::replication::replication_display_state();
    Json(serde_json::json!({ "status": status_str }))
}

/// `GET /metrics` — Prometheus text exposition format.
///
/// Refreshes all KernelState gauges synchronously before rendering so that
/// the scrape always reflects the live pool sizes regardless of write
/// activity between scrapes.
///
/// This endpoint is **always unauthenticated** so that Prometheus can scrape
/// without a bearer token.
async fn metrics_handler(
    State(state): State<SharedEngine>,
) -> String {
    // Update kernel gauges from live state before rendering.
    {
        let engine = state.read().await;
        engine.update_prometheus_metrics();
    }
    crate::telemetry::get_metrics()
}

#[derive(serde::Deserialize, Default)]
struct TimelineQuery {
    /// ISO 8601 UTC lower bound (inclusive).
    from: Option<String>,
    /// ISO 8601 UTC upper bound (inclusive).
    to: Option<String>,
    /// Filter to events in a specific collection (not yet applied at kernel level;
    /// kept for future use when namespace is stored per-event).
    #[allow(dead_code)]
    collection: Option<String>,
}

async fn get_timeline(
    State(state): State<SharedEngine>,
    Query(q): Query<TimelineQuery>,
) -> Result<Json<TimelineResponse>, EngineError> {
    use valori_kernel::event::KernelEvent;

    let engine = state.read().await;
    let Some(ref committer) = engine.event_committer else {
        return Err(EngineError::InvalidInput("Event log not enabled (set VALORI_EVENT_LOG_PATH)".to_string()));
    };

    let from_unix = q.from.as_deref().and_then(parse_iso8601);
    let to_unix   = q.to.as_deref().and_then(parse_iso8601);

    let journal = committer.journal();
    let mut entries: Vec<TimelineEntry> = Vec::new();

    for (log_index, (event, ts)) in journal.committed_with_timestamps().enumerate() {
        // Apply timestamp range filter.
        if let Some(from) = from_unix { if ts < from { continue; } }
        if let Some(to)   = to_unix   { if ts > to   { continue; } }

        let (event_type, record_id, node_id, edge_id) = match event {
            KernelEvent::InsertRecord { id, .. }          => ("InsertRecord",          Some(id.0), None,       None),
            KernelEvent::AutoInsertRecord { .. }          => ("AutoInsertRecord",       None,       None,       None),
            KernelEvent::InsertRecordEncrypted { id, .. } => ("InsertRecordEncrypted", Some(id.0), None,       None),
            KernelEvent::DeleteRecord { id }              => ("DeleteRecord",           Some(id.0), None,       None),
            KernelEvent::SoftDeleteRecord { id }          => ("SoftDeleteRecord",       Some(id.0), None,       None),
            KernelEvent::ShredKey { .. }                  => ("ShredKey",               None,       None,       None),
            KernelEvent::CreateNode { id, .. }            => ("CreateNode",             None,       Some(id.0), None),
            KernelEvent::AutoCreateNode { .. }            => ("AutoCreateNode",         None,       None,       None),
            KernelEvent::DeleteNode { id }                => ("DeleteNode",             None,       Some(id.0), None),
            KernelEvent::CreateEdge { id, .. }            => ("CreateEdge",             None,       None,       Some(id.0)),
            KernelEvent::AutoCreateEdge { .. }            => ("AutoCreateEdge",         None,       None,       None),
            KernelEvent::DeleteEdge { id }                => ("DeleteEdge",             None,       None,       Some(id.0)),
            KernelEvent::AutoInsertRecordEncrypted { .. } => ("AutoInsertRecordEncrypted", None,    None,       None),
        };

        entries.push(TimelineEntry {
            log_index: log_index as u64,
            timestamp_unix: ts,
            timestamp_iso: unix_to_iso8601(ts),
            event_type,
            record_id,
            node_id,
            edge_id,
        });
    }

    let total = entries.len();
    Ok(Json(TimelineResponse {
        events: entries,
        total,
        from_unix,
        to_unix,
    }))
}

// ── Collection (namespace) management endpoints ───────────────────────────────

async fn create_collection_handler(
    State(state): State<SharedEngine>,
    Json(payload): Json<CreateCollectionRequest>,
) -> Result<Json<CreateCollectionResponse>, EngineError> {
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return Err(EngineError::InvalidInput("collection name cannot be empty".into()));
    }
    let mut engine = state.write().await;
    let already_exists = engine.namespaces.map.contains_key(&name) || name == "default";
    let id = engine.create_collection(&name)?;
    Ok(Json(CreateCollectionResponse {
        name,
        id,
        created: !already_exists,
    }))
}

async fn list_collections_handler(
    State(state): State<SharedEngine>,
) -> Json<ListCollectionsResponse> {
    let engine = state.read().await;
    let collections = engine
        .list_collections()
        .into_iter()
        .map(|(name, id)| CollectionInfo { name, id })
        .collect();
    Json(ListCollectionsResponse { collections })
}

async fn drop_collection_handler(
    State(state): State<SharedEngine>,
    AxumPath(name): AxumPath<String>,
) -> Result<axum::http::StatusCode, EngineError> {
    let mut engine = state.write().await;
    engine.drop_collection(&name)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ── Phase 3.1: object-store handlers ─────────────────────────────────────────

#[derive(serde::Serialize)]
struct StorageSnapshotUploadResponse {
    key: String,
    state_hash: String,
    size_bytes: usize,
    pruned: usize,
}

#[derive(serde::Serialize)]
struct ListRemoteSnapshotsResponse {
    snapshots: Vec<crate::object_store::SnapshotEntry>,
    count: usize,
}

#[derive(serde::Deserialize)]
struct RestoreFromStoreRequest {
    /// Object key returned by a previous upload or list call.
    key: String,
}

#[derive(serde::Serialize)]
struct RestoreFromStoreResponse {
    key: String,
    state_hash: String,
    size_bytes: usize,
}

#[derive(serde::Serialize)]
struct ListRemoteWalResponse {
    segments: Vec<crate::object_store::WalEntry>,
    count: usize,
}

#[derive(serde::Deserialize)]
struct ArchiveWalRequest {
    /// Absolute path on this node's local filesystem to the sealed segment.
    path: String,
}

#[derive(serde::Serialize)]
struct ArchiveWalResponse {
    key: String,
    size_bytes: u64,
}

/// `GET /v1/storage/snapshots` — list snapshots in the object store.
async fn list_remote_snapshots(
    State(state): State<SharedEngine>,
) -> Result<Json<ListRemoteSnapshotsResponse>, EngineError> {
    let object_store = {
        let engine = state.read().await;
        engine.object_store.clone()
    };
    let os = object_store.ok_or_else(|| {
        EngineError::InvalidInput(
            "object store not configured — set VALORI_OBJECT_STORE_URL".into(),
        )
    })?;
    let snapshots = os.list_snapshots().await.map_err(|e| {
        EngineError::InvalidInput(format!("object store list failed: {e}"))
    })?;
    let count = snapshots.len();
    Ok(Json(ListRemoteSnapshotsResponse { snapshots, count }))
}

/// `POST /v1/storage/snapshots/upload` — snapshot current state and push to object store.
///
/// Automatically prunes old snapshots according to `VALORI_OBJECT_STORE_KEEP` (default 7).
async fn upload_snapshot_to_store(
    State(state): State<SharedEngine>,
) -> Result<Json<StorageSnapshotUploadResponse>, EngineError> {
    // Capture snapshot data and object store handle while holding the lock,
    // then release before any async I/O so we don't hold the mutex across awaits.
    let (snap_bytes, state_hash, object_store, keep) = {
        let engine = state.read().await;
        let snap = engine.snapshot()?;
        let proof = engine.get_proof();
        let hash = proof
            .final_state_hash
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let os = engine.object_store.clone();
        let keep = engine.object_store_keep as usize;
        (snap, hash, os, keep)
    };

    let os = object_store.ok_or_else(|| {
        EngineError::InvalidInput(
            "object store not configured — set VALORI_OBJECT_STORE_URL".into(),
        )
    })?;

    let size_bytes = snap_bytes.len();
    let key = os
        .upload_snapshot(&snap_bytes, &state_hash)
        .await
        .map_err(|e| EngineError::InvalidInput(format!("upload failed: {e}")))?;

    let pruned = os
        .prune_snapshots(keep)
        .await
        .unwrap_or(0);

    Ok(Json(StorageSnapshotUploadResponse {
        key,
        state_hash,
        size_bytes,
        pruned,
    }))
}

/// `POST /v1/storage/snapshots/restore` — pull a snapshot from the object store and restore.
///
/// Body: `{ "key": "snapshots/00000001750000000_abc12345.snap" }`
async fn restore_from_store(
    State(state): State<SharedEngine>,
    Json(req): Json<RestoreFromStoreRequest>,
) -> Result<Json<RestoreFromStoreResponse>, EngineError> {
    let object_store = {
        let engine = state.read().await;
        engine.object_store.clone()
    };
    let os = object_store.ok_or_else(|| {
        EngineError::InvalidInput(
            "object store not configured — set VALORI_OBJECT_STORE_URL".into(),
        )
    })?;

    let data = os
        .download_snapshot(&req.key)
        .await
        .map_err(|e| EngineError::InvalidInput(format!("download failed: {e}")))?;
    let size_bytes = data.len();

    {
        let mut engine = state.write().await;
        engine.restore(&data)?;
    }

    // Compute hash of the just-restored state.
    let state_hash = {
        let engine = state.read().await;
        engine
            .get_proof()
            .final_state_hash
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };

    tracing::info!(
        key = %req.key,
        state_hash = %state_hash,
        "restored from object store"
    );
    Ok(Json(RestoreFromStoreResponse {
        key: req.key,
        state_hash,
        size_bytes,
    }))
}

/// `GET /v1/storage/wal` — list archived WAL segments in the object store.
async fn list_remote_wal(
    State(state): State<SharedEngine>,
) -> Result<Json<ListRemoteWalResponse>, EngineError> {
    let object_store = {
        let engine = state.read().await;
        engine.object_store.clone()
    };
    let os = object_store.ok_or_else(|| {
        EngineError::InvalidInput(
            "object store not configured — set VALORI_OBJECT_STORE_URL".into(),
        )
    })?;
    let segments = os.list_wal_segments().await.map_err(|e| {
        EngineError::InvalidInput(format!("object store list failed: {e}"))
    })?;
    let count = segments.len();
    Ok(Json(ListRemoteWalResponse { segments, count }))
}

/// `POST /v1/storage/wal/archive` — archive a sealed WAL segment to the object store.
///
/// Body: `{ "path": "/data/events.log.000001" }`
///
/// The segment must already be sealed (rotated away from the live log path).
/// Auto-archival on rotation is wired in Phase 3.2.
async fn archive_wal_segment(
    State(state): State<SharedEngine>,
    Json(req): Json<ArchiveWalRequest>,
) -> Result<Json<ArchiveWalResponse>, EngineError> {
    let object_store = {
        let engine = state.read().await;
        engine.object_store.clone()
    };
    let os = object_store.ok_or_else(|| {
        EngineError::InvalidInput(
            "object store not configured — set VALORI_OBJECT_STORE_URL".into(),
        )
    })?;

    let local_path = std::path::Path::new(&req.path);
    if !local_path.exists() {
        return Err(EngineError::InvalidInput(format!(
            "segment not found: {}",
            req.path
        )));
    }
    let size_bytes = std::fs::metadata(local_path)
        .map(|m| m.len())
        .unwrap_or(0);
    let key = os
        .archive_wal_segment(local_path)
        .await
        .map_err(|e| EngineError::InvalidInput(format!("archive failed: {e}")))?;

    Ok(Json(ArchiveWalResponse { key, size_bytes }))
}

// ── Phase 3.5: API key management ────────────────────────────────────────────

#[derive(Deserialize)]
struct CreateKeyRequest {
    #[serde(default = "default_scope")]
    scope: ApiScope,
    collection: Option<String>,
    description: Option<String>,
}

fn default_scope() -> ApiScope { ApiScope::ReadWrite }

async fn create_key_handler(
    Extension(auth): Extension<Arc<AuthState>>,
    Json(req): Json<CreateKeyRequest>,
) -> impl IntoResponse {
    let created = auth.key_store.create(req.scope, req.collection, req.description);
    (StatusCode::CREATED, Json(created))
}

async fn list_keys_handler(
    Extension(auth): Extension<Arc<AuthState>>,
) -> impl IntoResponse {
    let keys = auth.key_store.list();
    Json(serde_json::json!({ "keys": keys }))
}

async fn revoke_key_handler(
    Extension(auth): Extension<Arc<AuthState>>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    if auth.key_store.revoke(&id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

// ── Phase 3.6: Crypto-shredding ───────────────────────────────────────────────

#[derive(Deserialize)]
struct InsertEncryptedRequest {
    /// Base64-encoded plaintext payload (will be encrypted by the vault).
    payload: String,
    tag: Option<u64>,
    collection: Option<String>,
    /// Optional pre-chosen key_id (hex). If absent, a fresh key_id is generated.
    key_id: Option<String>,
}

#[derive(Serialize)]
struct InsertEncryptedResponse {
    id: u32,
    key_id: String,
}

async fn insert_encrypted_handler(
    State(state): State<SharedEngine>,
    Json(payload): Json<InsertEncryptedRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    use base64::Engine as _;
    let plaintext = base64::engine::general_purpose::STANDARD
        .decode(&payload.payload)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("base64 decode: {e}")))?;

    let key_id: [u8; 16] = if let Some(ref hex) = payload.key_id {
        hex_to_key_id(hex)
            .ok_or_else(|| (StatusCode::BAD_REQUEST, "key_id must be 32 hex chars".into()))?
    } else {
        new_key_id()
    };

    let mut engine = state.write().await;
    let ns = engine.resolve_collection(payload.collection.as_deref())
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let tag = payload.tag.unwrap_or(0);

    let id = engine.insert_encrypted_ns(&plaintext, tag, ns, key_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok((StatusCode::CREATED, Json(InsertEncryptedResponse {
        id,
        key_id: key_id_to_hex(&key_id),
    })))
}

#[derive(Serialize)]
struct ShredKeyResponse {
    key_id: String,
    shredded: bool,
}

async fn shred_key_handler(
    State(state): State<SharedEngine>,
    AxumPath(key_id_hex): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let key_id = hex_to_key_id(&key_id_hex)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "key_id must be 32 hex chars".into()))?;

    let mut engine = state.write().await;
    engine.shred_key(key_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ShredKeyResponse { key_id: key_id_hex, shredded: true }))
}

#[derive(Serialize)]
struct CryptoStatusResponse {
    key_id: String,
    exists: bool,
}

async fn crypto_status_handler(
    State(state): State<SharedEngine>,
    AxumPath(key_id_hex): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let key_id = hex_to_key_id(&key_id_hex)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "key_id must be 32 hex chars".into()))?;

    let engine = state.read().await;
    let exists = engine.vault.key_exists(&key_id);
    Ok(Json(CryptoStatusResponse { key_id: key_id_hex, exists }))
}

// ── Phase 3.13: index config endpoint ────────────────────────────────────────

#[derive(Serialize)]
struct IndexConfigResponse {
    index_type: String,
    hnsw: Option<HnswConfigView>,
}

#[derive(Serialize)]
struct HnswConfigView {
    m: usize,
    m_max0: usize,
    ef_construction: usize,
    ef_search: usize,
}

async fn index_config_handler(
    State(state): State<SharedEngine>,
) -> impl IntoResponse {
    let engine = state.read().await;
    let index_type = match engine.index_kind {
        crate::config::IndexKind::BruteForce => "brute_force",
        crate::config::IndexKind::Hnsw       => "hnsw",
        crate::config::IndexKind::Ivf        => "ivf",
    };
    let hnsw = if engine.index_kind == crate::config::IndexKind::Hnsw {
        let c = &engine.hnsw_config;
        Some(HnswConfigView {
            m: c.m,
            m_max0: c.m_max0,
            ef_construction: c.ef_construction,
            ef_search: c.ef_search,
        })
    } else {
        None
    };
    Json(IndexConfigResponse { index_type: index_type.into(), hnsw })
}
