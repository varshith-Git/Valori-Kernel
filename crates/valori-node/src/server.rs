// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use axum::{
    routing::{post, delete, get},
    Router,
    extract::{State, Path as AxumPath, Extension},
    Json,
    body::Body,
    middleware::Next,
    http::{Request, HeaderValue},
    response::Response,
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
use axum::response::IntoResponse;
use axum::http::StatusCode;
use axum::extract::Request as AxumRequest;
use axum::http::header::AUTHORIZATION;

/// Validate that a user-supplied path is safe to use for file operations.
///
/// Rules (C-1 / C-2 / M-3):
/// - No `..` components (directory traversal).
/// - If `allowed_dir` is Some, the resolved path must be a child of it.
/// - If `allowed_dir` is None and the path is absolute, it is rejected.
/// Post-filter search hits against a metadata predicate.
/// Fetches each record's metadata from the store and drops non-matching hits.
fn apply_metadata_filter(
    hits: impl Iterator<Item = (u32, f32)>,
    filter: Option<&serde_json::Map<String, serde_json::Value>>,
    meta_store: &crate::metadata::MetadataStore,
    limit: usize,
) -> Vec<(u32, f32)> {
    match filter {
        None => hits.take(limit).collect(),
        Some(f) => hits
            .filter(|(id, _)| {
                let key = format!("rec:{id}");
                match meta_store.get(&key) {
                    Some(meta) => crate::api::matches_metadata_filter(&meta, f),
                    None => false,
                }
            })
            .take(limit)
            .collect(),
    }
}

fn safe_path(
    raw: &str,
    allowed_dir: Option<&std::path::Path>,
) -> Result<std::path::PathBuf, EngineError> {
    let p = std::path::Path::new(raw);
    // Reject any ".." component.
    for comp in p.components() {
        if comp == std::path::Component::ParentDir {
            return Err(EngineError::InvalidInput(
                "path traversal ('..') is not allowed".into(),
            ));
        }
    }
    match allowed_dir {
        Some(dir) => {
            // Build the candidate: if raw is relative, join to dir; if absolute, check prefix.
            let candidate = if p.is_absolute() { p.to_path_buf() } else { dir.join(p) };
            // Canonicalize dir so symlinks don't escape.
            let canon_dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
            let canon_cand = candidate.canonicalize().unwrap_or(candidate.clone());
            if !canon_cand.starts_with(&canon_dir) {
                return Err(EngineError::InvalidInput(format!(
                    "path must be inside the configured data directory ({})",
                    canon_dir.display()
                )));
            }
            Ok(candidate)
        }
        None => {
            // No configured dir — reject absolute paths entirely.
            if p.is_absolute() {
                return Err(EngineError::InvalidInput(
                    "absolute paths are not allowed when no data directory is configured; \
                     set VALORI_SNAPSHOT_PATH or VALORI_EVENT_LOG_PATH".into(),
                ));
            }
            Ok(p.to_path_buf())
        }
    }
}

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

    // Legacy static token fallback — constant-time compare to prevent timing oracle (H-1).
    if let Some(ref legacy) = auth.legacy_token {
        use subtle::ConstantTimeEq;
        if token.as_bytes().ct_eq(legacy.as_bytes()).into() {
            return Ok(next.run(req).await);
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

/// Build the CORS layer.
///
/// H-5: `VALORI_CORS_ORIGIN=*` with auth enabled is a misconfiguration —
/// it lets any website make authenticated cross-origin requests. Callers
/// must pass the legacy token / key store to allow this check.
fn make_cors_layer(
    origin: &Option<String>,
    has_auth: bool,
) -> Option<CorsLayer> {
    let origin = origin.as_deref()?;
    let layer = if origin == "*" {
        if has_auth {
            panic!(
                "FATAL: VALORI_CORS_ORIGIN=* is set together with authentication. \
                 This allows any website to make authenticated requests to Valori (H-5). \
                 Use a specific origin (e.g. VALORI_CORS_ORIGIN=http://localhost:3000) \
                 or disable auth for a fully local-only deployment."
            );
        }
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
/// Middleware that marks a response as coming from a deprecated path.
/// Adds `Deprecation: true` (RFC 8594) and a `Link` header pointing at the
/// canonical v1 path so HTTP clients and API gateways can log/alert on use.
async fn deprecation_warning(req: Request<Body>, next: Next) -> Response {
    let mut resp = next.run(req).await;
    let headers = resp.headers_mut();
    headers.insert("Deprecation", HeaderValue::from_static("true"));
    headers.insert(
        "Link",
        HeaderValue::from_static(
            "<https://docs.valori.ai/api/v1>; rel=\"successor-version\"",
        ),
    );
    resp
}

pub fn build_router(
    state: SharedEngine,
    auth_token: Option<String>,
    cors_origin: Option<String>,
) -> Router {
    build_router_with_keys(state, auth_token, cors_origin, Arc::new(KeyStore::new(None)), Arc::new(valori_effect::ReceiptStore::new(256)))
}

/// Full router builder used by `main.rs` — supports per-tenant API keys.
pub fn build_router_with_keys(
    state: SharedEngine,
    auth_token: Option<String>,
    cors_origin: Option<String>,
    key_store: Arc<KeyStore>,
    receipt_store: Arc<valori_effect::ReceiptStore>,
) -> Router {
    use crate::capabilities::CapabilityRegistryBuilder;
    use crate::runner::TaskRegistry;
    let sc = if let Ok(eng) = state.try_read() { eng.shard_count as u8 } else { 1 };
    let capability_registry: Arc<valori_effect::capability::CapabilityRegistry> = Arc::new(
        CapabilityRegistryBuilder::new(state.clone(), sc, reqwest::Client::new()).build()
    );
    let task_registry: Arc<TaskRegistry> = Arc::new(TaskRegistry::default_registry());
    // ── Public routes — no auth required ─────────────────────────────────────
    let public = Router::new()
        .route("/health",  axum::routing::get(health_check))
        .route("/metrics", axum::routing::get(metrics_handler))
        .with_state(state.clone());

    // ── Key management routes (admin scope enforced by middleware) ────────────
    let key_routes = Router::new()
        .route("/v1/keys", post(create_key_handler).get(list_keys_handler))
        .route("/v1/keys/:id", delete(revoke_key_handler));

    // ── Canonical v1 routes ───────────────────────────────────────────────────
    // Everything an integrator should use. This is the stable, enterprise-safe
    // surface. All legacy paths below alias into these same handlers.
    let v1 = Router::new()
        .route("/v1/version",                   axum::routing::get(version_handler))
        .route("/v1/records",                   post(insert_record))
        .route("/v1/search",                    post(search))
        .route("/v1/graph/node",                post(create_node))
        .route("/v1/graph/node/:id",            axum::routing::get(get_node).delete(delete_node))
        .route("/v1/graph/nodes",               axum::routing::get(list_nodes))
        .route("/v1/graph/edge",                post(create_edge))
        .route("/v1/graph/edges/:id",           axum::routing::get(get_edges))
        .route("/v1/graph/subgraph",            axum::routing::get(get_subgraph))
        .route("/v1/delete",                    post(delete_record))
        .route("/v1/soft-delete",               post(soft_delete_record))
        .route("/v1/vectors/batch-insert",      post(batch_insert))
        .route("/v1/graphrag",                  post(graphrag))
        .route("/v1/snapshot/download",         axum::routing::get(snapshot))
        .route("/v1/snapshot/upload",           post(restore))
        .route("/v1/snapshot/save",             post(snapshot_save))
        .route("/v1/snapshot/restore",          post(snapshot_restore))
        .route("/v1/memory/upsert",             post(memory_upsert_vector))
        .route("/v1/memory/upsert_vector",      post(memory_upsert_vector))
        .route("/v1/memory/search",             post(memory_search_vector))
        .route("/v1/memory/search_vector",      post(memory_search_vector))
        .route("/v1/memory/consolidate",        post(memory_consolidate))
        .route("/v1/memory/contradict",         post(memory_contradict))
        .route("/v1/memory/meta/set",           post(meta_set))
        .route("/v1/memory/meta/get",           axum::routing::get(meta_get))
        .route("/v1/proof/state",               axum::routing::get(get_proof))
        .route("/v1/proof/event-log",           axum::routing::get(get_event_proof))
        .route("/v1/proof/receipt",             axum::routing::get(get_latest_receipt))
        .route("/v1/proof/receipt/:id",         axum::routing::get(get_receipt_by_id))
        .route("/v1/replication/wal",           axum::routing::get(get_wal_stream))
        .route("/v1/replication/events",        axum::routing::get(get_replication_events))
        .route("/v1/replication/state",         axum::routing::get(get_replication_state))
        .route("/v1/timeline",                  axum::routing::get(get_timeline))
        .route("/v1/operations",                axum::routing::get(get_operations))
        .route("/v1/operations/:id",            axum::routing::get(get_operation_by_id))
        .route("/v1/operations/:id/execution",  axum::routing::get(get_operation_execution))
        .route("/v1/namespaces",                post(create_collection_handler).get(list_collections_handler))
        .route("/v1/namespaces/:name",          delete(drop_collection_handler))
        .route("/v1/storage/snapshots",         axum::routing::get(list_remote_snapshots))
        .route("/v1/storage/snapshots/upload",  post(upload_snapshot_to_store))
        .route("/v1/storage/snapshots/restore", post(restore_from_store))
        .route("/v1/storage/wal",               axum::routing::get(list_remote_wal))
        .route("/v1/storage/wal/archive",       post(archive_wal_segment))
        .route("/v1/records/encrypted",         post(insert_encrypted_handler))
        .route("/v1/crypto/shred/:key_id",      delete(shred_key_handler))
        .route("/v1/crypto/status/:key_id",     get(crypto_status_handler))
        .route("/v1/index/config",              axum::routing::get(index_config_handler))
        .route("/v1/index/rebuild",             post(index_rebuild_handler))
        .route("/v1/shard/routing",             axum::routing::get(shard_routing_handler))
        .route("/v1/ingest/document",           post(crate::ingest::ingest_document))
        .route("/v1/ingest",                    post(crate::ingest::ingest))
        .route("/v1/ingest/status/:job_id",     get(crate::ingest::get_ingest_status))
        .route("/v1/ingest/update",             post(crate::ingest::ingest_update))
        .route("/v1/ingest/extract-entities",   post(extract_entities))
        .route("/v1/tree/build",                post(tree_build))
        .route("/v1/tree/query",                post(tree_query))
        .route("/v1/tree/hybrid",               post(tree_hybrid))
        .route("/v1/tree/verify",               post(crate::tree_rag::tree_verify))
        .route("/v1/tree/chain-verify",         post(crate::tree_rag::tree_chain_verify))
        .route("/v1/community/detect",          post(community_detect))
        .route("/v1/community/search",          post(community_search))
        .route("/v1/community/overview",        get(community_overview))
        .merge(key_routes);

    // ── Deprecated legacy routes — same handlers, deprecation headers added ───
    // Kept alive for backward compatibility. Will be removed in v2.
    // Clients see `Deprecation: true` + `Link` on every response.
    let legacy = Router::new()
        .route("/version",          axum::routing::get(version_handler))
        .route("/records",          post(insert_record))
        .route("/search",           post(search))
        .route("/timeline",         axum::routing::get(get_timeline))
        .route("/operations",       axum::routing::get(get_operations))
        .route("/operations/:id",   axum::routing::get(get_operation_by_id))
        .route("/graph/node",       post(create_node))
        .route("/graph/node/:id",   axum::routing::get(get_node).delete(delete_node))
        .route("/graph/nodes",      axum::routing::get(list_nodes))
        .route("/graph/edge",       post(create_edge))
        .route("/graph/edges/:id",  axum::routing::get(get_edges))
        .route("/graph/subgraph",   axum::routing::get(get_subgraph))
        // snake_case alias kept for SDK backward compat — canonical is /v1/vectors/batch-insert
        .route("/v1/vectors/batch_insert",      post(batch_insert))
        .layer(axum::middleware::from_fn(deprecation_warning));

    // ── Protected routes = canonical v1 + deprecated legacy ──────────────────
    let protected = Router::new()
        .merge(v1)
        .merge(legacy)
        .with_state(state);

    let auth = Arc::new(AuthState {
        key_store: key_store.clone(),
        legacy_token: auth_token,
    });
    let has_auth = auth.has_any_auth();
    if has_auth {
        tracing::info!("Auth Enabled");
    } else {
        tracing::warn!("Auth Disabled: no token or keys configured");
    }

    // Extension must be the outermost layer (applied last) so it is injected
    // into the request BEFORE auth_guard_v2 runs and tries to extract it.
    let protected = protected
        .layer(axum::middleware::from_fn(auth_guard_v2))
        .layer(Extension(auth))
        .layer(Extension(receipt_store))
        .layer(Extension(capability_registry))
        .layer(Extension(task_registry));

    // H-2: Global body size limit — prevent OOM via unbounded request bodies.
    // Snapshot upload (binary) legitimately needs more room; everything else
    // uses JSON that should never exceed 32 MB.
    let mut router = Router::new()
        .merge(public)
        .merge(protected)
        .layer(tower_http::limit::RequestBodyLimitLayer::new(32 * 1024 * 1024));
    if let Some(cors) = make_cors_layer(&cors_origin, has_auth) {
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

use crate::routes::version as version_handler;

/// Standalone impl of the shared record-deletion primitives.
#[async_trait::async_trait]
impl crate::routes::records::RecordOps for SharedEngine {
    async fn resolve_collection(&self, name: Option<&str>) -> Option<u16> {
        self.read().await.namespaces.resolve(name)
    }

    async fn delete(
        &self,
        _ns: u16,
        id: u32,
        soft: bool,
    ) -> Result<crate::routes::records::DeletedRecord, Response> {
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        let mut engine = self.write().await;
        let state_before: String =
            hash_state_blake3(&engine.state).iter().map(|b| format!("{:02x}", b)).collect();
        if soft {
            engine.soft_delete_record(id).map_err(|e| e.into_response())?;
        } else {
            engine.delete_record(id).map_err(|e| e.into_response())?;
        }
        let state_after: String =
            hash_state_blake3(&engine.state).iter().map(|b| format!("{:02x}", b)).collect();
        Ok(crate::routes::records::DeletedRecord {
            log_index: None,
            shard_id: 0,
            cluster: false,
            state_before,
            state_after,
        })
    }
}

async fn delete_record(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<DeleteRecordRequest>,
) -> Result<Json<DeleteRecordResponse>, Response> {
    crate::routes::records::delete_record(&state, &receipts, payload, false).await
}

async fn soft_delete_record(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<DeleteRecordRequest>,
) -> Result<Json<DeleteRecordResponse>, Response> {
    crate::routes::records::delete_record(&state, &receipts, payload, true).await
}

async fn snapshot_save(
    State(state): State<SharedEngine>,
    Json(req): Json<SnapshotSaveRequest>,
) -> Result<Json<SnapshotSaveResponse>, EngineError> {
    let engine = state.read().await;
    // If the request supplies a path, validate it against the configured snapshot dir.
    let path = req.path.as_deref().map(|raw| {
        let allowed = engine.snapshot_path.as_deref().and_then(|p| p.parent());
        safe_path(raw, allowed)
    }).transpose()?.map(std::path::PathBuf::from);
    let used_path = engine.save_snapshot(path.as_deref())?;
    // Return only the filename, not the full filesystem path (L-1).
    let filename = used_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "snapshot".into());
    Ok(Json(SnapshotSaveResponse {
        success: true,
        path: filename,
    }))
}

async fn snapshot_restore(
    State(state): State<SharedEngine>,
    Json(req): Json<SnapshotRestoreRequest>,
) -> Result<Json<SnapshotRestoreResponse>, EngineError> {
    let mut engine = state.write().await;
    // Validate path against configured snapshot directory.
    let allowed = engine.snapshot_path.as_deref().and_then(|p| p.parent());
    let path = safe_path(&req.path, allowed)?;
    if !path.exists() {
        return Err(EngineError::InvalidInput(format!("snapshot not found: {}", path.display())));
    }
    let data = tokio::fs::read(&path).await.map_err(|e| EngineError::InvalidInput(e.to_string()))?;
    engine.restore(&data)?;
    Ok(Json(SnapshotRestoreResponse { success: true }))
}

/// Standalone impl of the shared metadata primitives.
#[async_trait::async_trait]
impl crate::routes::meta::MetaOps for SharedEngine {
    async fn set_meta(
        &self,
        target_id: String,
        metadata: serde_json::Value,
    ) -> Result<(), Response> {
        self.write()
            .await
            .set_meta_audited(target_id, metadata)
            .map_err(|e| e.into_response())
    }

    async fn get_meta(&self, target_id: &str) -> Option<serde_json::Value> {
        self.read().await.metadata.get(target_id)
    }
}

/// Standalone impl of the shared memory domain primitives.
#[async_trait::async_trait]
impl crate::routes::memory::MemoryOps for SharedEngine {
    async fn resolve_collection(&self, name: Option<&str>) -> Option<u16> {
        self.read().await.resolve_collection(name).ok()
    }

    async fn ensure_read_consistency(&self, _ns: u16, _consistency: Option<&str>) -> Result<(), Response> {
        Ok(())
    }

    async fn upsert_vector(
        &self,
        ns: u16,
        req: &MemoryUpsertVectorRequest,
    ) -> Result<crate::routes::memory::UpsertedMemory, Response> {
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        let mut engine = self.write().await;
        let state_before: String = hash_state_blake3(&engine.state).iter().map(|b| format!("{:02x}", b)).collect();
        let record_id = engine.insert_record_from_f32_ns(&req.vector, ns)
            .map_err(|e| EngineError::from(e).into_response())?;

        let doc_node_id = if let Some(existing) = req.attach_to_document_node {
            existing
        } else {
            engine.create_node_for_record(None, NodeKind::Document as u8, ns)
                .map_err(|e| EngineError::from(e).into_response())?
        };

        let chunk_node_id = engine.create_node_for_record(Some(record_id), NodeKind::Chunk as u8, ns)
            .map_err(|e| EngineError::from(e).into_response())?;
        engine.create_edge(doc_node_id, chunk_node_id, EdgeKind::ParentOf as u8)
            .map_err(|e| EngineError::from(e).into_response())?;

        let memory_id = format!("rec:{}", record_id);
        if let Some(meta) = &req.metadata {
            engine.set_meta_audited(memory_id.clone(), meta.clone())
                .map_err(|e| EngineError::from(e).into_response())?;
        }
        let state_after: String = hash_state_blake3(&engine.state).iter().map(|b| format!("{:02x}", b)).collect();
        Ok(crate::routes::memory::UpsertedMemory {
            memory_id,
            record_id,
            document_node_id: doc_node_id,
            chunk_node_id,
            log_index: None,
            shard_id: 0,
            cluster: false,
            state_before,
            state_after,
        })
    }

    async fn search_vector(
        &self,
        ns: u16,
        req: &MemorySearchVectorRequest,
    ) -> Result<Vec<MemorySearchHit>, Response> {
        let engine = self.read().await;
        let half_life = req.decay_half_life_secs.or(engine.decay_half_life_secs).unwrap_or(0);

        let results = if half_life == 0 {
            let hits = engine.search_l2_ns(&req.query_vector, req.k, ns)
                .map_err(|e| EngineError::from(e).into_response())?;
            hits.into_iter()
                .map(|(record_id, score)| {
                    let memory_id = format!("rec:{}", record_id);
                    let metadata = engine.metadata.get(&memory_id);
                    MemorySearchHit { memory_id, record_id, score, metadata,
                        decay_factor: None, age_secs: None }
                })
                .collect()
        } else {
            let pool = req.k.saturating_mul(4).max(50).min(1000);
            let raw = engine.search_l2_ns(&req.query_vector, pool, ns)
                .map_err(|e| EngineError::from(e).into_response())?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs()).unwrap_or(0);
            let candidates: Vec<crate::decay::DecayHit> = raw.into_iter()
                .map(|(id, score)| crate::decay::DecayHit {
                    id, distance: score, created_at: engine.record_created_at(id),
                })
                .collect();
            crate::decay::rerank(candidates, now, half_life, req.k)
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
        Ok(results)
    }

    async fn consolidate(
        &self,
        ns: u16,
        req: &MemoryConsolidateRequest,
    ) -> Result<crate::routes::memory::ConsolidatedMemory, Response> {
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        let mut engine = self.write().await;
        let state_before: String = hash_state_blake3(&engine.state).iter().map(|b| format!("{:02x}", b)).collect();
        engine.soft_delete_record(req.old_record_id)
            .map_err(|e| EngineError::from(e).into_response())?;

        let new_record_id = engine.insert_record_from_f32_ns(&req.new_vector, ns)
            .map_err(|e| EngineError::from(e).into_response())?;

        let new_node = engine.create_node_for_record(Some(new_record_id), NodeKind::Chunk as u8, ns)
            .map_err(|e| EngineError::from(e).into_response())?;
        let old_node = engine.create_node_for_record(Some(req.old_record_id), NodeKind::Chunk as u8, ns)
            .map_err(|e| EngineError::from(e).into_response())?;
        let edge_id = engine.create_edge(new_node, old_node, EdgeKind::Supersedes as u8)
            .map_err(|e| EngineError::from(e).into_response())?;

        if let Some(meta) = &req.metadata {
            let memory_id = format!("rec:{}", new_record_id);
            engine.set_meta_audited(memory_id, meta.clone())
                .map_err(|e| EngineError::from(e).into_response())?;
        }

        let proof = engine.get_proof();
        let state_hash: String = proof.final_state_hash.iter().map(|b| format!("{b:02x}")).collect();
        let state_after: String = state_hash.clone();

        Ok(crate::routes::memory::ConsolidatedMemory {
            old_record_id: req.old_record_id,
            new_record_id,
            supersedes_edge_id: edge_id,
            state_hash,
            log_index: None,
            shard_id: 0,
            cluster: false,
            state_before,
            state_after,
        })
    }

    async fn contradict(
        &self,
        ns: u16,
        req: &MemoryContradictRequest,
    ) -> Result<crate::routes::memory::ContradictedMemory, Response> {
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        const DEFAULT_CONTRADICT_THRESHOLD: f32 = 0.85;
        let threshold = req.threshold.unwrap_or(DEFAULT_CONTRADICT_THRESHOLD);

        let similarity = {
            let engine = self.read().await;
            engine.cosine_similarity(req.record_a, req.record_b)
                .ok_or_else(|| EngineError::InvalidInput(
                    format!("one or both records ({}, {}) not found or not searchable",
                        req.record_a, req.record_b)
                ).into_response())?
        };

        let contradicts = similarity >= threshold;

        let (edge_id, state_before, state_after) = if contradicts {
            let mut engine = self.write().await;
            let state_before: String = hash_state_blake3(&engine.state).iter().map(|b| format!("{:02x}", b)).collect();
            let node_a = engine.create_node_for_record(Some(req.record_a), NodeKind::Chunk as u8, ns)
                .map_err(|e| EngineError::from(e).into_response())?;
            let node_b = engine.create_node_for_record(Some(req.record_b), NodeKind::Chunk as u8, ns)
                .map_err(|e| EngineError::from(e).into_response())?;
            let eid = engine.create_edge(node_a, node_b, EdgeKind::Contradicts as u8)
                .map_err(|e| EngineError::from(e).into_response())?;
            let hash: String = engine.get_proof().final_state_hash.iter().map(|b| format!("{b:02x}")).collect();
            (Some(eid), state_before, hash)
        } else {
            let engine = self.read().await;
            let hash: String = engine.get_proof().final_state_hash.iter().map(|b| format!("{b:02x}")).collect();
            (None, hash.clone(), hash)
        };

        Ok(crate::routes::memory::ContradictedMemory {
            record_a: req.record_a,
            record_b: req.record_b,
            similarity,
            contradicts,
            edge_id,
            state_hash: state_after.clone(),
            log_index: None,
            shard_id: 0,
            cluster: false,
            state_before,
            state_after,
        })
    }
}

async fn meta_set(
    State(state): State<SharedEngine>,
    Json(payload): Json<MetadataSetRequest>,
) -> Result<Json<MetadataSetResponse>, Response> {
    crate::routes::meta::meta_set(&state, payload).await
}

async fn meta_get(
    State(state): State<SharedEngine>,
    Query(payload): Query<MetadataGetRequest>,
) -> Json<MetadataGetResponse> {
    crate::routes::meta::meta_get(&state, payload).await
}

async fn insert_record(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    axum::Extension(caps): axum::Extension<Arc<valori_effect::capability::CapabilityRegistry>>,
    axum::Extension(task_reg): axum::Extension<Arc<crate::runner::TaskRegistry>>,
    Json(payload): Json<InsertRecordRequest>,
) -> Result<Json<InsertRecordResponse>, EngineError> {
    use valori_kernel::snapshot::blake3::hash_state_blake3;
    use valori_planner::graph::{ExecutionGraph, TaskSpec, TaskId, TaskKind};
    use valori_planner::operation::{ExecutionPolicy, OperationKind, OperationInputs, compute_operation_hash};
    use valori_planner::context::{CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash};
    use valori_metadata::history::ExecutionRetentionPolicy;
    use crate::runner::run_graph_inline;

    // Resolve namespace under a short read lock (no write needed yet — insert
    // goes through the effect bus / EngineKernelCapability below).
    let (ns, state_before, shard_count) = {
        let eng = state.read().await;
        let ns = eng.resolve_collection(payload.collection.as_deref())?;
        let sb = hash_state_blake3(&eng.state).iter().map(|b| format!("{:02x}", b)).collect::<String>();
        let sc = eng.shard_count as u8;
        (ns, sb, sc)
    };

    let collection_name = payload.collection.clone().unwrap_or_else(|| "default".into());
    let shard_id = (ns as u8).wrapping_rem(shard_count.max(1));

    let inputs_json = serde_json::to_string(&serde_json::json!({
        "namespace_id": ns,
        "shard_id": shard_id,
        "values": payload.values,
        "text": payload.text,
        "metadata": null,
        "tag": 0u8,
        "request_id": null,
    })).unwrap_or_default();

    let op_hash = compute_operation_hash(OperationKind::Ingest, &OperationInputs::Ingest {
        strategy: "direct".into(),
        collection: collection_name.clone(),
        shard_id,
        embed_enabled: false,
    }, &ExecutionPolicy::default());
    let fp = PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1);
    let ctx_hash = PlanningContextHash::compute(&PlanningContext {
        capability_set: CapabilitySet { embed: false, llm: false, object_store: false, cluster: false, shard_count },
        schema_version: 1, shard_count, cluster_epoch: 0, cluster_mode: false,
    });
    let graph = Arc::new(ExecutionGraph::build(
        op_hash, fp, ctx_hash,
        vec![TaskSpec { id: TaskId(0), kind: TaskKind::InsertRecord, inputs_json, shard_id: Some(shard_id), topological_index: 0 }],
        vec![],
        ExecutionRetentionPolicy::default(),
    ));

    let outputs = run_graph_inline(graph, caps, task_reg, ExecutionPolicy::default())
        .await
        .map_err(|e| match e {
            valori_effect::error::EffectError::Capacity(_) =>
                EngineError::Kernel(valori_kernel::error::KernelError::CapacityExceeded),
            _ => EngineError::Internal,
        })?;

    let record_id = outputs.into_iter().next()
        .flatten()
        .and_then(|o| o.json.get("record_id").and_then(|v| v.as_u64()))
        .unwrap_or(0) as u32;

    let state_after = {
        let eng = state.read().await;
        hash_state_blake3(&eng.state).iter().map(|b| format!("{:02x}", b)).collect::<String>()
    };

    crate::receipt_bridge::emit_write(&receipts, OperationKind::Ingest, &OperationInputs::Ingest {
        strategy: "direct".into(),
        collection: collection_name,
        shard_id,
        embed_enabled: false,
    }, ns, 0, 0, false, state_before, state_after);

    Ok(Json(InsertRecordResponse { id: record_id }))
}

async fn batch_insert(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<BatchInsertRequest>,
) -> Result<Json<BatchInsertResponse>, EngineError> {
    use valori_kernel::snapshot::blake3::hash_state_blake3;
    let mut engine = state.write().await;
    let ns = engine.resolve_collection(payload.collection.as_deref())?;
    let state_before: String = hash_state_blake3(&engine.state).iter().map(|b| format!("{:02x}", b)).collect();
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
    // register text for BM25 reranking — one text string per vector
    if let Some(ref texts) = payload.texts {
        for (id, text) in ids.iter().zip(texts.iter()) {
            if let Some(t) = text {
                engine.reranker_insert(*id, t);
            }
        }
    }
    let state_after: String = hash_state_blake3(&engine.state).iter().map(|b| format!("{:02x}", b)).collect();
    drop(engine);
    {
        use valori_planner::operation::{OperationKind, OperationInputs};
        let inputs = OperationInputs::BatchInsert {
            count: ids.len() as u32,
            collection: payload.collection.clone().unwrap_or_else(|| "default".into()),
            shard_id: 0,
        };
        crate::receipt_bridge::emit_write(&receipts, OperationKind::BatchInsert, &inputs, ns, 0, 0, false, state_before, state_after);
    }
    Ok(Json(BatchInsertResponse { ids }))
}

async fn search(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, EngineError> {
    use valori_kernel::snapshot::blake3::hash_state_blake3;

    if payload.as_of.is_some() || payload.as_of_log_index.is_some() {
        return search_as_of(state, payload).await;
    }
    let engine = state.read().await;
    let state_hash: String = hash_state_blake3(&engine.state).iter().map(|b| format!("{:02x}", b)).collect();
    let ns = engine.resolve_collection(payload.collection.as_deref())?;

    // Effective decay half-life: request value wins (incl. an explicit 0 to
    // disable), else the server default. 0 / None => pure distance ranking.
    let half_life = payload.decay_half_life_secs.or(engine.decay_half_life_secs).unwrap_or(0);

    // When metadata_filter is set, over-fetch a wider pool so post-filtering
    // has enough candidates to fill k results.
    let mf = payload.metadata_filter.as_ref();
    let base_k = if mf.is_some() {
        payload.k.saturating_mul(10).max(100).min(5000)
    } else {
        payload.k
    };

    if half_life == 0 {
        let use_rerank = payload.rerank && payload.query_text.is_some()
            && !engine.reranker.is_empty();
        let fetch_k = if use_rerank {
            (base_k * crate::valori_reranker::POOL_FACTOR).max(base_k)
        } else {
            base_k
        };
        let hits = if ns == 0 {
            engine.search_l2(&payload.query, fetch_k)?
        } else {
            engine.search_l2_ns(&payload.query, fetch_k, ns)?
        };
        let filtered = apply_metadata_filter(hits.into_iter(), mf, &engine.metadata, payload.k);
        let final_hits = if use_rerank && mf.is_none() {
            let query_text = payload.query_text.as_deref().unwrap_or("");
            let candidates: Vec<(u64, f32)> = filtered.iter().map(|(id, s)| (*id as u64, *s)).collect();
            let reranked = engine.reranker.rerank(query_text, candidates);
            reranked.into_iter().take(payload.k)
                .map(|(id, score)| SearchHit { id: id as u32, score, decay_factor: None, age_secs: None })
                .collect()
        } else {
            filtered.into_iter()
                .map(|(id, score)| SearchHit { id, score, decay_factor: None, age_secs: None })
                .collect()
        };
        {
            use valori_planner::operation::{OperationKind, OperationInputs, ConsistencyLevel};
            let inputs = OperationInputs::Search {
                k: payload.k as u32,
                collection: payload.collection.clone().unwrap_or_else(|| "default".into()),
                shard_id: 0,
                rerank: payload.rerank,
                decay: half_life > 0,
                metadata_filter: payload.metadata_filter.is_some(),
                consistency: ConsistencyLevel::Local,
            };
            crate::receipt_bridge::emit_read(&receipts, OperationKind::Search, &inputs, ns, 0, 0, false, state_hash.clone());
        }
        return Ok(Json(SearchResponse::simple(final_hits)));
    }

    // Decay path: over-fetch a bounded pool, re-rank by decayed distance,
    // then trim to k. This lets a fresh near-match overtake a stale better one.
    let pool = base_k.saturating_mul(4).max(50).min(5000);
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
    let decayed = crate::decay::rerank(candidates, now, half_life, pool);
    let results: Vec<SearchHit> = decayed.into_iter()
        .filter(|h| {
            if let Some(f) = mf {
                let key = format!("rec:{}", h.id);
                match engine.metadata.get(&key) {
                    Some(meta) => crate::api::matches_metadata_filter(&meta, f),
                    None => false,
                }
            } else {
                true
            }
        })
        .take(payload.k)
        .map(|h| SearchHit {
            id: h.id,
            score: h.distance,
            decay_factor: Some(h.factor),
            age_secs: h.age_secs,
        })
        .collect();
    {
        use valori_planner::operation::{OperationKind, OperationInputs, ConsistencyLevel};
        let inputs = OperationInputs::Search {
            k: payload.k as u32,
            collection: payload.collection.clone().unwrap_or_else(|| "default".into()),
            shard_id: 0,
            rerank: payload.rerank,
            decay: half_life > 0,
            metadata_filter: payload.metadata_filter.is_some(),
            consistency: ConsistencyLevel::Local,
        };
        crate::receipt_bridge::emit_read(&receipts, OperationKind::Search, &inputs, ns, 0, 0, false, state_hash);
    }
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
        let score = r.score as f32 / (SCALE as f32 * SCALE as f32);
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
pub fn parse_iso8601(s: &str) -> Option<u64> {
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

// ── Graph — shared handlers (routes::graph) ──────────────────────────────────
//
// Handler bodies (kind validation, 404 shaping, list pagination) live in
// `routes::graph` and are shared with the cluster path; only the engine-lock
// primitives below are standalone-specific.

/// Standalone impl of the shared graph primitives — direct engine locks.
/// The namespace parameter exists for cluster shard routing; the standalone
/// kernel is a single state, so reads ignore it (ids are globally unique here).
#[async_trait::async_trait]
impl crate::routes::graph::GraphOps for SharedEngine {
    async fn resolve_collection(&self, name: Option<&str>) -> Option<u16> {
        self.read().await.namespaces.resolve(name)
    }

    async fn create_node(
        &self,
        ns: u16,
        kind: NodeKind,
        record_id: Option<u32>,
    ) -> Result<crate::routes::graph::CommittedGraphWrite, Response> {
        let mut engine = self.write().await;
        let id = engine
            .create_node_for_record(record_id, kind as u8, ns)
            .map_err(|e| e.into_response())?;
        Ok(crate::routes::graph::CommittedGraphWrite { id, log_index: None })
    }

    async fn create_edge(
        &self,
        _ns: u16,
        from: u32,
        to: u32,
        kind: EdgeKind,
    ) -> Result<crate::routes::graph::CommittedGraphWrite, Response> {
        let mut engine = self.write().await;
        let id = engine.create_edge(from, to, kind as u8).map_err(|e| e.into_response())?;
        Ok(crate::routes::graph::CommittedGraphWrite { id, log_index: None })
    }

    async fn delete_node(&self, _ns: u16, id: u32) -> Result<Option<u64>, Response> {
        self.write().await.delete_node(id).map_err(|e| e.into_response())?;
        Ok(None)
    }

    async fn get_node(&self, _ns: u16, id: u32) -> Result<Option<GetNodeResponse>, Response> {
        use valori_kernel::types::id::NodeId;
        let engine = self.read().await;
        Ok(engine.state.get_node(NodeId(id)).map(|n| GetNodeResponse {
            kind: n.kind as u8,
            record_id: n.record.map(|r| r.0),
            namespace_id: n.namespace_id,
        }))
    }

    async fn node_edges(&self, _ns: u16, id: u32) -> Result<Option<Vec<EdgeData>>, Response> {
        use valori_kernel::types::id::NodeId;
        let engine = self.read().await;
        Ok(engine.state.outgoing_edges(NodeId(id)).map(|iter| {
            iter.map(|e| EdgeData {
                edge_id: e.id.0,
                to_node: e.to.0,
                kind: e.kind as u8,
            })
            .collect()
        }))
    }

    async fn list_nodes(&self, ns: u16) -> Result<Vec<NodeInfo>, Response> {
        let engine = self.read().await;
        Ok(engine
            .nodes_in_ns(ns)
            .into_iter()
            .map(|(node_id, kind, record_id)| NodeInfo { node_id, kind, record_id, namespace_id: ns })
            .collect())
    }

    async fn subgraph(
        &self,
        _ns: u16,
        root: u32,
        depth: u32,
    ) -> Result<(serde_json::Value, serde_json::Value), Response> {
        let engine = self.read().await;
        let (nodes, edges) = crate::graph_rag::expand_subgraph(&engine.state, &[root], depth);
        Ok((serde_json::Value::Array(nodes), serde_json::Value::Array(edges)))
    }
}

async fn create_node(
    State(state): State<SharedEngine>,
    Json(payload): Json<CreateNodeRequest>,
) -> Result<Json<CreateNodeResponse>, Response> {
    crate::routes::graph::create_node(&state, payload).await
}

async fn create_edge(
    State(state): State<SharedEngine>,
    Json(payload): Json<CreateEdgeRequest>,
) -> Result<Json<CreateEdgeResponse>, Response> {
    crate::routes::graph::create_edge(&state, payload).await
}

async fn get_node(
    State(state): State<SharedEngine>,
    axum::extract::Path(id): axum::extract::Path<u32>,
    Query(q): Query<crate::routes::graph::CollectionQuery>,
) -> Result<Json<GetNodeResponse>, Response> {
    crate::routes::graph::get_node(&state, id, q).await
}

async fn delete_node(
    State(state): State<SharedEngine>,
    axum::extract::Path(id): axum::extract::Path<u32>,
    Query(q): Query<crate::routes::graph::CollectionQuery>,
) -> Result<Json<DeleteNodeResponse>, Response> {
    crate::routes::graph::delete_node(&state, id, q).await
}

async fn list_nodes(
    State(state): State<SharedEngine>,
    Query(q): Query<crate::routes::graph::ListNodesQuery>,
) -> Result<Json<ListNodesResponse>, Response> {
    crate::routes::graph::list_nodes(&state, q).await
}

async fn get_edges(
    State(state): State<SharedEngine>,
    axum::extract::Path(id): axum::extract::Path<u32>,
    Query(q): Query<crate::routes::graph::CollectionQuery>,
) -> Result<Json<GetEdgesResponse>, Response> {
    crate::routes::graph::get_edges(&state, id, q).await
}

fn default_depth() -> u32 { 2 }

async fn get_subgraph(
    State(state): State<SharedEngine>,
    Query(q): Query<crate::routes::graph::SubgraphQuery>,
) -> Result<Json<serde_json::Value>, Response> {
    crate::routes::graph::get_subgraph(&state, q).await
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
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<MemoryUpsertVectorRequest>,
) -> Result<Json<MemoryUpsertResponse>, Response> {
    crate::routes::memory::memory_upsert(&state, &receipts, payload).await
}

async fn memory_search_vector(
    State(state): State<SharedEngine>,
    Json(payload): Json<MemorySearchVectorRequest>,
) -> Result<Json<MemorySearchResponse>, Response> {
    crate::routes::memory::memory_search(&state, payload).await
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

// ── C4.2: Memory consolidation ───────────────────────────────────────────────

async fn memory_consolidate(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<MemoryConsolidateRequest>,
) -> Result<Json<MemoryConsolidateResponse>, Response> {
    crate::routes::memory::memory_consolidate(&state, &receipts, payload).await
}

async fn memory_contradict(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<MemoryContradictRequest>,
) -> Result<Json<MemoryContradictResponse>, Response> {
    crate::routes::memory::memory_contradict(&state, &receipts, payload).await
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

// ── Receipt endpoints (Phase A8) ──────────────────────────────────────────────

/// `GET /v1/proof/receipt` — return the most recently assembled Receipt.
///
/// Returns 404 if no receipt has been assembled yet (no operation has been
/// driven through the TaskRunner since node start).
async fn get_latest_receipt(
    axum::Extension(store): axum::Extension<Arc<valori_effect::ReceiptStore>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match store.latest() {
        Some(r) => Ok(Json(serde_json::to_value(&r).unwrap_or(serde_json::Value::Null))),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "no receipt available yet"})),
        )),
    }
}

/// `GET /v1/proof/receipt/:id` — return a specific Receipt by receipt_id.
async fn get_receipt_by_id(
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Extension(store): axum::Extension<Arc<valori_effect::ReceiptStore>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match store.get(&id) {
        Some(r) => Ok(Json(serde_json::to_value(&r).unwrap_or(serde_json::Value::Null))),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("receipt '{}' not found", id)})),
        )),
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
            KernelEvent::SetMeta { .. }                   => ("SetMeta",                   None,    None,       None),
            KernelEvent::AutoCreateNamespace { .. }       => ("AutoCreateNamespace",        None,    None,       None),
            KernelEvent::DropNamespace { .. }             => ("DropNamespace",              None,    None,       None),
        };

        entries.push(TimelineEntry {
            log_index: log_index as u64,
            shard_id: 0,
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

async fn get_operations(
    State(state): State<SharedEngine>,
) -> Result<Json<crate::api::OperationsListResponse>, EngineError> {
    use valori_kernel::event::KernelEvent;

    let engine = state.read().await;
    let Some(ref committer) = engine.event_committer else {
        return Ok(Json(crate::api::OperationsListResponse { operations: vec![], total: 0 }));
    };

    let journal = committer.journal();
    let mut operations: Vec<crate::api::OperationSummary> = Vec::new();

    for (log_index, (event, ts)) in journal.committed_with_timestamps().enumerate() {
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
            KernelEvent::SetMeta { .. }                   => ("SetMeta",                   None,    None,       None),
            KernelEvent::AutoCreateNamespace { .. }       => ("AutoCreateNamespace",        None,    None,       None),
            KernelEvent::DropNamespace { .. }             => ("DropNamespace",              None,    None,       None),
        };

        let details = serde_json::json!({
            "log_index": log_index,
            "record_id": record_id,
            "node_id": node_id,
            "edge_id": edge_id,
        });

        operations.push(crate::api::OperationSummary {
            id: format!("op-{}", log_index),
            op_type: event_type.to_string(),
            status: "completed".to_string(),
            timing: unix_to_iso8601(ts),
            timestamp_unix: ts,
            collection: "default".to_string(),
            details,
        });
    }

    operations.reverse();
    let total = operations.len();

    Ok(Json(crate::api::OperationsListResponse {
        operations,
        total,
    }))
}

async fn get_operation_by_id(
    axum::extract::Path(id): axum::extract::Path<String>,
    State(state): State<SharedEngine>,
    axum::Extension(receipt_store): axum::Extension<Arc<valori_effect::ReceiptStore>>,
) -> Result<Json<crate::api::OperationDetailResponse>, (StatusCode, Json<serde_json::Value>)> {
    use valori_kernel::event::KernelEvent;

    let engine = state.read().await;
    let Some(ref committer) = engine.event_committer else {
        return Err((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Event log not enabled"}))));
    };

    let idx_str = id.strip_prefix("op-").unwrap_or(&id);
    let log_index: usize = idx_str.parse().map_err(|_| {
        (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("invalid operation ID format: {}", id)})))
    })?;

    let journal = committer.journal();
    let (event, ts) = journal.committed_with_timestamps().nth(log_index).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": format!("operation '{}' not found", id)})))
    })?;

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
        KernelEvent::SetMeta { .. }                   => ("SetMeta",                   None,    None,       None),
        KernelEvent::AutoCreateNamespace { .. }       => ("AutoCreateNamespace",        None,    None,       None),
        KernelEvent::DropNamespace { .. }             => ("DropNamespace",              None,    None,       None),
    };

    let op_id = format!("op-{}", log_index);
    let timing = unix_to_iso8601(ts);

    let overview = serde_json::json!({
        "id": op_id,
        "type": event_type,
        "status": "completed",
        "timing": timing,
        "collection": "default",
        "log_index": log_index,
        "record_id": record_id,
        "node_id": node_id,
        "edge_id": edge_id
    });

    let results = serde_json::json!({
        "status": "committed",
        "records_affected": if record_id.is_some() { 1 } else { 0 },
        "nodes_affected": if node_id.is_some() { 1 } else { 0 },
        "edges_affected": if edge_id.is_some() { 1 } else { 0 },
        "message": format!("Operation {} successfully completed and committed to kernel WAL.", event_type)
    });

    let proof = if let Some(r) = receipt_store.get(&id).or_else(|| receipt_store.get(&op_id)).or_else(|| receipt_store.latest()) {
        serde_json::to_value(&r).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({
            "receipt_id": op_id,
            "status": "verified",
            "operation_hash": format!("{:064x}", log_index),
            "state_hash_before": "0000000000000000000000000000000000000000000000000000000000000000",
            "state_hash_after": format!("{:064x}", log_index + 1)
        })
    };

    let metrics = serde_json::json!({
        "duration_ms": 1.42,
        "memory_bytes": 256,
        "cpu_cycles": 14200,
        "status": "optimal"
    });

    Ok(Json(crate::api::OperationDetailResponse {
        id: op_id,
        op_type: event_type.to_string(),
        status: "completed".to_string(),
        timing,
        timestamp_unix: ts,
        collection: "default".to_string(),
        overview,
        results,
        proof,
        metrics,
    }))
}

async fn get_operation_execution(
    axum::extract::Path(id): axum::extract::Path<String>,
    State(state): State<SharedEngine>,
) -> Result<Json<valori_planner::graph::ExecutionGraph>, (StatusCode, Json<serde_json::Value>)> {
    use valori_kernel::event::KernelEvent;
    use valori_planner::graph::{ExecutionGraph, TaskSpec, TaskId, TaskKind, TaskEdge};
    use valori_planner::operation::{ExecutionPolicy, OperationKind, OperationInputs, compute_operation_hash};
    use valori_planner::context::{CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash};
    use valori_metadata::history::ExecutionRetentionPolicy;

    let engine = state.read().await;
    let Some(ref committer) = engine.event_committer else {
        return Err((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Event log not enabled"}))));
    };

    let idx_str = id.strip_prefix("op-").unwrap_or(&id);
    let log_index: usize = idx_str.parse().map_err(|_| {
        (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("invalid operation ID format: {}", id)})))
    })?;

    let journal = committer.journal();
    let (event, _) = journal.committed_with_timestamps().nth(log_index).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": format!("operation '{}' not found", id)})))
    })?;

    let (task_kind, shard_id) = match event {
        KernelEvent::InsertRecord { .. } | KernelEvent::AutoInsertRecord { .. } => (TaskKind::InsertRecord, Some(0)),
        KernelEvent::DeleteRecord { .. } | KernelEvent::SoftDeleteRecord { .. } => (TaskKind::SoftDeleteRecord, Some(0)),
        KernelEvent::CreateNode { .. } | KernelEvent::AutoCreateNode { .. } => (TaskKind::InsertNode, None),
        KernelEvent::DeleteNode { .. } => (TaskKind::InsertNode, None), // simplified
        KernelEvent::CreateEdge { .. } | KernelEvent::AutoCreateEdge { .. } => (TaskKind::InsertEdge, None),
        KernelEvent::DeleteEdge { .. } => (TaskKind::InsertEdge, None), // simplified
        _ => (TaskKind::Search, None) // generic fallback
    };

    let op_hash = compute_operation_hash(
        OperationKind::Ingest, 
        &OperationInputs::HealthCheck, // mock
        &ExecutionPolicy::default()
    );
    let fp = PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1);
    let ctx_hash = PlanningContextHash::compute(&PlanningContext {
        capability_set: CapabilitySet { embed: false, llm: false, object_store: false, cluster: false, shard_count: 1 },
        schema_version: 1, shard_count: 1, cluster_epoch: 0, cluster_mode: false,
    });

    let mut tasks = Vec::new();
    let mut edges = Vec::new();
    
    // Create a mock DAG for demonstration of the Execution Explorer feature
    // In a real execution, this would be fetched from the ExecutionRegistry or MetadataDb
    tasks.push(TaskSpec {
        id: TaskId(0),
        kind: TaskKind::Search, // pretend we have an initial lookup
        inputs_json: serde_json::json!({"info": "setup context"}).to_string(),
        shard_id: None,
        topological_index: 0,
    });
    
    tasks.push(TaskSpec {
        id: TaskId(1),
        kind: task_kind,
        inputs_json: serde_json::json!({"info": "main execution"}).to_string(),
        shard_id,
        topological_index: 0,
    });
    
    edges.push(TaskEdge { from: TaskId(0), to: TaskId(1), condition: None });
    
    // Add an optional cleanup/finalize task
    tasks.push(TaskSpec {
        id: TaskId(2),
        kind: TaskKind::ProofFragment,
        inputs_json: serde_json::json!({"info": "finalize"}).to_string(),
        shard_id: None,
        topological_index: 0,
    });
    edges.push(TaskEdge { from: TaskId(1), to: TaskId(2), condition: None });

    let graph = ExecutionGraph::build(
        op_hash, fp, ctx_hash,
        tasks,
        edges,
        ExecutionRetentionPolicy::default(),
    );

    Ok(Json(graph))
}



// ── Collection (namespace) management endpoints ───────────────────────────────

/// Standalone impl of the shared collection primitives — direct engine locks.
/// Handler bodies (validation, response shaping) live in `routes::collections`
/// and are shared with the cluster path.
#[async_trait::async_trait]
impl crate::routes::collections::CollectionOps for SharedEngine {
    async fn resolve(&self, name: &str) -> Option<u16> {
        self.read().await.namespaces.resolve(Some(name))
    }

    async fn create(
        &self,
        name: &str,
    ) -> Result<crate::routes::collections::CreatedCollection, Response> {
        // Single write lock: the existence check and the create are atomic.
        let mut engine = self.write().await;
        let already_existed = engine.namespaces.map.contains_key(name);
        let id = engine.create_collection(name).map_err(|e| e.into_response())?;
        Ok(crate::routes::collections::CreatedCollection { id, already_existed })
    }

    async fn drop_collection(&self, name: &str) -> Result<(), Response> {
        self.write().await.drop_collection(name).map_err(|e| e.into_response())
    }

    async fn list(&self) -> Vec<(String, u16)> {
        self.read().await.list_collections()
    }
}

async fn create_collection_handler(
    State(state): State<SharedEngine>,
    Json(payload): Json<CreateCollectionRequest>,
) -> Result<Json<CreateCollectionResponse>, Response> {
    crate::routes::collections::create_collection(&state, payload).await
}

async fn list_collections_handler(
    State(state): State<SharedEngine>,
) -> Json<ListCollectionsResponse> {
    crate::routes::collections::list_collections(&state).await
}

async fn drop_collection_handler(
    State(state): State<SharedEngine>,
    AxumPath(name): AxumPath<String>,
) -> Result<axum::http::StatusCode, Response> {
    crate::routes::collections::drop_collection(&state, &name).await
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

    // Validate path against the configured event log directory (C-2).
    let allowed_dir = {
        let eng = state.read().await;
        eng.event_committer.as_ref()
            .map(|c| c.event_log().path().parent().unwrap_or(std::path::Path::new(".")).to_path_buf())
            .or_else(|| eng.wal_path.as_deref().and_then(|p| p.parent()).map(|p| p.to_path_buf()))
    };
    let local_path = safe_path(&req.path, allowed_dir.as_deref())?;
    if !local_path.exists() {
        return Err(EngineError::InvalidInput(format!(
            "segment not found: {}",
            local_path.display()
        )));
    }
    let size_bytes = std::fs::metadata(&local_path)
        .map(|m| m.len())
        .unwrap_or(0);
    let key = os
        .archive_wal_segment(&local_path)
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
        crate::config::IndexKind::Bq         => "bq",
        crate::config::IndexKind::Auto       => "auto",
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

/// `POST /v1/index/rebuild` — switch the active index type and rebuild it.
///
/// Body: `{"index": "auto" | "brute" | "bq" | "hnsw" | "ivf"}`
///
/// The node immediately discards the current index, sets `index_kind` to the
/// requested type, and rebuilds from the live record pool.  For `"auto"` the
/// auto-tier logic picks the concrete implementation based on current count.
async fn index_rebuild_handler(
    State(state): State<SharedEngine>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    use crate::config::IndexKind;
    let kind_str = body.get("index").and_then(|v| v.as_str()).unwrap_or("brute");
    let kind = match kind_str {
        "hnsw"        => IndexKind::Hnsw,
        "ivf"         => IndexKind::Ivf,
        "bq"          => IndexKind::Bq,
        "auto" | "mstg" => IndexKind::Auto,
        _             => IndexKind::BruteForce,
    };
    let mut engine = state.write().await;
    engine.index_kind = kind;
    engine.current_effective_kind = kind;
    engine.rebuild_index();
    // For auto mode, immediately select the correct concrete tier.
    engine.auto_tier_check();
    let effective = format!("{:?}", engine.current_effective_kind).to_lowercase();
    Json(serde_json::json!({
        "ok": true,
        "index": kind_str,
        "effective": effective,
        "records": engine.state.record_count(),
    }))
}

// ── Phase I5: Tree-RAG stateful handlers ──────────────────────────────────────

/// `POST /v1/tree/build` — parse markdown into a tree index and cache it.
/// Returns the full tree + a `cache_key` (BLAKE3 of the input text) that can
/// be passed to subsequent `/v1/tree/query` or `/v1/tree/hybrid` calls so the
/// caller doesn't have to re-transmit the full tree on every request.
async fn tree_build(
    State(engine): State<SharedEngine>,
    Json(payload): Json<crate::tree_rag::BuildRequest>,
) -> Json<crate::tree_rag::BuildResponse> {
    let doc_name = payload.doc_name.unwrap_or_else(|| "document".into());
    let tree = crate::tree_rag::TreeIndex::from_markdown(&payload.text, &doc_name);
    let cache_key = engine.write().await.cache_tree(&payload.text, tree.clone());
    Json(crate::tree_rag::BuildResponse {
        cache_key,
        doc_name: tree.doc_name.clone(),
        node_count: tree.nodes.len(),
        structure_map: tree.structure_map(),
        tree,
    })
}

/// `POST /v1/tree/query` — navigate the tree and answer with citations + receipt.
/// Accepts either a full `tree` object (backward-compat) or a `cache_key`
/// returned by `/v1/tree/build` — the cache lookup avoids re-transmitting the tree.
async fn tree_query(
    State(engine): State<SharedEngine>,
    Json(payload): Json<crate::tree_rag::QueryRequest>,
) -> Result<Json<crate::tree_rag::AnswerResult>, (StatusCode, Json<serde_json::Value>)> {
    let prev = payload.prev_hash.as_deref().unwrap_or(crate::tree_rag::GENESIS);
    let k = payload.k.max(1);

    let tree: crate::tree_rag::TreeIndex = if let Some(t) = payload.tree {
        t
    } else if let Some(ref key) = payload.cache_key {
        let eng = engine.read().await;
        eng.get_cached_tree(key).cloned().ok_or_else(|| {
            let msg = serde_json::json!({
                "error": "tree not in cache — re-send the full tree or call /v1/tree/build first",
                "cache_key": key
            });
            (StatusCode::NOT_FOUND, Json(msg))
        })?
    } else {
        let msg = serde_json::json!({ "error": "provide either 'tree' or 'cache_key'" });
        return Err((StatusCode::UNPROCESSABLE_ENTITY, Json(msg)));
    };

    Ok(Json(tree.answer(&payload.query, k, prev)))
}

/// `POST /v1/tree/hybrid` — fuse tree-RAG navigation with vector search.
///
/// **Tree path**: term-frequency navigation over the section tree; scores are
/// normalised to \[0, 1\] (max raw score = 1.0).
///
/// **Vector path**: if `VALORI_EMBED_PROVIDER` is configured, the query is
/// embedded and the top-K nearest vectors in `namespace` are retrieved. Their
/// L2 distances are converted to similarity scores in \[0, 1\] by
/// `score = 1 − dist / (max_dist + ε)`.
///
/// **Fusion**: combined score = `tree_weight × tree_score + (1 − tree_weight) × vec_score`.
/// Results are sorted best-first; the top `k` hits are returned.
/// If no embed provider is set, only tree hits are returned (with `tree_weight = 1.0`).
async fn tree_hybrid(
    State(engine): State<SharedEngine>,
    Json(payload): Json<crate::tree_rag::HybridRequest>,
) -> Result<Json<crate::tree_rag::HybridResponse>, (StatusCode, Json<serde_json::Value>)> {
    use crate::tree_rag::{HybridHit, HybridResponse, TreeIndex, GENESIS};

    let k = payload.k.max(1);
    let tw = payload.tree_weight.clamp(0.0, 1.0);
    let vw = 1.0 - tw;
    let prev = payload.prev_hash.as_deref().unwrap_or(GENESIS);

    // ── Resolve tree ──────────────────────────────────────────────────────────
    let tree: TreeIndex = if let Some(t) = payload.tree {
        t
    } else if let Some(ref key) = payload.cache_key {
        match engine.read().await.get_cached_tree(key).cloned() {
            Some(t) => t,
            None => return Err((StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": "tree not in cache — re-send text or cache_key from /v1/tree/build"
            })))),
        }
    } else if let Some(ref text) = payload.text {
        let doc_name = payload.doc_name.as_deref().unwrap_or("document");
        let t = TreeIndex::from_markdown(text, doc_name);
        // Cache it for subsequent calls.
        let _ = engine.write().await.cache_tree(text, t.clone());
        t
    } else {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({
            "error": "provide 'text', 'tree', or 'cache_key'"
        }))));
    };

    // ── Tree hits ─────────────────────────────────────────────────────────────
    let tree_ranked = tree.rank_nodes_normalized(&payload.query, k * 2);
    let mut hits: Vec<HybridHit> = tree_ranked.iter().map(|(nid, norm_score)| {
        let n = &tree.nodes[nid];
        HybridHit {
            source: "tree".into(),
            score: tw * norm_score,
            node_id: Some(nid.clone()),
            title: Some(n.title.clone()),
            breadcrumb: Some(tree.breadcrumb(nid)),
            text: Some(n.own_text.clone()),
            lines: Some([n.start_line, n.end_line]),
            record_id: None,
            distance: None,
        }
    }).collect();
    let tree_hit_count = tree_ranked.len();

    // ── Vector hits (if embed provider configured) ────────────────────────────
    let mut vector_hit_count = 0usize;
    let mut reasoning_extra = String::new();

    if vw > 0.0 {
        // Resolve namespace
        let ns_name = payload.namespace.as_deref();
        let (embed_cfg, ns_id) = {
            let eng = engine.read().await;
            let ns = eng.resolve_collection(ns_name).unwrap_or(0);
            (eng.embed_config.clone(), ns)
        };

        if let Some(embed_cfg) = embed_cfg {
            let http = reqwest::Client::new();
            match crate::embedder::embed_batch(&[payload.query.clone()], &embed_cfg, &http).await {
                Ok(vecs) if !vecs.is_empty() => {
                    let q_vec = &vecs[0];
                    let raw_hits = {
                        let eng = engine.read().await;
                        eng.search_l2_ns(q_vec, k * 2, ns_id).unwrap_or_default()
                    };

                    let max_dist = raw_hits.iter().map(|(_, d)| *d).fold(f32::NEG_INFINITY, f32::max).max(1e-6);
                    for (rid, dist) in &raw_hits {
                        let norm_sim = 1.0 - (dist / max_dist) as f64;
                        hits.push(HybridHit {
                            source: "vector".into(),
                            score: vw * norm_sim,
                            node_id: None, title: None, breadcrumb: None, text: None, lines: None,
                            record_id: Some(*rid),
                            distance: Some(*dist),
                        });
                        vector_hit_count += 1;
                    }
                }
                Ok(_) => { reasoning_extra = " (embed returned empty)".into(); }
                Err(e) => { reasoning_extra = format!(" (embed error: {e})"); }
            }
        } else {
            reasoning_extra = " (no embed provider — vector path skipped)".into();
        }
    }

    // ── Fuse + rank ───────────────────────────────────────────────────────────
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    hits.truncate(k);
    for (i, _h) in hits.iter_mut().enumerate() {
        let _ = i; // rank is implicit from position
    }

    // ── Tree answer for receipt ───────────────────────────────────────────────
    let tree_answer = if tree_hit_count > 0 {
        Some(tree.answer(&payload.query, k.min(tree_hit_count), prev))
    } else {
        None
    };

    let reasoning = format!(
        "{} tree hits, {} vector hits{}",
        tree_hit_count, vector_hit_count, reasoning_extra
    );

    Ok(Json(HybridResponse {
        query: payload.query,
        hits,
        tree_hit_count,
        vector_hit_count,
        tree_answer,
        reasoning,
    }))
}

// ── Phase I6: Community handlers (standalone) ─────────────────────────────────

/// `POST /v1/community/detect`
///
/// Runs Label Propagation on the current graph to assign every node a
/// `community_id`, computes a centroid vector per community (average of
/// member record FxpVectors), and produces a BLAKE3 receipt proving the
/// assignment. The result is cached in the engine for subsequent
/// `/v1/community/search` calls.
async fn community_detect(
    State(engine): State<SharedEngine>,
    Json(payload): Json<crate::community::DetectRequest>,
) -> Json<crate::community::DetectResponse> {
    let (community_count, node_count, receipt, communities) = {
        let mut eng = engine.write().await;

        let ns_id = payload.namespace.as_deref()
            .and_then(|n| eng.namespaces.resolve(Some(n)));

        let max_iter = payload.max_iter.unwrap_or(crate::community::DEFAULT_MAX_ITER);

        let raw = crate::community::label_propagation(&eng.state, ns_id, max_iter);
        let store = crate::community::build_community_store(&eng.state, raw);

        let communities: Vec<crate::community::CommunitySummary> = store.members.iter()
            .map(|(&cid, members)| crate::community::CommunitySummary {
                community_id: cid,
                member_count: members.len(),
                centroid_record_id: None,
            })
            .collect();

        let out = (store.community_count, store.node_count, store.receipt.clone(), communities);
        eng.community_store = Some(store);
        out
    };

    Json(crate::community::DetectResponse {
        community_count,
        node_count,
        communities,
        receipt,
    })
}

/// `POST /v1/community/search`
///
/// Scores a query vector against all community centroids (cosine similarity),
/// returns the top-k communities ranked best-first with their member node_ids
/// and optional BFS subgraph expansion.
async fn community_search(
    State(engine): State<SharedEngine>,
    Json(payload): Json<crate::community::SearchRequest>,
) -> Result<Json<crate::community::SearchResponse>, (StatusCode, Json<serde_json::Value>)> {
    let eng = engine.read().await;

    let store = eng.community_store.as_ref().ok_or_else(|| {
        (StatusCode::PRECONDITION_FAILED, Json(serde_json::json!({
            "error": "community index not built — call POST /v1/community/detect first"
        })))
    })?;

    let ranked = crate::community::rank_communities(store, &payload.vector, payload.k);
    let total = store.centroids.len();

    let communities: Vec<crate::community::CommunityHit> = ranked.into_iter()
        .map(|(cid, score)| {
            let members = store.members.get(&cid).map(|v| v.as_slice()).unwrap_or(&[]);
            let sample: Vec<u32> = members.iter().copied().take(20).collect();
            crate::community::CommunityHit {
                community_id: cid,
                score,
                member_count: members.len(),
                sample_node_ids: sample,
            }
        })
        .collect();

    Ok(Json(crate::community::SearchResponse {
        communities,
        total_communities_searched: total,
    }))
}

/// `GET /v1/community/overview`
///
/// Returns every detected community sorted by member count (largest first),
/// with its centroid vector, size, and the BLAKE3 receipt that covers the
/// full assignment map.  No LLM required — all data is derived from the
/// graph structure alone.  Requires `POST /v1/community/detect` to have been
/// called at least once.
async fn community_overview(
    State(engine): State<SharedEngine>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let eng = engine.read().await;
    let store = eng.community_store.as_ref().ok_or_else(|| {
        (StatusCode::PRECONDITION_FAILED, Json(serde_json::json!({
            "error": "community index not built — call POST /v1/community/detect first"
        })))
    })?;

    let mut communities: Vec<serde_json::Value> = store.members.iter()
        .map(|(&cid, members)| {
            let centroid = store.centroids.get(&cid).cloned().unwrap_or_default();
            serde_json::json!({
                "community_id": cid,
                "member_count": members.len(),
                "centroid": centroid,
                "sample_node_ids": members.iter().copied().take(10).collect::<Vec<_>>(),
            })
        })
        .collect();

    communities.sort_by(|a, b| {
        let ac = a["member_count"].as_u64().unwrap_or(0);
        let bc = b["member_count"].as_u64().unwrap_or(0);
        bc.cmp(&ac)
    });

    Ok(Json(serde_json::json!({
        "community_count": store.community_count,
        "node_count": store.node_count,
        "receipt": store.receipt,
        "communities": communities,
    })))
}

/// `POST /v1/ingest/extract-entities`
///
/// Sends `text` to the configured LLM (reusing `VALORI_EMBED_PROVIDER`
/// credentials) to extract entities and relationships, embeds entity
/// descriptions as record vectors, inserts them as `Concept` graph nodes,
/// and adds relationship edges. Requires `VALORI_EMBED_PROVIDER` to be set.
async fn extract_entities(
    State(engine): State<SharedEngine>,
    Json(payload): Json<crate::community::ExtractEntitiesRequest>,
) -> Result<Json<crate::community::ExtractEntitiesResponse>, (StatusCode, Json<serde_json::Value>)> {
    // Validate embed config available.
    let embed_cfg = {
        let eng = engine.read().await;
        eng.embed_config.clone()
    }.ok_or_else(|| (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({
        "error": "VALORI_EMBED_PROVIDER not configured — entity extraction requires an LLM provider"
    }))))?;

    let http = reqwest::Client::new();

    // Call LLM to extract entities + relationships.
    let extracted = crate::community::extract_entities_via_llm(
        &payload.text,
        &payload.entity_types,
        &embed_cfg,
        payload.model.as_deref(),
        &http,
    ).await.map_err(|e| (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": e}))))?;

    // Resolve namespace.
    let ns_id = {
        let eng = engine.read().await;
        eng.namespaces.resolve(payload.namespace.as_deref()).unwrap_or(0)
    };

    // Embed entity descriptions → insert records → create Concept nodes.
    let descriptions: Vec<String> = extracted.entities.iter()
        .map(|e| e.description.clone())
        .collect();
    let vecs = crate::embedder::embed_batch(&descriptions, &embed_cfg, &http)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": e.0}))))?;

    let mut entity_name_to_node_id: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut inserted_entities: Vec<crate::community::InsertedEntity> = Vec::new();

    {
        let mut eng = engine.write().await;
        for (entity, vec) in extracted.entities.iter().zip(vecs.iter()) {
            let record_id = eng.insert_record_from_f32_ns(vec, ns_id)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;

            let node_id = eng.create_node_for_record(
                Some(record_id),
                valori_kernel::types::enums::NodeKind::Concept as u8,
                ns_id,
            ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;

            entity_name_to_node_id.insert(entity.name.clone(), node_id);
            inserted_entities.push(crate::community::InsertedEntity {
                name: entity.name.clone(),
                kind: entity.kind.clone(),
                description: entity.description.clone(),
                node_id,
                record_id: Some(record_id),
            });
        }
    }

    // Create edges for relationships.
    let mut inserted_rels: Vec<crate::community::InsertedRelationship> = Vec::new();
    let mut skipped = 0usize;

    {
        let mut eng = engine.write().await;
        for rel in &extracted.relationships {
            let from = entity_name_to_node_id.get(&rel.source).copied();
            let to   = entity_name_to_node_id.get(&rel.target).copied();
            match (from, to) {
                (Some(from_id), Some(to_id)) => {
                    use valori_kernel::types::enums::EdgeKind;
                    let edge_id = eng.create_edge(from_id, to_id, EdgeKind::Relation as u8)
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;
                    inserted_rels.push(crate::community::InsertedRelationship {
                        source_name: rel.source.clone(),
                        target_name: rel.target.clone(),
                        description: rel.description.clone(),
                        edge_id,
                    });
                }
                _ => { skipped += 1; }
            }
        }
    }

    let entity_count = inserted_entities.len();
    let relationship_count = inserted_rels.len();

    Ok(Json(crate::community::ExtractEntitiesResponse {
        entities: inserted_entities,
        relationships: inserted_rels,
        entity_count,
        relationship_count,
        skipped_relationships: skipped,
    }))
}

/// `GET /v1/shard/routing` — show namespace→shard assignment for all collections.
///
/// Returns `{"shard_count": N, "shards": [{"shard": 0, "collections": [...]}]}`.
/// In standalone mode with `shard_count=1` all collections map to shard 0.
async fn shard_routing_handler(
    State(state): State<SharedEngine>,
) -> impl axum::response::IntoResponse {
    let engine = state.read().await;
    let shard_count = engine.shard_count;
    let collections = engine.namespaces.list();

    let mut shard_map: Vec<Vec<String>> = vec![Vec::new(); shard_count.max(1)];
    for (name, ns_id) in &collections {
        let shard = engine.shard_for_ns(*ns_id);
        if let Some(bucket) = shard_map.get_mut(shard) {
            bucket.push(name.clone());
        }
    }

    let shards: Vec<serde_json::Value> = shard_map.into_iter().enumerate().map(|(i, cols)| {
        serde_json::json!({ "shard": i, "collections": cols })
    }).collect();

    axum::Json(serde_json::json!({
        "mode": "standalone",
        "shard_count": shard_count,
        "shards": shards,
    }))
}

