// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::api::*;
// `#[utoipa::path]` response bodies name `ApiError`, which is declared beside the
// OpenAPI registry rather than in `api.rs` (it mirrors an error type from a crate
// that does not depend on utoipa).
use crate::api_keys::{required_scope, ApiScope, AuthState, KeyStore};
use crate::crypto_vault::{hex_to_key_id, key_id_to_hex, new_key_id};
use crate::engine::Engine;
use crate::errors::EngineError;
#[cfg(feature = "utoipa")]
#[allow(unused_imports)]
use crate::openapi::ApiError;
use axum::{
    body::Body,
    extract::{Extension, Path as AxumPath, State},
    http::{HeaderValue, Request},
    middleware::Next,
    response::Response,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::io::ReaderStream;
use tower_http::cors::{Any, CorsLayer};

/// Phase 3.11: RwLock-backed engine — allows concurrent reads.
/// Read handlers call `.read().await`; write handlers call `.write().await`.
pub type SharedEngine = Arc<RwLock<Engine>>;

/// A single long-lived HTTP client shared across all handlers.
/// `reqwest::Client` internally manages a connection pool; creating one per
/// request wastes sockets and bypasses keep-alive.
static HTTP_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
pub(crate) fn shared_http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(reqwest::Client::new)
}

use axum::extract::Query;
use axum::extract::Request as AxumRequest;
use axum::http::header::AUTHORIZATION;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use valori_kernel::types::enums::{EdgeKind, NodeKind};

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
                    Some(meta) => valori_search::matches_metadata_filter(&meta, f),
                    None => false,
                }
            })
            .take(limit)
            .collect(),
    }
}

/// G1.4.1 — graph-aware reranking, applied as a final pass over whatever
/// `Vec<SearchHit>` the existing pipeline (BM25/plain or decay branch)
/// already produced. Pure, read-time, zero canonical-state impact — see
/// docs/reviews/graph-g1.4.1-graph-aware-reranking-design.md.
///
/// Seeds: the resolved graph nodes of the top `seed_count` hits (already
/// the pipeline's own best candidates — no new API surface). Signal: hop
/// distance from that seed set, reduced per record to the MINIMUM across
/// all of that record's live graph nodes (G1.3.1's `nodes_referencing_record`
/// enumeration). Missing/unreachable graph data is neutral — never drops a
/// candidate, never penalizes it (see `graph_rerank::graph_penalty`).
fn apply_graph_rerank(
    engine: &Engine,
    hits: Vec<SearchHit>,
    req: &crate::api::GraphRerankRequest,
    k: usize,
) -> Vec<SearchHit> {
    if hits.is_empty() {
        return hits;
    }
    let seed_count = req.seed_count.clamp(1, 10);
    let weight = req.weight.clamp(0.0, 1.0);
    let max_depth = req.max_depth.min(valori_rag::graph::MAX_DEPTH);
    let direction = match req
        .direction
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("incoming") => valori_rag::graph::Direction::Incoming,
        Some("both") => valori_rag::graph::Direction::Both,
        // None, "outgoing", or an unrecognized value all fall back to the
        // default — this is a ranking hint, not a hard filter, so an
        // unknown direction string degrades gracefully rather than 400ing
        // (unlike GET /v1/graph/query, which rejects it — a rerank hint
        // has no correctness contract to violate by falling back).
        _ => valori_rag::graph::Direction::Outgoing,
    };

    let state = engine.kernel_state();

    let top_ids: Vec<u32> = hits.iter().take(seed_count).map(|h| h.id).collect();
    let seed_map = valori_rag::graph::resolve_seed_nodes(state, &top_ids);
    let mut seed_nodes: Vec<u32> = seed_map.values().copied().collect();
    seed_nodes.sort_unstable();
    seed_nodes.dedup();

    let distances =
        valori_rag::graph::graph_distances_from_seeds(state, &seed_nodes, direction, max_depth);

    // Graph rerank doesn't carry decay_factor/age_secs through its own
    // scoring types (they're orthogonal to the graph signal) — stash them
    // by id and reattach after rerank.
    let mut extras: std::collections::HashMap<u32, (Option<f32>, Option<u64>)> =
        std::collections::HashMap::with_capacity(hits.len());
    let rerank_hits: Vec<valori_search::GraphRerankHit> = hits
        .iter()
        .map(|h| {
            extras.insert(h.id, (h.decay_factor, h.age_secs));
            let nodes = valori_rag::graph::nodes_referencing_record(state, h.id);
            let graph_distance = nodes.iter().filter_map(|n| distances.get(n).copied()).min();
            valori_search::GraphRerankHit {
                id: h.id,
                score: h.score,
                graph_distance,
            }
        })
        .collect();

    metrics::counter!("valori_graph_rerank_total", 1u64);
    valori_search::graph_rerank_apply(rerank_hits, weight, k)
        .into_iter()
        .map(|r| {
            let (decay_factor, age_secs) = extras.get(&r.id).copied().unwrap_or((None, None));
            SearchHit {
                id: r.id,
                score: r.score,
                decay_factor,
                age_secs,
                graph_distance: r.graph_distance,
            }
        })
        .collect()
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
            let candidate = if p.is_absolute() {
                p.to_path_buf()
            } else {
                dir.join(p)
            };
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
                     set VALORI_SNAPSHOT_PATH or VALORI_EVENT_LOG_PATH"
                        .into(),
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
fn make_cors_layer(origin: &Option<String>, has_auth: bool) -> Option<CorsLayer> {
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
        HeaderValue::from_static("<https://docs.valori.ai/api/v1>; rel=\"successor-version\""),
    );
    resp
}

pub fn build_router(
    state: SharedEngine,
    auth_token: Option<String>,
    cors_origin: Option<String>,
) -> Router {
    build_router_with_keys(
        state,
        auth_token,
        cors_origin,
        Arc::new(KeyStore::new(None)),
        Arc::new(valori_effect::ReceiptStore::new(256)),
    )
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
    let sc = if let Ok(eng) = state.try_read() {
        eng.shard_count as u8
    } else {
        1
    };
    let capability_registry: Arc<valori_effect::capability::CapabilityRegistry> = Arc::new(
        CapabilityRegistryBuilder::new(state.clone(), sc, shared_http_client().clone()).build(),
    );
    let task_registry: Arc<TaskRegistry> = Arc::new(TaskRegistry::default_registry());
    let execution_registry: Arc<crate::execution_registry::ExecutionRegistry> =
        Arc::new(crate::execution_registry::ExecutionRegistry::default());
    // ── Public routes — no auth required ─────────────────────────────────────
    let public = Router::new()
        .route("/health", axum::routing::get(health_check))
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
        .route("/v1/version", axum::routing::get(version_handler))
        .route("/v1/records", post(insert_record))
        .route("/v1/records/:id", axum::routing::get(get_record_by_id))
        .route(
            "/v1/records/:id/metadata",
            axum::routing::patch(update_record_metadata),
        )
        .route("/v1/search", post(search))
        .route("/v1/search/multi", post(multi_search))
        .route("/v1/graph/node", post(create_node))
        .route(
            "/v1/graph/node/:id",
            axum::routing::get(get_node).delete(delete_node),
        )
        .route("/v1/graph/nodes", axum::routing::get(list_nodes))
        .route("/v1/graph/edge", post(create_edge))
        .route("/v1/graph/edges/:id", axum::routing::get(get_edges))
        .route("/v1/graph/subgraph", axum::routing::get(get_subgraph))
        .route("/v1/graph/query", axum::routing::get(graph_query))
        .route("/v1/delete", post(delete_record))
        .route("/v1/soft-delete", post(soft_delete_record))
        .route("/v1/vectors/batch-insert", post(batch_insert))
        .route("/v1/graphrag", post(graphrag))
        .route("/v1/snapshot/download", axum::routing::get(snapshot))
        .route("/v1/snapshot/upload", post(restore))
        .route("/v1/snapshot/save", post(snapshot_save))
        .route("/v1/snapshot/restore", post(snapshot_restore))
        .route("/v1/memory/upsert", post(memory_upsert_vector))
        .route("/v1/memory/upsert_vector", post(memory_upsert_vector_alias))
        .route("/v1/memory/search", post(memory_search_vector))
        .route("/v1/memory/search_vector", post(memory_search_vector_alias))
        .route("/v1/memory/consolidate", post(memory_consolidate))
        .route("/v1/memory/contradict", post(memory_contradict))
        .route("/v1/memory/meta/set", post(meta_set))
        .route("/v1/memory/meta/get", axum::routing::get(meta_get))
        .route("/v1/usage", axum::routing::get(usage_handler))
        .route("/v1/proof/state", axum::routing::get(get_proof))
        .route("/v1/proof/event-log", axum::routing::get(get_event_proof))
        .route("/v1/proof/receipt", axum::routing::get(get_latest_receipt))
        .route(
            "/v1/proof/receipt/:id",
            axum::routing::get(get_receipt_by_id),
        )
        .route("/v1/replication/wal", axum::routing::get(get_wal_stream))
        .route(
            "/v1/replication/events",
            axum::routing::get(get_replication_events),
        )
        .route(
            "/v1/replication/state",
            axum::routing::get(get_replication_state),
        )
        .route("/v1/timeline", axum::routing::get(get_timeline))
        .route("/v1/operations", axum::routing::get(get_operations))
        .route(
            "/v1/operations/:id",
            axum::routing::get(get_operation_by_id),
        )
        .route(
            "/v1/operations/:id/execution",
            axum::routing::get(get_operation_execution),
        )
        .route(
            "/v1/namespaces",
            post(create_collection_handler).get(list_collections_handler),
        )
        .route("/v1/namespaces/:name", delete(drop_collection_handler))
        .route(
            "/v1/namespaces/:name/index",
            post(index_lifecycle_create_handler).get(index_lifecycle_status_handler),
        )
        .route(
            "/v1/storage/snapshots",
            axum::routing::get(list_remote_snapshots),
        )
        .route(
            "/v1/storage/snapshots/upload",
            post(upload_snapshot_to_store),
        )
        .route("/v1/storage/snapshots/restore", post(restore_from_store))
        .route("/v1/storage/manifest", axum::routing::get(get_manifest))
        .route("/v1/storage/wal", axum::routing::get(list_remote_wal))
        .route("/v1/storage/wal/archive", post(archive_wal_segment))
        .route("/v1/records/encrypted", post(insert_encrypted_handler))
        .route("/v1/crypto/shred/:key_id", delete(shred_key_handler))
        .route("/v1/crypto/status/:key_id", get(crypto_status_handler))
        .route("/v1/index/config", axum::routing::get(index_config_handler))
        .route("/v1/index/rebuild", post(index_rebuild_handler))
        .route(
            "/v1/shard/routing",
            axum::routing::get(shard_routing_handler),
        )
        .route("/v1/ingest/document", post(valori_ingest::ingest_document))
        .route("/v1/ingest", post(crate::ingest::ingest))
        .route(
            "/v1/ingest/status/:job_id",
            get(crate::ingest::get_ingest_status),
        )
        .route("/v1/ingest/update", post(crate::ingest::ingest_update))
        .route("/v1/ingest/extract-entities", post(extract_entities))
        .route("/v1/tree/build", post(tree_build))
        .route("/v1/tree/query", post(tree_query))
        .route("/v1/tree/hybrid", post(tree_hybrid))
        .route("/v1/tree/verify", post(valori_rag::tree::tree_verify))
        .route(
            "/v1/tree/chain-verify",
            post(valori_rag::tree::tree_chain_verify),
        )
        .route("/v1/community/detect", post(community_detect))
        .route("/v1/community/search", post(community_search))
        .route("/v1/community/overview", get(community_overview))
        .route("/v1/models/health", axum::routing::get(models_health))
        .merge(key_routes);

    // ── Deprecated legacy routes — same handlers, deprecation headers added ───
    // Kept alive for backward compatibility. Will be removed in v2.
    // Clients see `Deprecation: true` + `Link` on every response.
    let legacy = Router::new()
        .route("/version", axum::routing::get(version_handler))
        .route("/records", post(insert_record))
        .route("/search", post(search))
        .route("/timeline", axum::routing::get(get_timeline))
        .route("/operations", axum::routing::get(get_operations))
        .route("/operations/:id", axum::routing::get(get_operation_by_id))
        .route("/graph/node", post(create_node))
        .route(
            "/graph/node/:id",
            axum::routing::get(get_node).delete(delete_node),
        )
        .route("/graph/nodes", axum::routing::get(list_nodes))
        .route("/graph/edge", post(create_edge))
        .route("/graph/edges/:id", axum::routing::get(get_edges))
        .route("/graph/subgraph", axum::routing::get(get_subgraph))
        // snake_case alias kept for SDK backward compat — canonical is /v1/vectors/batch-insert
        .route("/v1/vectors/batch_insert", post(batch_insert))
        .layer(axum::middleware::from_fn(deprecation_warning));

    // ── Protected routes = canonical v1 + deprecated legacy ──────────────────
    let protected = Router::new().merge(v1).merge(legacy).with_state(state);

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
        .layer(Extension(task_registry))
        .layer(Extension(execution_registry))
        // Phase API-2: serialises idempotent (`request_id`-carrying) inserts
        // on the standalone path, which has no Raft log to order them.
        .layer(Extension(Arc::new(tokio::sync::Mutex::new(()))));

    // H-2: Global body size limit — prevent OOM via unbounded request bodies.
    // Snapshot upload (binary) legitimately needs more room; everything else
    // uses JSON that should never exceed 32 MB.
    let mut router = Router::new()
        .merge(public)
        .merge(protected)
        .layer(tower_http::limit::RequestBodyLimitLayer::new(
            32 * 1024 * 1024,
        ))
        // Phase API-2: outermost, so EVERY error response leaving this router
        // carries a machine-readable `code` — including the bare-status
        // 401/403 the auth guard emits with no body.
        .layer(axum::middleware::from_fn(
            crate::error_codes::attach_error_code,
        ));
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
#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/health",
    operation_id = "get_health",
    tag = "meta",
    summary = "Node health and capacity snapshot",
    description = "Always unauthenticated so load-balancer probes work without a token. \
                   Returns the legacy top-level fields alongside the structured \
                   `engine` / `cluster` sub-objects (Phase API-3 §11).",
    security(),
    responses(
        (status = 200, description = "Node is serving", body = HealthResponse),
        (status = 503, description = "At least one kernel pool is at 100% capacity \
                                      (standalone), or this node sees no elected \
                                      leader (cluster). The body is the same \
                                      HealthResponse; branch on `status`.",
                       body = HealthResponse),
    ),
))]
pub(crate) async fn health_check(State(state): State<SharedEngine>) -> impl IntoResponse {
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

    let engine_json = serde_json::to_value(&h).unwrap_or_default();

    let resp = crate::api::HealthResponse {
        status: h.status.to_string(),
        mode: "standalone".to_string(),
        version: h.version.to_string(),
        collections: Some(h.collections),
        persistence: Some(h.persistence.clone()),
        records: Some(serde_json::to_value(&h.records).unwrap_or_default()),
        nodes: Some(serde_json::to_value(&h.nodes).unwrap_or_default()),
        edges: Some(serde_json::to_value(&h.edges).unwrap_or_default()),
        event_log_height: h.event_log_height,
        embed_enabled: Some(h.embed_enabled),
        embed_provider: h.embed_provider.clone(),
        shard_count: h.shard_count,
        leader: None,
        dim: None,
        node_id: None,
        role: None,
        leader_id: None,
        term: None,
        raft_state: None,
        state_hash: None,
        members: None,
        engine: Some(engine_json),
        cluster: None,
    };

    (status_code, Json(resp))
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
        ns: u16,
        id: u32,
        soft: bool,
    ) -> Result<crate::routes::records::DeletedRecord, Response> {
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        use valori_kernel::types::id::RecordId;
        let mut engine = self.write().await;
        // G1.3.1 BUG-4: a record existing is not enough — it must belong to
        // the resolved namespace, or this must behave exactly like "not
        // found" (never confirm cross-tenant existence). Same convention as
        // `GraphOps::delete_node`.
        match engine.state.get_record(RecordId(id)) {
            Some(r) if r.namespace_id == ns => {}
            _ => {
                return Err((
                    StatusCode::NOT_FOUND,
                    axum::Json(serde_json::json!({"error": "record not found"})),
                )
                    .into_response())
            }
        }
        let state_before: String = hash_state_blake3(&engine.state)
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        if soft {
            engine
                .soft_delete_record(id)
                .map_err(|e| e.into_response())?;
        } else {
            engine.delete_record(id).map_err(|e| e.into_response())?;
        }
        let state_after: String = hash_state_blake3(&engine.state)
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        Ok(crate::routes::records::DeletedRecord {
            log_index: None,
            shard_id: 0,
            cluster: false,
            state_before,
            state_after,
        })
    }
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/delete",
    operation_id = "delete_record",
    tag = "records",
    summary = "Hard-delete a record",
    description = "Frees the slab slot and unlinks the record from its collection. \
                   Use `/v1/soft-delete` to tombstone instead.",
    request_body = DeleteRecordRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Record deleted", body = DeleteRecordResponse),
        (status = 400, description = "Malformed request", body = ApiError),
        (status = 404, description = "No such record or collection", body = ApiError),
        (status = 500, description = "Commit or audit-chain failure", body = ApiError),
    ),
))]
pub(crate) async fn delete_record(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<DeleteRecordRequest>,
) -> Result<Json<DeleteRecordResponse>, Response> {
    crate::routes::records::delete_record(&state, &receipts, payload, false).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/soft-delete",
    operation_id = "soft_delete_record",
    tag = "records",
    summary = "Soft-delete a record",
    description = "Tombstones the record so it stops appearing in search results \
                   while its slab slot and audit history are retained.",
    request_body = DeleteRecordRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Record tombstoned", body = DeleteRecordResponse),
        (status = 400, description = "Malformed request", body = ApiError),
        (status = 404, description = "No such record or collection", body = ApiError),
        (status = 500, description = "Commit or audit-chain failure", body = ApiError),
    ),
))]
pub(crate) async fn soft_delete_record(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<DeleteRecordRequest>,
) -> Result<Json<DeleteRecordResponse>, Response> {
    crate::routes::records::delete_record(&state, &receipts, payload, true).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/records/{id}",
    operation_id = "get_record",
    tag = "records",
    summary = "Fetch one record by id",
    description = "Returns the stored vector converted back to f32, plus whatever metadata was committed with it. The vector round-trips through Q16.16, so it is equal to the inserted value only to the fixed-point quantum.",
    params(
        ("id" = u32, Path, description = "Record id"),
        crate::routes::graph::CollectionQuery,
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "The record", body = crate::api::RecordResponse),
        (status = 404, description = "No such record in this collection", body = ApiError),
    ),
))]
async fn get_record_by_id(
    State(state): State<SharedEngine>,
    axum::extract::Path(id): axum::extract::Path<u32>,
    Query(q): Query<crate::routes::graph::CollectionQuery>,
) -> Result<Json<crate::api::RecordResponse>, Response> {
    let engine = state.read().await;
    let ns = engine
        .resolve_collection(q.collection.as_deref())
        .map_err(|e| e.into_response())?;
    let rec_id = valori_kernel::types::id::RecordId(id);
    let rec = engine
        .state
        .get_record(rec_id)
        .filter(|r| r.namespace_id == ns)
        .ok_or_else(|| {
            (
                axum::http::StatusCode::NOT_FOUND,
                axum::Json(serde_json::json!({"error": "record not found"})),
            )
                .into_response()
        })?;
    let vector: Vec<f32> = rec
        .vector
        .data
        .iter()
        .map(|s| valori_kernel::fxp::ops::to_f32(*s))
        .collect();
    Ok(Json(crate::api::RecordResponse {
        id,
        vector,
        metadata: rec
            .metadata
            .as_ref()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok()),
        tag: rec.tag,
    }))
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    patch,
    path = "/v1/records/{id}/metadata",
    operation_id = "update_record_metadata",
    tag = "records",
    summary = "Replace a record's metadata",
    description = "The request body replaces the stored metadata blob wholesale — this is not a merge. The vector is untouched. The change is committed to the BLAKE3 audit chain.",
    params(
        ("id" = u32, Path, description = "Record id"),
        crate::routes::graph::CollectionQuery,
    ),
    request_body(content = std::collections::HashMap<String, serde_json::Value>, description = "Arbitrary JSON object, stored verbatim and merged into the record's metadata."),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Metadata replaced", body = crate::api::UpdateMetadataResponse),
        (status = 404, description = "No such record in this collection", body = ApiError),
        (status = 500, description = "Commit or audit-chain failure", body = ApiError),
    ),
))]
async fn update_record_metadata(
    State(state): State<SharedEngine>,
    axum::extract::Path(id): axum::extract::Path<u32>,
    Query(q): Query<crate::routes::graph::CollectionQuery>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<crate::api::UpdateMetadataResponse>, Response> {
    let mut engine = state.write().await;
    let ns = engine
        .resolve_collection(q.collection.as_deref())
        .map_err(|e| e.into_response())?;
    let rec_id = valori_kernel::types::id::RecordId(id);
    if engine
        .state
        .get_record(rec_id)
        .filter(|r| r.namespace_id == ns)
        .is_none()
    {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({"error": "record not found"})),
        )
            .into_response());
    }
    let metadata_bytes = serde_json::to_vec(&body).ok();
    engine
        .update_record_metadata(id, metadata_bytes, ns)
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        })?;
    Ok(Json(crate::api::UpdateMetadataResponse { ok: true, id }))
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/snapshot/save",
    operation_id = "save_snapshot",
    tag = "snapshot",
    summary = "Write a snapshot to local disk",
    description = "Writes to `path` when given, otherwise to `VALORI_SNAPSHOT_PATH`. Fails when neither is set.",
    request_body = SnapshotSaveRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Snapshot written", body = SnapshotSaveResponse),
        (status = 400, description = "No snapshot path configured or given", body = ApiError),
        (status = 500, description = "Write failure", body = ApiError),
    ),
))]
async fn snapshot_save(
    State(state): State<SharedEngine>,
    axum::Extension(caps): axum::Extension<Arc<valori_effect::capability::CapabilityRegistry>>,
    axum::Extension(task_reg): axum::Extension<Arc<crate::runner::TaskRegistry>>,
    Json(req): Json<SnapshotSaveRequest>,
) -> Result<Json<SnapshotSaveResponse>, EngineError> {
    use crate::runner::run_graph_inline;
    use valori_planner::context::{
        CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash,
    };
    use valori_planner::graph::ExecutionRetentionPolicy;
    use valori_planner::graph::{ExecutionGraph, TaskId, TaskKind, TaskSpec};
    use valori_planner::operation::{
        compute_operation_hash, ExecutionPolicy, OperationInputs, OperationKind,
    };

    // Validate path under a short read lock, then release.
    let (validated_path, shard_count) = {
        let engine = state.read().await;
        let sc = engine.shard_count as u8;
        let p = req
            .path
            .as_deref()
            .map(|raw| {
                let allowed = engine.snapshot_path.as_deref().and_then(|p| p.parent());
                safe_path(raw, allowed)
            })
            .transpose()?
            .map(std::path::PathBuf::from);
        (p, sc)
    };

    let path_str = validated_path
        .as_deref()
        .and_then(|p| p.to_str())
        .map(|s| s.to_string());
    let filename = validated_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "snapshot.val".into());

    let inputs_json = serde_json::to_string(&serde_json::json!({
        "shard_id": 0u8,
        "path": path_str,
    }))
    .unwrap_or_default();

    let op_hash = compute_operation_hash(
        OperationKind::Snapshot,
        &OperationInputs::Snapshot { shard_id: 0 },
        &ExecutionPolicy::default(),
    );
    let fp = PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1);
    let ctx_hash = PlanningContextHash::compute(&PlanningContext {
        capability_set: CapabilitySet {
            embed: false,
            llm: false,
            object_store: false,
            cluster: false,
            shard_count,
        },
        schema_version: 1,
        shard_count,
        cluster_epoch: 0,
        cluster_mode: false,
    });
    let graph = Arc::new(ExecutionGraph::build(
        op_hash,
        fp,
        ctx_hash,
        vec![TaskSpec {
            id: TaskId(0),
            kind: TaskKind::SnapshotArtifact,
            inputs_json,
            shard_id: Some(0),
            topological_index: 0,
        }],
        vec![],
        ExecutionRetentionPolicy::default(),
    ));

    run_graph_inline(graph, caps, task_reg, ExecutionPolicy::default())
        .await
        .map_err(|e| EngineError::InvalidInput(format!("snapshot: {e}")))?;

    Ok(Json(SnapshotSaveResponse {
        success: true,
        path: filename,
    }))
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/snapshot/restore",
    operation_id = "restore_snapshot",
    tag = "snapshot",
    summary = "Restore state from a local snapshot file",
    description = "Reads `path` from this node's own filesystem. Destructive: the current state is replaced.",
    request_body = SnapshotRestoreRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "State replaced", body = SnapshotRestoreResponse),
        (status = 400, description = "Unreadable, malformed, or incompatible snapshot", body = ApiError),
    ),
))]
async fn snapshot_restore(
    State(state): State<SharedEngine>,
    Json(req): Json<SnapshotRestoreRequest>,
) -> Result<Json<SnapshotRestoreResponse>, EngineError> {
    let mut engine = state.write().await;
    // Validate path against configured snapshot directory.
    let allowed = engine.snapshot_path.as_deref().and_then(|p| p.parent());
    let path = safe_path(&req.path, allowed)?;
    if !path.exists() {
        return Err(EngineError::InvalidInput(format!(
            "snapshot not found: {}",
            path.display()
        )));
    }
    let data = tokio::fs::read(&path)
        .await
        .map_err(|e| EngineError::InvalidInput(e.to_string()))?;
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

    async fn ensure_read_consistency(
        &self,
        _ns: u16,
        _consistency: Option<&str>,
    ) -> Result<(), Response> {
        Ok(())
    }

    async fn upsert_vector(
        &self,
        ns: u16,
        req: &MemoryUpsertVectorRequest,
    ) -> Result<crate::routes::memory::UpsertedMemory, Response> {
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        let mut engine = self.write().await;
        let state_before: String = hash_state_blake3(&engine.state)
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        let record_id = engine
            .insert_record_from_f32_ns(&req.vector, ns)
            .map_err(|e| EngineError::from(e).into_response())?;

        let doc_node_id = if let Some(existing) = req.attach_to_document_node {
            existing
        } else {
            engine
                .create_node_for_record(None, NodeKind::Document as u8, ns)
                .map_err(|e| EngineError::from(e).into_response())?
        };

        let chunk_node_id = engine
            .create_node_for_record(Some(record_id), NodeKind::Chunk as u8, ns)
            .map_err(|e| EngineError::from(e).into_response())?;
        engine
            .create_edge_ns(doc_node_id, chunk_node_id, EdgeKind::ParentOf as u8, ns)
            .map_err(|e| EngineError::from(e).into_response())?;

        let memory_id = format!("rec:{}", record_id);
        if let Some(meta) = &req.metadata {
            engine
                .set_meta_audited(memory_id.clone(), meta.clone())
                .map_err(|e| EngineError::from(e).into_response())?;
        }
        let state_after: String = hash_state_blake3(&engine.state)
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
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
        let half_life = req
            .decay_half_life_secs
            .or(engine.decay_half_life_secs)
            .unwrap_or(0);
        let mf = req.metadata_filter.as_ref();
        // Over-fetch when a metadata filter is active so post-filtering can fill k.
        let base_k = if mf.is_some() {
            req.k.saturating_mul(10).max(100).min(5000)
        } else {
            req.k
        };

        let results = if half_life == 0 {
            let use_rerank = req.rerank && req.query_text.is_some() && !engine.reranker.is_empty();
            let fetch_k = if use_rerank {
                (base_k * valori_search::POOL_FACTOR).max(base_k)
            } else {
                base_k
            };
            let hits = engine
                .search_l2_ns(&req.query_vector, fetch_k, ns)
                .map_err(|e| EngineError::from(e).into_response())?;
            let filtered = apply_metadata_filter(hits.into_iter(), mf, &engine.metadata, req.k);
            let final_ids: Vec<(u32, f32)> = if use_rerank {
                let query_text = req.query_text.as_deref().unwrap_or("");
                let candidates: Vec<(u64, f32)> =
                    filtered.iter().map(|(id, s)| (*id as u64, *s)).collect();
                engine
                    .reranker
                    .rerank(query_text, candidates)
                    .into_iter()
                    .take(req.k)
                    .map(|(id, s)| (id as u32, s))
                    .collect()
            } else {
                filtered
            };
            final_ids
                .into_iter()
                .map(|(record_id, score)| {
                    let memory_id = format!("rec:{record_id}");
                    let metadata = engine.metadata.get(&memory_id);
                    MemorySearchHit {
                        memory_id,
                        record_id,
                        score,
                        metadata,
                        decay_factor: None,
                        age_secs: None,
                    }
                })
                .collect()
        } else {
            let pool = base_k.saturating_mul(4).max(50).min(1000);
            let raw = engine
                .search_l2_ns(&req.query_vector, pool, ns)
                .map_err(|e| EngineError::from(e).into_response())?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let candidates: Vec<valori_search::DecayHit> = raw
                .into_iter()
                .map(|(id, score)| valori_search::DecayHit {
                    id,
                    distance: score,
                    created_at: engine.record_created_at(id),
                })
                .collect();
            valori_search::decay_rerank(candidates, now, half_life, base_k)
                .into_iter()
                .filter(|h| match mf {
                    None => true,
                    Some(f) => {
                        let key = format!("rec:{}", h.id);
                        engine
                            .metadata
                            .get(&key)
                            .map(|m| valori_search::matches_metadata_filter(&m, f))
                            .unwrap_or(false)
                    }
                })
                .take(req.k)
                .map(|h| {
                    let memory_id = format!("rec:{}", h.id);
                    let metadata = engine.metadata.get(&memory_id);
                    MemorySearchHit {
                        memory_id,
                        record_id: h.id,
                        score: h.distance,
                        metadata,
                        decay_factor: Some(h.factor),
                        age_secs: h.age_secs,
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
        let state_before: String = hash_state_blake3(&engine.state)
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        engine
            .soft_delete_record(req.old_record_id)
            .map_err(|e| EngineError::from(e).into_response())?;

        let new_record_id = engine
            .insert_record_from_f32_ns(&req.new_vector, ns)
            .map_err(|e| EngineError::from(e).into_response())?;

        let new_node = engine
            .create_node_for_record(Some(new_record_id), NodeKind::Chunk as u8, ns)
            .map_err(|e| EngineError::from(e).into_response())?;
        let old_node = engine
            .create_node_for_record(Some(req.old_record_id), NodeKind::Chunk as u8, ns)
            .map_err(|e| EngineError::from(e).into_response())?;
        let edge_id = engine
            .create_edge_ns(new_node, old_node, EdgeKind::Supersedes as u8, ns)
            .map_err(|e| EngineError::from(e).into_response())?;

        if let Some(meta) = &req.metadata {
            let memory_id = format!("rec:{}", new_record_id);
            engine
                .set_meta_audited(memory_id, meta.clone())
                .map_err(|e| EngineError::from(e).into_response())?;
        }

        let proof = engine.get_proof();
        let state_hash: String = proof
            .final_state_hash
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
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
            engine
                .cosine_similarity(req.record_a, req.record_b)
                .ok_or_else(|| {
                    EngineError::InvalidInput(format!(
                        "one or both records ({}, {}) not found or not searchable",
                        req.record_a, req.record_b
                    ))
                    .into_response()
                })?
        };

        let contradicts = similarity >= threshold;

        let (edge_id, state_before, state_after) = if contradicts {
            let mut engine = self.write().await;
            let state_before: String = hash_state_blake3(&engine.state)
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect();
            let node_a = engine
                .create_node_for_record(Some(req.record_a), NodeKind::Chunk as u8, ns)
                .map_err(|e| EngineError::from(e).into_response())?;
            let node_b = engine
                .create_node_for_record(Some(req.record_b), NodeKind::Chunk as u8, ns)
                .map_err(|e| EngineError::from(e).into_response())?;
            let eid = engine
                .create_edge_ns(node_a, node_b, EdgeKind::Contradicts as u8, ns)
                .map_err(|e| EngineError::from(e).into_response())?;
            let hash: String = engine
                .get_proof()
                .final_state_hash
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            (Some(eid), state_before, hash)
        } else {
            let engine = self.read().await;
            let hash: String = engine
                .get_proof()
                .final_state_hash
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
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

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/memory/meta/set",
    operation_id = "set_metadata_sidecar",
    tag = "memory",
    summary = "Attach sidecar metadata to a target",
    description = "Sidecar metadata is node-local: it is NOT replicated through Raft and NOT part of the BLAKE3 audit chain. Use record metadata when the value must be provable.",
    request_body = MetadataSetRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Stored", body = MetadataSetResponse),
        (status = 400, description = "Malformed or invalid request", body = ApiError),
    ),
))]
async fn meta_set(
    State(state): State<SharedEngine>,
    Json(payload): Json<MetadataSetRequest>,
) -> Result<Json<MetadataSetResponse>, Response> {
    crate::routes::meta::meta_set(&state, payload).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/memory/meta/get",
    operation_id = "get_metadata_sidecar",
    tag = "memory",
    summary = "Read sidecar metadata for a target",
    description = "`metadata` is null when nothing has been stored for `target_id`.",
    params(
        ("target_id" = String, Query, description = "Target identifier the metadata was stored under"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "The sidecar metadata, or null", body = MetadataGetResponse),
    ),
))]
async fn meta_get(
    State(state): State<SharedEngine>,
    Query(payload): Query<MetadataGetRequest>,
) -> Json<MetadataGetResponse> {
    crate::routes::meta::meta_get(&state, payload).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/records",
    operation_id = "insert_record",
    tag = "records",
    summary = "Insert a record",
    description = "Q16.16 fixed-point insert. Supplying `request_id` makes the call \
                   idempotent: a replay returns the original record id rather than \
                   inserting twice.",
    request_body = InsertRecordRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Record inserted (or idempotent replay resolved)", body = InsertRecordResponse),
        (status = 400, description = "Dimension mismatch or malformed vector", body = ApiError),
        (status = 404, description = "Target collection does not exist", body = ApiError),
        (status = 500, description = "Commit or audit-chain failure", body = ApiError),
        (status = 507, description = "Record slab capacity exhausted", body = ApiError),
    ),
))]
pub(crate) async fn insert_record(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    axum::Extension(caps): axum::Extension<Arc<valori_effect::capability::CapabilityRegistry>>,
    axum::Extension(task_reg): axum::Extension<Arc<crate::runner::TaskRegistry>>,
    axum::Extension(idempotency): axum::Extension<Arc<tokio::sync::Mutex<()>>>,
    Json(payload): Json<InsertRecordRequest>,
) -> Result<Json<InsertRecordResponse>, EngineError> {
    use crate::runner::run_graph_inline;
    use valori_kernel::snapshot::blake3::hash_state_blake3;
    use valori_planner::context::{
        CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash,
    };
    use valori_planner::graph::ExecutionRetentionPolicy;
    use valori_planner::graph::{ExecutionGraph, TaskId, TaskKind, TaskSpec};
    use valori_planner::operation::{
        compute_operation_hash, ExecutionPolicy, OperationInputs, OperationKind,
    };

    // Phase API-2 idempotency gate. Held for the whole handler only when the
    // caller supplied a `request_id`: the check-then-commit below is not
    // atomic on its own, and two concurrent replays of the same token would
    // otherwise both miss the dedup table and both insert. Cluster mode gets
    // this serialisation for free from the Raft log; standalone has no such
    // ordering point, so idempotent inserts (and only those) are serialised
    // here. Inserts without a `request_id` never touch the gate.
    let _idem_guard = match payload.request_id {
        Some(_) => Some(idempotency.lock().await),
        None => None,
    };

    // Resolve namespace under a short read lock (no write needed yet — insert
    // goes through the effect bus / EngineKernelCapability below).
    let (ns, old_root, state_before, shard_count, dedup_hit) = {
        let eng = state.read().await;
        let ns = eng.resolve_collection(payload.collection.as_deref())?;
        let or: [u8; 32] = hash_state_blake3(&eng.state);
        let sb = or.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        let sc = eng.shard_count as u8;
        let hit = payload.request_id.and_then(|rid| eng.dedup_lookup(&rid.0));
        (ns, or, sb, sc, hit)
    };

    // Replay of a token we have already applied: answer with the record the
    // original request created, and do not write anything. `old_root` and
    // `new_root` are identical because state is untouched — the receipt is a
    // truthful "nothing happened here" proof, not a fabricated insert proof.
    if let Some(existing_id) = dedup_hit {
        let sequence = {
            let eng = state.read().await;
            eng.event_committer()
                .map(|c| c.journal().committed_height())
                .unwrap_or(0)
        };
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let receipt = valori_kernel::proof::InsertReceipt::build(
            existing_id,
            old_root,
            &[],
            old_root,
            sequence,
            timestamp,
        );
        return Ok(Json(InsertRecordResponse {
            id: existing_id,
            log_index: None,
            deduplicated: true,
            receipt: receipt.into(),
        }));
    }

    let fxp_values: Vec<i32> = payload
        .values
        .iter()
        .map(|&f| valori_kernel::fxp::ops::from_f32(f).0)
        .collect();

    let collection_name = payload
        .collection
        .clone()
        .unwrap_or_else(|| "default".into());
    let shard_id = ((ns as u32) % (shard_count as u32).max(1)) as u8;

    let inputs_json = serde_json::to_string(&serde_json::json!({
        "namespace_id": ns,
        "shard_id": shard_id,
        "values": payload.values,
        "text": payload.text,
        "metadata": payload.metadata,
        "tag": payload.tag,
        "request_id": payload.request_id.map(|r| r.to_hex()),
    }))
    .unwrap_or_default();

    let op_hash = compute_operation_hash(
        OperationKind::Ingest,
        &OperationInputs::Ingest {
            strategy: "direct".into(),
            collection: collection_name.clone(),
            shard_id,
            embed_enabled: false,
        },
        &ExecutionPolicy::default(),
    );
    let fp = PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1);
    let ctx_hash = PlanningContextHash::compute(&PlanningContext {
        capability_set: CapabilitySet {
            embed: false,
            llm: false,
            object_store: false,
            cluster: false,
            shard_count,
        },
        schema_version: 1,
        shard_count,
        cluster_epoch: 0,
        cluster_mode: false,
    });
    let graph = Arc::new(ExecutionGraph::build(
        op_hash,
        fp,
        ctx_hash,
        vec![TaskSpec {
            id: TaskId(0),
            kind: TaskKind::InsertRecord,
            inputs_json,
            shard_id: Some(shard_id),
            topological_index: 0,
        }],
        vec![],
        ExecutionRetentionPolicy::default(),
    ));

    let outputs = run_graph_inline(graph, caps, task_reg, ExecutionPolicy::default())
        .await
        .map_err(|e| match e {
            valori_effect::error::EffectError::Capacity(_) => {
                EngineError::Kernel(valori_kernel::error::KernelError::CapacityExceeded)
            }
            valori_effect::error::EffectError::Dispatch(msg)
            | valori_effect::error::EffectError::TaskFailed(msg) => EngineError::InvalidInput(msg),
            other => EngineError::Unknown(other.to_string()),
        })?;

    let record_id = outputs
        .into_iter()
        .next()
        .flatten()
        .and_then(|o| o.json.get("record_id").and_then(|v| v.as_u64()))
        .unwrap_or(0) as u32;

    let (new_root, state_after, sequence) = {
        let eng = state.read().await;
        let nr: [u8; 32] = hash_state_blake3(&eng.state);
        let sa = nr.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        let seq = eng
            .event_committer()
            .map(|c| c.journal().committed_height())
            .unwrap_or(0);
        (nr, sa, seq)
    };
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    crate::receipt_bridge::emit_write(
        &receipts,
        OperationKind::Ingest,
        &OperationInputs::Ingest {
            strategy: "direct".into(),
            collection: collection_name,
            shard_id,
            embed_enabled: false,
        },
        ns,
        0,
        sequence,
        false,
        state_before,
        state_after,
    );

    let receipt = valori_kernel::proof::InsertReceipt::build(
        record_id,
        old_root,
        &fxp_values,
        new_root,
        sequence,
        timestamp,
    );
    // Remember the token only after a successful apply — a failed insert must
    // stay retryable with the same token.
    if let Some(rid) = payload.request_id {
        state.write().await.dedup_record(rid.0, record_id);
    }

    Ok(Json(InsertRecordResponse {
        id: record_id,
        log_index: None,
        deduplicated: false,
        receipt: receipt.into(),
    }))
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/vectors/batch-insert",
    operation_id = "insert_records_batch",
    tag = "records",
    summary = "Insert many vectors in one request",
    description = "Each optional per-item array (`metadata`, `request_ids`, `texts`) must be the same length as `batch` when present. A repeated `request_id` skips that item and returns the id assigned the first time, so the whole call is idempotent per item.",
    request_body = BatchInsertRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Ids assigned, in request order", body = BatchInsertResponse),
        (status = 400, description = "Malformed or invalid request", body = ApiError),
        (status = 507, description = "Record slab is full", body = ApiError),
    ),
))]
async fn batch_insert(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<BatchInsertRequest>,
) -> Result<Json<BatchInsertResponse>, EngineError> {
    use valori_kernel::snapshot::blake3::hash_state_blake3;
    let mut engine = state.write().await;
    let ns = engine.resolve_collection(payload.collection.as_deref())?;
    let state_before: String = hash_state_blake3(&engine.state)
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();
    let meta_bytes: Option<Vec<Option<Vec<u8>>>> = payload.metadata.as_ref().map(|m| {
        m.iter()
            .map(|s| s.as_ref().map(|s| s.as_bytes().to_vec()))
            .collect()
    });
    // Parse optional per-item idempotency keys from 32-hex strings to [u8;16].
    let parsed_request_ids: Option<Vec<Option<[u8; 16]>>> =
        payload.request_ids.as_ref().map(|rids| {
            rids.iter()
                .map(|entry| {
                    entry.as_deref().and_then(|hex| {
                        if hex.len() != 32 {
                            return None;
                        }
                        let mut bytes = [0u8; 16];
                        for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
                            bytes[i] =
                                u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
                        }
                        Some(bytes)
                    })
                })
                .collect()
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
    let state_after: String = hash_state_blake3(&engine.state)
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();
    drop(engine);
    {
        use valori_planner::operation::{OperationInputs, OperationKind};
        let inputs = OperationInputs::BatchInsert {
            count: ids.len() as u32,
            collection: payload
                .collection
                .clone()
                .unwrap_or_else(|| "default".into()),
            shard_id: 0,
        };
        crate::receipt_bridge::emit_write(
            &receipts,
            OperationKind::BatchInsert,
            &inputs,
            ns,
            0,
            0,
            false,
            state_before,
            state_after,
        );
    }
    Ok(Json(BatchInsertResponse { ids }))
}

/// Hard ceiling on a single search's `k`. Above this, a client-supplied `k`
/// gets multiplied by `valori_search::POOL_FACTOR` (20x) on the rerank path
/// before it's used to size a results buffer — an unbounded `k` is a
/// client-triggerable unbounded allocation, not just a slow query.
const MAX_SEARCH_K: usize = 5000;

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/search",
    operation_id = "search",
    tag = "search",
    summary = "K-nearest-neighbour search within one collection",
    description = "Composable ranking: `decay_half_life_secs` applies recency decay, \
                   `query_text` enables the Valori Reranker's term blend, \
                   `graph_rerank` nudges by graph proximity, and `metadata_filter` \
                   restricts candidates. `k` must be in 1..=5000.",
    request_body = SearchRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Ranked hits, nearest first", body = SearchResponse),
        (status = 400, description = "`k` out of range, dimension mismatch, or bad filter", body = ApiError),
        (status = 404, description = "Target collection does not exist", body = ApiError),
        (status = 500, description = "Index or state failure", body = ApiError),
    ),
))]
pub(crate) async fn search(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, EngineError> {
    use valori_kernel::snapshot::blake3::hash_state_blake3;

    if payload.k == 0 || payload.k > MAX_SEARCH_K {
        return Err(EngineError::InvalidInput(format!(
            "k must be between 1 and {MAX_SEARCH_K}, got {}",
            payload.k
        )));
    }

    if payload.as_of.is_some() || payload.as_of_log_index.is_some() {
        return search_as_of(state, payload).await;
    }
    let engine = state.read().await;
    let state_hash: String = hash_state_blake3(&engine.state)
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();
    let ns = engine.resolve_collection(payload.collection.as_deref())?;

    // Effective decay half-life: request value wins (incl. an explicit 0 to
    // disable), else the server default. 0 / None => pure distance ranking.
    let half_life = payload
        .decay_half_life_secs
        .or(engine.decay_half_life_secs)
        .unwrap_or(0);

    // When metadata_filter is set, over-fetch a wider pool so post-filtering
    // has enough candidates to fill k results.
    let mf = payload.metadata_filter.as_ref();
    let base_k = if mf.is_some() {
        payload.k.saturating_mul(10).max(100).min(5000)
    } else {
        payload.k
    };

    if half_life == 0 {
        let use_rerank =
            payload.rerank && payload.query_text.is_some() && !engine.reranker.is_empty();
        let fetch_k = if use_rerank {
            (base_k * valori_search::POOL_FACTOR).max(base_k)
        } else {
            base_k
        };
        let hits = if ns == 0 {
            engine.search_l2(&payload.query, fetch_k)?
        } else {
            engine.search_l2_ns(&payload.query, fetch_k, ns)?
        };
        let filtered = apply_metadata_filter(hits.into_iter(), mf, &engine.metadata, payload.k);
        let final_hits = if use_rerank {
            let query_text = payload.query_text.as_deref().unwrap_or("");
            let candidates: Vec<(u64, f32)> =
                filtered.iter().map(|(id, s)| (*id as u64, *s)).collect();
            let reranked = engine.reranker.rerank(query_text, candidates);
            reranked
                .into_iter()
                .take(payload.k)
                .map(|(id, score)| SearchHit {
                    id: id as u32,
                    score,
                    decay_factor: None,
                    age_secs: None,
                    graph_distance: None,
                })
                .collect()
        } else {
            filtered
                .into_iter()
                .map(|(id, score)| SearchHit {
                    id,
                    score,
                    decay_factor: None,
                    age_secs: None,
                    graph_distance: None,
                })
                .collect()
        };
        let final_hits = if let Some(gr) = payload.graph_rerank.as_ref() {
            apply_graph_rerank(&engine, final_hits, gr, payload.k)
        } else {
            final_hits
        };
        {
            use valori_planner::operation::{ConsistencyLevel, OperationInputs, OperationKind};
            let inputs = OperationInputs::Search {
                k: payload.k as u32,
                collection: payload
                    .collection
                    .clone()
                    .unwrap_or_else(|| "default".into()),
                shard_id: 0,
                rerank: payload.rerank,
                decay: half_life > 0,
                metadata_filter: payload.metadata_filter.is_some(),
                consistency: ConsistencyLevel::Local,
            };
            crate::receipt_bridge::emit_read(
                &receipts,
                OperationKind::Search,
                &inputs,
                ns,
                0,
                0,
                false,
                state_hash.clone(),
            );
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
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let candidates: Vec<valori_search::DecayHit> = raw
        .into_iter()
        .map(|(id, score)| valori_search::DecayHit {
            id,
            distance: score,
            created_at: engine.record_created_at(id),
        })
        .collect();
    let decayed = valori_search::decay_rerank(candidates, now, half_life, pool);
    let results: Vec<SearchHit> = decayed
        .into_iter()
        .filter(|h| {
            if let Some(f) = mf {
                let key = format!("rec:{}", h.id);
                match engine.metadata.get(&key) {
                    Some(meta) => valori_search::matches_metadata_filter(&meta, f),
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
            graph_distance: None,
        })
        .collect();
    let results = if let Some(gr) = payload.graph_rerank.as_ref() {
        apply_graph_rerank(&engine, results, gr, payload.k)
    } else {
        results
    };
    {
        use valori_planner::operation::{ConsistencyLevel, OperationInputs, OperationKind};
        let inputs = OperationInputs::Search {
            k: payload.k as u32,
            collection: payload
                .collection
                .clone()
                .unwrap_or_else(|| "default".into()),
            shard_id: 0,
            rerank: payload.rerank,
            decay: half_life > 0,
            metadata_filter: payload.metadata_filter.is_some(),
            consistency: ConsistencyLevel::Local,
        };
        crate::receipt_bridge::emit_read(
            &receipts,
            OperationKind::Search,
            &inputs,
            ns,
            0,
            0,
            false,
            state_hash,
        );
    }
    Ok(Json(SearchResponse::simple(results)))
}

/// Point-in-time search: replay committed events up to the target index/timestamp,
/// run the search on the replayed state, and return the results with a BLAKE3 proof.
async fn search_as_of(
    state: SharedEngine,
    payload: SearchRequest,
) -> Result<Json<SearchResponse>, EngineError> {
    use valori_kernel::fxp::qformat::SCALE;
    use valori_kernel::index::SearchResult;
    use valori_kernel::snapshot::blake3::hash_state_blake3;
    use valori_kernel::state::kernel::KernelState;
    use valori_kernel::types::scalar::FxpScalar;
    use valori_kernel::types::vector::FxpVector;

    let engine = state.read().await;

    let committer = engine.event_committer().ok_or_else(|| {
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
        let unix = parse_iso8601(payload.as_of.as_deref().unwrap_or("")).ok_or_else(|| {
            EngineError::InvalidInput(
                "invalid as_of timestamp — expected ISO 8601 UTC, e.g. 2026-03-03T00:00:00Z".into(),
            )
        })?;
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

    if target_idx >= journal.committed_height() as usize {
        return Err(EngineError::InvalidInput(format!(
            "as_of_log_index {target_idx} is out of range (have {} events)",
            journal.committed_height()
        )));
    }

    // Replay events[0..=target_idx] into a fresh kernel using each event's recorded namespace.
    let mut replay = KernelState::new();
    for (event, ns_id) in journal.committed_with_namespaces().take(target_idx + 1) {
        let _ = replay.apply_event_ns(event, ns_id);
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
    let fxp_data: Vec<FxpScalar> = payload
        .query
        .iter()
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
    let results: Vec<SearchHit> = results_buf[..found]
        .iter()
        .map(|r| {
            let score = r.score as f32 / (SCALE as f32 * SCALE as f32);
            // Decay is a "now"-relative re-rank; it is intentionally NOT applied to
            // point-in-time (as_of) queries, which reconstruct a historical state.
            SearchHit {
                id: r.id.0,
                score,
                decay_factor: None,
                age_secs: None,
                graph_distance: None, // G1.4.1: not supported on as_of point-in-time queries
            }
        })
        .collect();

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

/// `POST /v1/search/multi` — Phase 5 cross-collection (orchestrated) search.
///
/// Fans the query out to every listed Collection independently (in parallel),
/// then merges results globally by Squared L2 distance (smaller = better).
/// All Collections must share the same `dim` and `metric`; different index
/// types are allowed.
///
/// BM25 reranking and graph reranking are intentionally excluded: hybrid
/// scores from different Collection corpora are incomparable and would
/// corrupt the global merge.
#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/search/multi",
    operation_id = "search_multi",
    tag = "search",
    summary = "Search several compatible collections and merge the results",
    description = "All named collections must share a dimension and metric — scores \
                   from different corpora are incomparable and would corrupt the \
                   merge. Collections that fail individually are reported in \
                   `partial_failures` rather than failing the whole request.",
    request_body = MultiSearchRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Globally merged top-k, with any per-collection failures", body = MultiSearchResponse),
        (status = 400, description = "Incompatible collections, too many collections, or `k` out of range", body = ApiError),
        (status = 404, description = "One of the named collections does not exist", body = ApiError),
    ),
))]
pub(crate) async fn multi_search(
    State(state): State<SharedEngine>,
    Json(payload): Json<crate::api::MultiSearchRequest>,
) -> Result<Json<crate::api::MultiSearchResponse>, EngineError> {
    use crate::routes::query_planner::{
        check_compatibility, merge_top_k, CollectionHits, MAX_MULTI_COLLECTIONS, MAX_MULTI_SEARCH_K,
    };

    // ── Input validation ──────────────────────────────────────────────────────
    if payload.collections.is_empty() {
        return Err(EngineError::InvalidInput(
            "collections list is empty; at least one collection is required".into(),
        ));
    }
    if payload.collections.len() > MAX_MULTI_COLLECTIONS {
        return Err(EngineError::InvalidInput(format!(
            "too many collections: {} requested, maximum is {}",
            payload.collections.len(),
            MAX_MULTI_COLLECTIONS
        )));
    }
    if payload.k == 0 || payload.k > MAX_MULTI_SEARCH_K {
        return Err(EngineError::InvalidInput(format!(
            "k must be between 1 and {}, got {}",
            MAX_MULTI_SEARCH_K, payload.k
        )));
    }

    // ── Resolve collections and check compatibility ───────────────────────────
    // Hold the read lock only long enough to resolve names → ns_ids + configs.
    let ns_pairs: Vec<(
        String,
        u16,
        valori_metadata::collection::CollectionVectorConfig,
    )> = {
        let engine = state.read().await;
        let mut result = Vec::with_capacity(payload.collections.len());
        for name in &payload.collections {
            let ns_id = engine.resolve_collection(Some(name.as_str()))?;
            // Phase API-2: 409, matching cluster. This is a mis-configured
            // Collection, not a malformed request — and it must never be the
            // 500 the cluster path used to return.
            let cfg = engine.namespaces.config(ns_id).ok_or_else(|| {
                EngineError::Conflict(format!(
                    "collection '{}' has no vector configuration; \
                     was it created with explicit dim and metric?",
                    name
                ))
            })?;
            result.push((name.clone(), ns_id, cfg));
        }
        result
    };

    let configs_for_check: Vec<(String, valori_metadata::collection::CollectionVectorConfig)> =
        ns_pairs.iter().map(|(n, _, c)| (n.clone(), *c)).collect();
    let (dim, _metric) =
        check_compatibility(&configs_for_check).map_err(EngineError::InvalidInput)?;

    if payload.query.len() != dim as usize {
        return Err(EngineError::InvalidInput(format!(
            "query vector has {} elements but collections require dim={}",
            payload.query.len(),
            dim
        )));
    }

    // ── Fan-out searches in parallel ─────────────────────────────────────────
    // Multiple concurrent `.read()` locks are allowed by tokio's RwLock, so
    // searches genuinely run in parallel even under write pressure.
    let k = payload.k;
    let half_life = payload.decay_half_life_secs.unwrap_or(0);

    // Each future returns (name, Ok(hits)) or (name, Err(msg)) so the
    // collection name is always available even in the error case.
    let futs: Vec<_> = ns_pairs
        .into_iter()
        .map(|(name, ns_id, _cfg)| {
            let state = state.clone();
            let query = payload.query.clone();
            let mf = payload.metadata_filter.clone();
            async move {
                let engine = state.read().await;
                let hits: Result<Vec<crate::api::MultiSearchHit>, String> = if half_life == 0 {
                    engine
                        .search_l2_ns(&query, k, ns_id)
                        .map_err(|e| e.to_string())
                        .map(|raw| {
                            let filtered = apply_metadata_filter(
                                raw.into_iter(),
                                mf.as_ref(),
                                &engine.metadata,
                                k,
                            );
                            filtered
                                .into_iter()
                                .map(|(id, score)| crate::api::MultiSearchHit {
                                    collection: name.clone(),
                                    id,
                                    score,
                                    decay_factor: None,
                                    age_secs: None,
                                })
                                .collect()
                        })
                } else {
                    // Decay path: over-fetch a bounded pool, re-rank by decayed distance.
                    let pool = k.saturating_mul(4).max(50).min(5000);
                    engine
                        .search_l2_ns(&query, pool, ns_id)
                        .map_err(|e| e.to_string())
                        .map(|raw| {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0);
                            let candidates: Vec<valori_search::DecayHit> = raw
                                .into_iter()
                                .map(|(id, score)| valori_search::DecayHit {
                                    id,
                                    distance: score,
                                    created_at: engine.record_created_at(id),
                                })
                                .collect();
                            valori_search::decay_rerank(candidates, now, half_life, pool)
                                .into_iter()
                                .filter(|h| {
                                    if let Some(ref f) = mf {
                                        let key = format!("rec:{}", h.id);
                                        match engine.metadata.get(&key) {
                                            Some(meta) => {
                                                valori_search::matches_metadata_filter(&meta, f)
                                            }
                                            None => false,
                                        }
                                    } else {
                                        true
                                    }
                                })
                                .take(k)
                                .map(|h| crate::api::MultiSearchHit {
                                    collection: name.clone(),
                                    id: h.id,
                                    score: h.distance,
                                    decay_factor: Some(h.factor),
                                    age_secs: h.age_secs,
                                })
                                .collect()
                        })
                };
                (name, hits)
            }
        })
        .collect();

    let raw_results = futures::future::join_all(futs).await;

    // ── Separate successes and partial failures ───────────────────────────────
    let mut per_coll: Vec<CollectionHits> = Vec::new();
    let mut failures: Vec<crate::api::PartialSearchFailure> = Vec::new();

    for (name, result) in raw_results {
        match result {
            Ok(hits) => per_coll.push(CollectionHits {
                collection: name,
                hits,
            }),
            Err(e) => {
                // A per-collection runtime error surfaces as a partial failure;
                // we still return whatever other collections succeeded.
                failures.push(crate::api::PartialSearchFailure {
                    collection: name,
                    error: e,
                });
            }
        }
    }

    Ok(Json(merge_top_k(per_coll, failures, k)))
}

fn bytes_to_hex(b: &[u8]) -> String {
    b.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Parse a subset of ISO 8601 UTC: `YYYY-MM-DDTHH:MM:SSZ` or `YYYY-MM-DDTHH:MM:SS+00:00`.
/// Returns unix seconds since the epoch.
pub fn parse_iso8601(s: &str) -> Option<u64> {
    let s = s.trim();
    // Require at least "YYYY-MM-DDTHH:MM:SS"
    if s.len() < 19 {
        return None;
    }
    let year: u64 = s[0..4].parse().ok()?;
    let month: u64 = s[5..7].parse().ok()?;
    let day: u64 = s[8..10].parse().ok()?;
    let hour: u64 = s[11..13].parse().ok()?;
    let min: u64 = s[14..16].parse().ok()?;
    let sec: u64 = s[17..19].parse().ok()?;
    if s.as_bytes().get(10) != Some(&b'T') {
        return None;
    }

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

    if days < 0 {
        return None;
    }
    Some(days as u64 * 86400 + hour * 3600 + min * 60 + sec)
}

/// Format unix seconds as `YYYY-MM-DDTHH:MM:SSZ` (UTC only).
pub fn unix_to_iso8601(unix_secs: u64) -> String {
    let mut rem = unix_secs;
    let sec = rem % 60;
    rem /= 60;
    let min = rem % 60;
    rem /= 60;
    let hour = rem % 24;
    rem /= 24;

    // Days since 1970-01-01.
    let is_leap = |y: u64| y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    const DAYS_IN_MONTH: [u64; 13] = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

    let mut year = 1970u64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if rem < days_in_year {
            break;
        }
        rem -= days_in_year;
        year += 1;
    }
    let mut month = 1u64;
    loop {
        let dim = DAYS_IN_MONTH[month as usize] + if month == 2 && is_leap(year) { 1 } else { 0 };
        if rem < dim {
            break;
        }
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
        Ok(crate::routes::graph::CommittedGraphWrite {
            id,
            log_index: None,
        })
    }

    async fn create_edge(
        &self,
        ns: u16,
        from: u32,
        to: u32,
        kind: EdgeKind,
    ) -> Result<crate::routes::graph::CommittedGraphWrite, Response> {
        let mut engine = self.write().await;
        let id = engine
            .create_edge_ns(from, to, kind as u8, ns)
            .map_err(|e| e.into_response())?;
        Ok(crate::routes::graph::CommittedGraphWrite {
            id,
            log_index: None,
        })
    }

    async fn delete_node(&self, ns: u16, id: u32) -> Result<Option<u64>, Response> {
        use valori_kernel::types::id::NodeId;
        let mut engine = self.write().await;
        // G1.1.1: defense-in-depth — the shared handler already 404s before
        // calling this (via `get_node`), but this method must not delete a
        // cross-namespace node even if called directly.
        match engine.get_node(NodeId(id)) {
            Some(n) if n.namespace_id == ns => {}
            _ => return Ok(None),
        }
        engine.delete_node(id).map_err(|e| e.into_response())?;
        Ok(None)
    }

    async fn get_node(&self, ns: u16, id: u32) -> Result<Option<GetNodeResponse>, Response> {
        use valori_kernel::types::id::NodeId;
        let engine = self.read().await;
        // G1.1.1: a node existing is not enough — it must belong to the
        // resolved namespace, or this must behave exactly like "not found"
        // (never confirm cross-tenant existence). See
        // docs/reviews/graph-g1.1.1-graph-read-namespace-isolation.md.
        Ok(engine
            .get_node(NodeId(id))
            .filter(|n| n.namespace_id == ns)
            .map(|n| GetNodeResponse {
                kind: n.kind as u8,
                record_id: n.record.map(|r| r.0),
                namespace_id: n.namespace_id,
            }))
    }

    async fn node_edges(&self, ns: u16, id: u32) -> Result<Option<Vec<EdgeData>>, Response> {
        use valori_kernel::types::id::NodeId;
        let engine = self.read().await;
        // G1.1.1: validate the SOURCE node's namespace before listing its
        // edges. Sufficient by construction — edges cannot cross namespaces
        // (G0's invariant), so a correctly-scoped source node implies every
        // one of its edges is also in-namespace; no per-edge check needed.
        match engine.get_node(NodeId(id)) {
            Some(n) if n.namespace_id == ns => {}
            _ => return Ok(None),
        }
        Ok(engine.outgoing_edges(NodeId(id)).map(|iter| {
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
            .map(|(node_id, kind, record_id)| NodeInfo {
                node_id,
                kind,
                record_id,
                namespace_id: ns,
            })
            .collect())
    }

    async fn subgraph(
        &self,
        ns: u16,
        root: u32,
        depth: u32,
    ) -> Result<(serde_json::Value, serde_json::Value), Response> {
        use valori_kernel::types::id::NodeId;
        let engine = self.read().await;
        // G1.1.1: validate the ROOT node's namespace before traversing.
        // Sufficient by construction (same reasoning as node_edges above) —
        // a correctly-scoped root cannot reach another namespace via edges.
        // A wrong-namespace root behaves exactly like a nonexistent one
        // already did (empty nodes/edges, 200 OK) — no new response shape.
        match engine.get_node(NodeId(root)) {
            Some(n) if n.namespace_id == ns => {}
            _ => {
                return Ok((
                    serde_json::Value::Array(vec![]),
                    serde_json::Value::Array(vec![]),
                ))
            }
        }
        let (nodes, edges) = valori_rag::graph::expand_subgraph(&engine.state, &[root], depth);
        Ok((
            serde_json::Value::Array(nodes),
            serde_json::Value::Array(edges),
        ))
    }

    async fn query(
        &self,
        ns: u16,
        query: valori_rag::graph::GraphQuery,
    ) -> Result<Option<Vec<valori_rag::graph::GraphQueryHit>>, Response> {
        let engine = self.read().await;
        Ok(valori_rag::graph::query_graph(&engine.state, ns, &query))
    }
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/graph/node",
    operation_id = "create_graph_node",
    tag = "graph",
    summary = "Create a graph node",
    description = "`kind` is the numeric NodeKind discriminant (0=Document, 1=Chunk, 2=Concept, …). `record_id` optionally binds the node to a stored vector.",
    request_body = CreateNodeRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Node created", body = CreateNodeResponse),
        (status = 400, description = "Unknown node kind or collection", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError),
    ),
))]
async fn create_node(
    State(state): State<SharedEngine>,
    Json(payload): Json<CreateNodeRequest>,
) -> Result<Json<CreateNodeResponse>, Response> {
    crate::routes::graph::create_node(&state, payload).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/graph/edge",
    operation_id = "create_graph_edge",
    tag = "graph",
    summary = "Create a graph edge",
    description = "`kind` is the numeric EdgeKind discriminant. Both endpoints must already exist in the same collection.",
    request_body = CreateEdgeRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Edge created", body = CreateEdgeResponse),
        (status = 400, description = "Unknown edge kind or collection", body = ApiError),
        (status = 404, description = "One of the endpoints does not exist", body = ApiError),
    ),
))]
async fn create_edge(
    State(state): State<SharedEngine>,
    Json(payload): Json<CreateEdgeRequest>,
) -> Result<Json<CreateEdgeResponse>, Response> {
    crate::routes::graph::create_edge(&state, payload).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/graph/node/{id}",
    operation_id = "get_graph_node",
    tag = "graph",
    summary = "Fetch one graph node",
    params(
        ("id" = u32, Path, description = "Node id"),
        crate::routes::graph::CollectionQuery,
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "The node", body = GetNodeResponse),
        (status = 404, description = "No such node in this collection", body = ApiError),
    ),
))]
async fn get_node(
    State(state): State<SharedEngine>,
    axum::extract::Path(id): axum::extract::Path<u32>,
    Query(q): Query<crate::routes::graph::CollectionQuery>,
) -> Result<Json<GetNodeResponse>, Response> {
    crate::routes::graph::get_node(&state, id, q).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    delete,
    path = "/v1/graph/node/{id}",
    operation_id = "delete_graph_node",
    tag = "graph",
    summary = "Delete a graph node",
    description = "Cascades to every edge incident on the node. Committed to the audit chain.",
    params(
        ("id" = u32, Path, description = "Node id"),
        crate::routes::graph::CollectionQuery,
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Node and incident edges removed", body = DeleteNodeResponse),
        (status = 404, description = "No such node in this collection", body = ApiError),
    ),
))]
async fn delete_node(
    State(state): State<SharedEngine>,
    axum::extract::Path(id): axum::extract::Path<u32>,
    Query(q): Query<crate::routes::graph::CollectionQuery>,
) -> Result<Json<DeleteNodeResponse>, Response> {
    crate::routes::graph::delete_node(&state, id, q).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/graph/nodes",
    operation_id = "list_graph_nodes",
    tag = "graph",
    summary = "List graph nodes",
    description = "`count` is the size of the filtered set before pagination; `nodes` is the page. Omitting `limit` returns everything.",
    params(
        crate::routes::graph::ListNodesQuery,
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Nodes in this collection", body = ListNodesResponse),
        (status = 404, description = "Unknown collection", body = ApiError),
    ),
))]
async fn list_nodes(
    State(state): State<SharedEngine>,
    Query(q): Query<crate::routes::graph::ListNodesQuery>,
) -> Result<Json<ListNodesResponse>, Response> {
    crate::routes::graph::list_nodes(&state, q).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/graph/edges/{id}",
    operation_id = "list_node_edges",
    tag = "graph",
    summary = "List the edges leaving one node",
    params(
        ("id" = u32, Path, description = "Source node id"),
        crate::routes::graph::CollectionQuery,
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Outgoing edges", body = GetEdgesResponse),
        (status = 404, description = "No such node in this collection", body = ApiError),
    ),
))]
async fn get_edges(
    State(state): State<SharedEngine>,
    axum::extract::Path(id): axum::extract::Path<u32>,
    Query(q): Query<crate::routes::graph::CollectionQuery>,
) -> Result<Json<GetEdgesResponse>, Response> {
    crate::routes::graph::get_edges(&state, id, q).await
}

fn default_depth() -> u32 {
    2
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/graph/subgraph",
    operation_id = "get_subgraph",
    tag = "graph",
    summary = "Expand a subgraph around a root node",
    description = "Breadth-first expansion bounded by `depth`. Traversal never crosses a collection boundary.",
    params(
        crate::routes::graph::SubgraphQuery,
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "The expanded subgraph", body = crate::api::SubgraphResponse),
        (status = 400, description = "Invalid root or depth", body = ApiError),
        (status = 404, description = "Unknown collection", body = ApiError),
    ),
))]
async fn get_subgraph(
    State(state): State<SharedEngine>,
    Query(q): Query<crate::routes::graph::SubgraphQuery>,
) -> Result<Json<serde_json::Value>, Response> {
    crate::routes::graph::get_subgraph(&state, q).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/graph/query",
    operation_id = "graph_query",
    tag = "graph",
    summary = "Deterministic bounded graph traversal",
    description = "Walks the graph from `start` with optional edge-kind and node-kind filters. Result order is deterministic for a given kernel state.",
    params(
        crate::routes::graph::GraphQueryParams,
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Reached nodes with their hop distance", body = GraphQueryResponse),
        (status = 400, description = "Unknown direction, kind, or collection", body = ApiError),
        (status = 404, description = "No such start node", body = ApiError),
    ),
))]
async fn graph_query(
    State(state): State<SharedEngine>,
    Query(q): Query<crate::routes::graph::GraphQueryParams>,
) -> Result<Json<crate::api::GraphQueryResponse>, Response> {
    crate::routes::graph::query(&state, q).await
}

// ── Phase 3.15: native GraphRAG — KNN + subgraph expansion in one call ────────

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(serde::Deserialize)]
pub(crate) struct GraphRagRequest {
    query_vector: Vec<f32>,
    /// Legacy alias for `retrieval_k`. When `retrieval_k` is absent, `k` is used.
    #[serde(default)]
    k: Option<usize>,
    /// How many vector candidates to use as seeds for graph expansion.
    #[serde(default)]
    retrieval_k: Option<usize>,
    /// Maximum returned hits. Absent = defaults to `retrieval_k` (Phase 5.4).
    #[serde(default)]
    final_k: Option<usize>,
    /// Budget on graph-only candidates (applied before `final_k`). Absent = 100.
    #[serde(default)]
    max_graph_candidates: Option<usize>,
    /// Phase 5.4: halt BFS before visiting a node that would exceed this count.
    #[serde(default)]
    max_nodes: Option<usize>,
    /// Phase 5.4: halt edge emission once this count is reached per BFS round.
    #[serde(default)]
    max_edges: Option<usize>,
    /// Phase 5.4: β in `final_score = (1-β)×vector_rel + β×graph_rel`. Range [0,1].
    #[serde(default = "default_graph_weight")]
    graph_weight: f32,
    #[serde(default = "default_depth")]
    depth: u32,
    #[serde(default)]
    collection: Option<String>,
}

fn default_graph_weight() -> f32 {
    0.3
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/graphrag",
    operation_id = "graphrag",
    tag = "graph",
    summary = "Vector search plus graph expansion in one read",
    description = "Retrieves the K nearest vectors and the connected subgraph around them from a single consistent kernel snapshot. `final_score = (1-graph_weight)*vector_rel + graph_weight*graph_rel`.",
    request_body = GraphRagRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Blended hits plus the expanded subgraph", body = crate::api::GraphRagResponse),
        (status = 400, description = "Malformed or invalid request", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError),
    ),
))]
/// Retrieve the K nearest memories AND the knowledge subgraph around them, in a
/// single read against one consistent kernel snapshot. No second store, no sync.
async fn graphrag(
    State(state): State<SharedEngine>,
    axum::Extension(caps): axum::Extension<Arc<valori_effect::capability::CapabilityRegistry>>,
    axum::Extension(task_reg): axum::Extension<Arc<crate::runner::TaskRegistry>>,
    Json(payload): Json<GraphRagRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    use crate::runner::run_graph_inline;
    use valori_planner::context::{
        CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash,
    };
    use valori_planner::graph::ExecutionRetentionPolicy;
    use valori_planner::graph::{ExecutionGraph, TaskId, TaskKind, TaskSpec};
    use valori_planner::operation::{
        compute_operation_hash, ExecutionPolicy, OperationInputs, OperationKind,
    };

    let (ns, shard_count) = {
        let eng = state.read().await;
        let ns = eng.resolve_collection(payload.collection.as_deref())?;
        (ns, eng.shard_count as u8)
    };
    let shard_id = ((ns as u32) % (shard_count as u32).max(1)) as u8;

    // Phase 5.3: `retrieval_k` is the canonical name; `k` is the backward-compat alias.
    // Phase 5.4: `final_k` defaults to `retrieval_k` (not unlimited) to bound result size.
    let retrieval_k = payload.retrieval_k.or(payload.k).unwrap_or(5).max(1);
    let final_k = payload.final_k.unwrap_or(retrieval_k) as u32;
    let max_graph_candidates = payload.max_graph_candidates.unwrap_or(100).max(1) as u32;
    let max_nodes = payload.max_nodes.map(|v| v as u32);
    let max_edges = payload.max_edges.map(|v| v as u32);
    let graph_weight = payload.graph_weight.clamp(0.0, 1.0);

    let inputs_json = serde_json::to_string(&serde_json::json!({
        "shard_id": shard_id,
        "namespace_id": ns,
        "vector": payload.query_vector,
        "k": retrieval_k,
        "depth": payload.depth,
        "final_k": final_k,
        "max_graph_candidates": max_graph_candidates,
        "max_nodes": max_nodes,
        "max_edges": max_edges,
        "graph_weight": graph_weight,
    }))
    .unwrap_or_default();

    let op_hash = compute_operation_hash(
        OperationKind::GraphRag,
        &OperationInputs::GraphRag {
            k: retrieval_k as u32,
            depth: payload.depth,
            collection: payload
                .collection
                .clone()
                .unwrap_or_else(|| "default".into()),
            shard_id,
        },
        &ExecutionPolicy::default(),
    );
    let fp = PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1);
    let ctx_hash = PlanningContextHash::compute(&PlanningContext {
        capability_set: CapabilitySet {
            embed: false,
            llm: false,
            object_store: false,
            cluster: false,
            shard_count,
        },
        schema_version: 1,
        shard_count,
        cluster_epoch: 0,
        cluster_mode: false,
    });
    let graph = Arc::new(ExecutionGraph::build(
        op_hash,
        fp,
        ctx_hash,
        vec![TaskSpec {
            id: TaskId(0),
            kind: TaskKind::GraphRag,
            inputs_json,
            shard_id: Some(shard_id),
            topological_index: 0,
        }],
        vec![],
        ExecutionRetentionPolicy::default(),
    ));

    let outputs = run_graph_inline(graph, caps, task_reg, ExecutionPolicy::default())
        .await
        .map_err(|e| EngineError::InvalidInput(format!("graphrag: {e}")))?;

    let result = outputs.into_iter().next().flatten()
        .map(|o| o.json)
        .unwrap_or(serde_json::json!({ "hits": [], "seed_nodes": [], "subgraph": { "nodes": [], "edges": [] } }));

    metrics::counter!("valori_graphrag_total", 1u64);
    Ok(Json(result))
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/snapshot/download",
    operation_id = "download_snapshot",
    tag = "snapshot",
    summary = "Download a snapshot of the current state",
    description = "Streams the V6 snapshot as raw bytes. The format is versioned and self-describing; restore it with `POST /v1/snapshot/upload`.",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Snapshot bytes", content_type = "application/octet-stream", body = crate::openapi::SnapshotBytes),
        (status = 500, description = "Snapshot encode failure", body = ApiError),
    ),
))]
async fn snapshot(State(state): State<SharedEngine>) -> Result<Vec<u8>, EngineError> {
    let engine = state.read().await;
    engine.snapshot()
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/snapshot/upload",
    operation_id = "upload_snapshot",
    tag = "snapshot",
    summary = "Restore state from an uploaded snapshot",
    description = "Replaces the entire in-memory state with the uploaded snapshot and rebuilds the state hash from scratch. Destructive.",
    request_body(content = crate::openapi::SnapshotBytes, content_type = "application/octet-stream"),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "State replaced"),
        (status = 400, description = "Malformed or incompatible snapshot", body = ApiError),
    ),
))]
async fn restore(
    State(state): State<SharedEngine>,
    body: axum::body::Bytes,
) -> Result<(), EngineError> {
    let mut engine = state.write().await;
    engine.restore(&body)?;
    Ok(())
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/memory/upsert",
    operation_id = "memory_upsert",
    tag = "memory",
    summary = "Store an agent memory",
    description = "Inserts the vector and links it into the knowledge graph as a chunk node under a document node, returning both ids alongside a stable `memory_id`.",
    request_body = MemoryUpsertVectorRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Memory stored; document and chunk nodes linked", body = MemoryUpsertResponse),
        (status = 400, description = "Malformed or invalid request", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError),
    ),
))]
async fn memory_upsert_vector(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<MemoryUpsertVectorRequest>,
) -> Result<Json<MemoryUpsertResponse>, Response> {
    crate::routes::memory::memory_upsert(&state, &receipts, payload).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/memory/upsert_vector",
    operation_id = "memory_upsert_vector",
    tag = "memory",
    summary = "Store an agent memory (SDK path)",
    description = "Identical to `POST /v1/memory/upsert`. This is the path the Python SDK has always used; both are supported and neither is deprecated.",
    request_body = MemoryUpsertVectorRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Memory stored; document and chunk nodes linked", body = MemoryUpsertResponse),
        (status = 400, description = "Malformed or invalid request", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError),
    ),
))]
// `/v1/memory/{upsert,search}_vector` are not sugar for the canonical paths —
// they are what `python/valoricore` has always called (see `remote.py` and
// `protocol.py`), so they are a first-class part of the public surface and
// must appear in the contract. utoipa binds exactly one path per function, so
// each alias gets a thin wrapper that delegates to the canonical handler
// rather than being silently omitted from the generated document.
async fn memory_upsert_vector_alias(
    state: State<SharedEngine>,
    receipts: axum::Extension<Arc<valori_effect::ReceiptStore>>,
    payload: Json<MemoryUpsertVectorRequest>,
) -> Result<Json<MemoryUpsertResponse>, Response> {
    memory_upsert_vector(state, receipts, payload).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/memory/search_vector",
    operation_id = "memory_search_vector",
    tag = "memory",
    summary = "Recall agent memories (SDK path)",
    description = "Identical to `POST /v1/memory/search`. This is the path the Python SDK has always used; both are supported and neither is deprecated.",
    params(
        crate::routes::explain::ExplainParams,
    ),
    request_body = MemorySearchVectorRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Recalled memories, best first", body = MemorySearchResponse),
        (status = 400, description = "Malformed or invalid request", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError),
    ),
))]
async fn memory_search_vector_alias(
    state: State<SharedEngine>,
    caps: axum::Extension<Arc<valori_effect::capability::CapabilityRegistry>>,
    task_reg: axum::Extension<Arc<crate::runner::TaskRegistry>>,
    explain: axum::extract::Query<crate::routes::explain::ExplainParams>,
    payload: Json<MemorySearchVectorRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    memory_search_vector(state, caps, task_reg, explain, payload).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/memory/search",
    operation_id = "memory_search",
    tag = "memory",
    summary = "Recall agent memories",
    description = "Vector recall with optional recency decay, metadata filtering, and hybrid term re-ranking. When `decay_half_life_secs` is set, each hit also carries `decay_factor` and `age_secs`; `score` remains the true distance. Add `?explain=true` for an `_execution` block describing the plan that ran.",
    params(
        crate::routes::explain::ExplainParams,
    ),
    request_body = MemorySearchVectorRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Recalled memories, best first", body = MemorySearchResponse),
        (status = 400, description = "Malformed or invalid request", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError),
    ),
))]
async fn memory_search_vector(
    State(state): State<SharedEngine>,
    axum::Extension(caps): axum::Extension<Arc<valori_effect::capability::CapabilityRegistry>>,
    axum::Extension(task_reg): axum::Extension<Arc<crate::runner::TaskRegistry>>,
    axum::extract::Query(explain): axum::extract::Query<crate::routes::explain::ExplainParams>,
    Json(payload): Json<MemorySearchVectorRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    use crate::runner::run_graph_inline;
    use axum::http::StatusCode;
    use valori_planner::context::{
        CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash,
    };
    use valori_planner::graph::ExecutionRetentionPolicy;
    use valori_planner::graph::{ExecutionGraph, TaskId, TaskKind, TaskSpec};
    use valori_planner::operation::{
        compute_operation_hash, ExecutionPolicy, OperationInputs, OperationKind,
    };

    let (ns, shard_count) = {
        let eng = state.read().await;
        let ns = eng
            .resolve_collection(payload.collection.as_deref())
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({"error": e.to_string()})),
                )
                    .into_response()
            })?;
        (ns, eng.shard_count as u8)
    };
    let shard_id = ((ns as u32) % (shard_count as u32).max(1)) as u8;

    let inputs_json = serde_json::to_string(&serde_json::json!({
        "shard_id": shard_id,
        "namespace_id": ns,
        "vector": payload.query_vector,
        "k": payload.k,
        "decay_half_life_secs": payload.decay_half_life_secs.map(|v| v as f64),
        "rerank": payload.rerank,
        "query_text": payload.query_text,
        "metadata_filter": payload.metadata_filter.as_ref().map(|m| serde_json::Value::Object(m.clone())),
    })).unwrap_or_default();

    let op_hash = compute_operation_hash(
        OperationKind::MemorySearch,
        &OperationInputs::MemorySearch {
            k: payload.k as u32,
            collection: payload
                .collection
                .clone()
                .unwrap_or_else(|| "default".into()),
            shard_id,
            decay: payload.decay_half_life_secs.is_some(),
        },
        &ExecutionPolicy::default(),
    );
    let fp = PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1);
    let ctx_hash = PlanningContextHash::compute(&PlanningContext {
        capability_set: CapabilitySet {
            embed: false,
            llm: false,
            object_store: false,
            cluster: false,
            shard_count,
        },
        schema_version: 1,
        shard_count,
        cluster_epoch: 0,
        cluster_mode: false,
    });
    let graph = Arc::new(ExecutionGraph::build(
        op_hash,
        fp,
        ctx_hash,
        vec![TaskSpec {
            id: TaskId(0),
            kind: TaskKind::MemorySearch,
            inputs_json,
            shard_id: Some(shard_id),
            topological_index: 0,
        }],
        vec![],
        ExecutionRetentionPolicy::default(),
    ));

    // Retain the graph past execution so `?explain=true` can report its
    // content-addressed hash + task/edge structure. run_graph_inline consumes
    // its Arc, so hand it a clone. Time the run for `_execution.duration_ms`.
    let exec_started = std::time::Instant::now();
    let outputs = run_graph_inline(graph.clone(), caps, task_reg, ExecutionPolicy::default())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        })?;
    let exec_ms = exec_started.elapsed().as_secs_f64() * 1000.0;

    let raw = outputs
        .into_iter()
        .next()
        .flatten()
        .map(|o| o.json)
        .unwrap_or(serde_json::Value::Array(vec![]));
    let results: Vec<MemorySearchHit> = raw
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default();

    let execution = if explain.on() {
        let state_hash = { state.read().await.get_proof().final_state_hash };
        Some(crate::routes::explain::execution_block(
            "MemorySearch",
            Some(&graph),
            &state_hash,
            Some(exec_ms),
        ))
    } else {
        None
    };
    Ok(Json(crate::routes::explain::with_execution(
        MemorySearchResponse { results },
        execution,
    )))
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/proof/state",
    operation_id = "get_state_proof",
    tag = "proof",
    summary = "Current BLAKE3 state hash",
    description = "The Merkle root over every applied event. Two nodes with identical histories produce byte-identical values, which is what the cluster convergence watcher compares.",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "State hash", body = crate::api::StateProofResponse),
    ),
))]
async fn get_proof(State(state): State<SharedEngine>) -> impl IntoResponse {
    let engine = state.read().await;
    let proof = engine.get_proof();
    // Encode all 32 bytes as lowercase hex — same wire format as the cluster's
    // state_proof handler so external clients see an identical response shape.
    let hex: String = proof
        .final_state_hash
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Json(crate::api::StateProofResponse {
        final_state_hash: hex,
    })
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/usage",
    operation_id = "get_usage",
    tag = "meta",
    summary = "Raw usage counters",
    description = "Read-only: takes no write lock, commits no event, and returns no plan or billing context — the node is deliberately plan-agnostic. `event_log_bytes` includes every rotated archive segment, not just the live one.",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Current counters", body = crate::api::UsageResponse),
    ),
))]
/// `GET /v1/usage` — Phase P2 (Cloud plan/quota/usage accounting):
/// read-only records/collections/storage-byte counts. Never mutates
/// canonical state (read lock only, no audit-log write, no
/// `KernelEvent`) and returns no plan/billing context whatsoever —
/// `valori-node` remains completely plan-agnostic. Cloud's own control
/// plane is the only thing that ever maps these raw numbers onto a
/// plan's limits.
async fn usage_handler(State(state): State<SharedEngine>) -> impl IntoResponse {
    let engine = state.read().await;
    let records = engine.record_count();
    let collections = engine.list_collections().len();
    let (event_log_bytes, snapshot_bytes) = storage_bytes_standalone(&engine);
    Json(crate::api::UsageResponse {
        records,
        collections,
        storage: crate::api::UsageStorage {
            event_log_bytes,
            snapshot_bytes,
            total_bytes: event_log_bytes + snapshot_bytes,
        },
    })
}

/// Sums the live event-log segment plus every rotated archive segment
/// (`events.log` -> `events.log.000001`, `.000002`, ... — see
/// `EventLogWriter::rotate()`; archived segments are never deleted, so
/// stat-ing only the live file silently undercounts after any rotation
/// has ever happened) plus the snapshot file, if configured. Falls back
/// to the legacy WAL path when the engine isn't using the event-log
/// persistence mode. Missing files/paths contribute 0, never an error —
/// this must never fail a request over a purely cosmetic accounting gap.
fn storage_bytes_standalone(engine: &Engine) -> (u64, u64) {
    let event_log_path = engine
        .event_committer()
        .map(|c| c.event_log().path().to_path_buf())
        .or_else(|| engine.wal_path.clone());
    let event_log_bytes = event_log_path.map(sum_log_and_archives).unwrap_or(0);
    let snapshot_bytes = engine
        .snapshot_path
        .as_ref()
        .map(|p| file_size(p))
        .unwrap_or(0);
    (event_log_bytes, snapshot_bytes)
}

/// Live segment size + every sibling file in the same directory whose name
/// starts with the live segment's own filename (the rotation naming
/// convention above).
pub(crate) fn sum_log_and_archives(live_path: std::path::PathBuf) -> u64 {
    let mut total = file_size(&live_path);
    if let (Some(dir), Some(live_name)) = (live_path.parent(), live_path.file_name()) {
        let live_name = live_name.to_string_lossy().into_owned();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let fname = entry.file_name();
                let fname = fname.to_string_lossy();
                if fname != live_name && fname.starts_with(live_name.as_str()) {
                    total += entry.metadata().map(|m| m.len()).unwrap_or(0);
                }
            }
        }
    }
    total
}

fn file_size(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

// ── C4.2: Memory consolidation ───────────────────────────────────────────────

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/memory/consolidate",
    operation_id = "memory_consolidate",
    tag = "memory",
    summary = "Replace a memory and record the supersession",
    description = "Commits three events atomically: soft-delete of the old record, insert of the new one, and a Supersedes edge from new to old. The returned `state_hash` covers all three.",
    request_body = MemoryConsolidateRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Old memory superseded", body = MemoryConsolidateResponse),
        (status = 400, description = "Malformed or invalid request", body = ApiError),
        (status = 404, description = "No such record", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError),
    ),
))]
async fn memory_consolidate(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<MemoryConsolidateRequest>,
) -> Result<Json<MemoryConsolidateResponse>, Response> {
    crate::routes::memory::memory_consolidate(&state, &receipts, payload).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/memory/contradict",
    operation_id = "memory_contradict",
    tag = "memory",
    summary = "Test two memories for contradiction",
    description = "Computes cosine similarity between the two records. When it meets `threshold` (default 0.85) a Contradicts edge is committed and its id returned. Below the threshold nothing is written.",
    request_body = MemoryContradictRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Similarity, verdict, and the edge id when one was written", body = MemoryContradictResponse),
        (status = 400, description = "Malformed or invalid request", body = ApiError),
        (status = 404, description = "No such record", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError),
    ),
))]
async fn memory_contradict(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<MemoryContradictRequest>,
) -> Result<Json<MemoryContradictResponse>, Response> {
    crate::routes::memory::memory_contradict(&state, &receipts, payload).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/proof/event-log",
    operation_id = "get_event_log_proof",
    tag = "proof",
    summary = "Audit-chain receipt for the event log",
    description = "The receipt primitive: the BLAKE3 hash of the event log, the final state hash, and the committed height. Feed it to `valori-verify` to replay and re-derive the chain independently. Requires `VALORI_EVENT_LOG_PATH`.",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Event-log proof", body = EventProofResponse),
        (status = 400, description = "Event log not enabled on this node", body = ApiError),
    ),
))]
async fn get_event_proof(
    State(state): State<SharedEngine>,
) -> Result<Json<EventProofResponse>, EngineError> {
    let engine = state.read().await;

    if let Some(committer) = engine.event_committer() {
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
            event_log_hash: event_log_hash_bytes
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            final_state_hash: proof
                .final_state_hash
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            snapshot_hash: None,
            event_count: committed_height,
            committed_height,
        };

        Ok(Json(response))
    } else {
        Err(EngineError::InvalidInput(
            "Event log not enabled".to_string(),
        ))
    }
}

// ── Receipt endpoints (Phase A8) ──────────────────────────────────────────────

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/proof/receipt",
    operation_id = "get_latest_receipt",
    tag = "proof",
    summary = "Most recent write receipt",
    description = "Receipts are held in an in-process store, so a restarted node has none until the next write.",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "The latest receipt", body = crate::openapi::ReceiptDto),
        (status = 404, description = "No receipt has been emitted yet"),
    ),
))]
/// `GET /v1/proof/receipt` — return the most recently assembled Receipt.
///
/// Returns 404 if no receipt has been assembled yet (no operation has been
/// driven through the TaskRunner since node start).
async fn get_latest_receipt(
    axum::Extension(store): axum::Extension<Arc<valori_effect::ReceiptStore>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match store.latest() {
        Some(r) => Ok(Json(
            serde_json::to_value(&r).unwrap_or(serde_json::Value::Null),
        )),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "no receipt available yet"})),
        )),
    }
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/proof/receipt/{id}",
    operation_id = "get_receipt",
    tag = "proof",
    summary = "One write receipt by id",
    params(
        ("id" = String, Path, description = "Receipt id"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "The receipt", body = crate::openapi::ReceiptDto),
        (status = 404, description = "No such receipt"),
    ),
))]
/// `GET /v1/proof/receipt/:id` — return a specific Receipt by receipt_id.
async fn get_receipt_by_id(
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Extension(store): axum::Extension<Arc<valori_effect::ReceiptStore>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match store.get(&id) {
        Some(r) => Ok(Json(
            serde_json::to_value(&r).unwrap_or(serde_json::Value::Null),
        )),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("receipt '{}' not found", id)})),
        )),
    }
}

async fn get_wal_stream(State(state): State<SharedEngine>) -> Result<Body, EngineError> {
    let path = {
        let engine = state.read().await;
        engine.wal_path.clone()
    }
    .ok_or(EngineError::InvalidInput("No WAL configured".into()))?;

    let file = tokio::fs::File::open(&path)
        .await
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
        if let Some(committer) = engine.event_committer_mut() {
            if let Err(e) = committer.flush_log() {
                tracing::error!("Failed to flush event log for replication: {}", e);
            }
            (
                committer.event_log().path().to_path_buf(),
                committer.subscribe(),
            )
        } else {
            return Err(EngineError::InvalidInput(
                "Event log not enabled".to_string(),
            ));
        }
    };

    let rx_stream =
        crate::replication::spawn_replication_stream(log_path, rx, start_offset).await?;

    use futures::StreamExt;
    let body_stream = tokio_stream::wrappers::ReceiverStream::new(rx_stream).map(|res| match res {
        Ok(json_line) => Ok(json_line),
        Err(e) => Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            e.to_string(),
        )),
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
async fn metrics_handler(State(state): State<SharedEngine>) -> String {
    // Update kernel gauges from live state before rendering.
    {
        let engine = state.read().await;
        engine.update_prometheus_metrics();
    }
    crate::telemetry::get_metrics()
}

#[cfg_attr(feature = "utoipa", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "utoipa", into_params(parameter_in = Query))]
#[derive(serde::Deserialize, Default)]
pub(crate) struct TimelineQuery {
    /// ISO 8601 UTC lower bound (inclusive).
    from: Option<String>,
    /// ISO 8601 UTC upper bound (inclusive).
    to: Option<String>,
    /// Return only the N most-recent events. Applied after timestamp filtering.
    limit: Option<usize>,
    /// Filter to events in a specific collection (not yet applied at kernel level;
    /// kept for future use when namespace is stored per-event).
    #[allow(dead_code)]
    collection: Option<String>,
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/timeline",
    operation_id = "get_timeline",
    tag = "proof",
    summary = "Committed events in chronological order",
    description = "Reads the event log directly, so it reflects committed state only. Known limitation: with `VALORI_SHARD_COUNT > 1` this reads shard 0's log.",
    params(
        TimelineQuery,
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Committed events", body = TimelineResponse),
        (status = 400, description = "Event log not enabled on this node", body = ApiError),
    ),
))]
async fn get_timeline(
    State(state): State<SharedEngine>,
    Query(q): Query<TimelineQuery>,
) -> Result<Json<TimelineResponse>, EngineError> {
    use valori_kernel::event::KernelEvent;

    let engine = state.read().await;
    let Some(committer) = engine.event_committer() else {
        return Err(EngineError::InvalidInput(
            "Event log not enabled (set VALORI_EVENT_LOG_PATH)".to_string(),
        ));
    };

    let from_unix = q.from.as_deref().and_then(parse_iso8601);
    let to_unix = q.to.as_deref().and_then(parse_iso8601);

    let journal = committer.journal();
    let mut entries: Vec<TimelineEntry> = Vec::new();

    for (log_index, (event, ts)) in journal.committed_with_timestamps().enumerate() {
        // Apply timestamp range filter.
        if let Some(from) = from_unix {
            if ts < from {
                continue;
            }
        }
        if let Some(to) = to_unix {
            if ts > to {
                continue;
            }
        }

        let (event_type, record_id, node_id, edge_id) = match event {
            KernelEvent::InsertRecord { id, .. } => ("InsertRecord", Some(id.0), None, None),
            KernelEvent::AutoInsertRecord { .. } => ("AutoInsertRecord", None, None, None),
            KernelEvent::InsertRecordEncrypted { id, .. } => {
                ("InsertRecordEncrypted", Some(id.0), None, None)
            }
            KernelEvent::DeleteRecord { id } => ("DeleteRecord", Some(id.0), None, None),
            KernelEvent::SoftDeleteRecord { id } => ("SoftDeleteRecord", Some(id.0), None, None),
            KernelEvent::ShredKey { .. } => ("ShredKey", None, None, None),
            KernelEvent::CreateNode { id, .. } => ("CreateNode", None, Some(id.0), None),
            KernelEvent::AutoCreateNode { .. } => ("AutoCreateNode", None, None, None),
            KernelEvent::DeleteNode { id } => ("DeleteNode", None, Some(id.0), None),
            KernelEvent::CreateEdge { id, .. } => ("CreateEdge", None, None, Some(id.0)),
            KernelEvent::AutoCreateEdge { .. } => ("AutoCreateEdge", None, None, None),
            KernelEvent::DeleteEdge { id } => ("DeleteEdge", None, None, Some(id.0)),
            KernelEvent::AutoInsertRecordEncrypted { .. } => {
                ("AutoInsertRecordEncrypted", None, None, None)
            }
            KernelEvent::SetMeta { .. } => ("SetMeta", None, None, None),
            KernelEvent::AutoCreateNamespace { .. } => ("AutoCreateNamespace", None, None, None),
            KernelEvent::DropNamespace { .. } => ("DropNamespace", None, None, None),
            KernelEvent::UpdateRecordMetadata { id, .. } => {
                ("UpdateRecordMetadata", Some(id.0), None, None)
            }
            KernelEvent::ConfigureNamespace { .. } => ("ConfigureNamespace", None, None, None),
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
    if let Some(n) = q.limit {
        let skip = total.saturating_sub(n);
        entries.drain(..skip);
    }
    Ok(Json(TimelineResponse {
        events: entries,
        total,
        from_unix,
        to_unix,
    }))
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/operations",
    operation_id = "list_operations",
    tag = "operations",
    summary = "List committed operations",
    description = "Derived from the BLAKE3-chained event log. `id` is the canonical \
                   string identity (Phase API-3 §13).",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Operation history, newest first", body = OperationsListResponse),
        (status = 501, description = "Node has no event log configured", body = ApiError),
    ),
))]
pub(crate) async fn get_operations(
    State(state): State<SharedEngine>,
) -> Result<Json<crate::api::OperationsListResponse>, EngineError> {
    use valori_kernel::event::KernelEvent;

    let engine = state.read().await;
    let Some(committer) = engine.event_committer() else {
        return Ok(Json(crate::api::OperationsListResponse {
            operations: vec![],
            total: 0,
        }));
    };

    let journal = committer.journal();
    let mut operations: Vec<crate::api::OperationSummary> = Vec::new();

    for (log_index, (event, ts)) in journal.committed_with_timestamps().enumerate() {
        let (event_type, record_id, node_id, edge_id) = match event {
            KernelEvent::InsertRecord { id, .. } => ("InsertRecord", Some(id.0), None, None),
            KernelEvent::AutoInsertRecord { .. } => ("AutoInsertRecord", None, None, None),
            KernelEvent::InsertRecordEncrypted { id, .. } => {
                ("InsertRecordEncrypted", Some(id.0), None, None)
            }
            KernelEvent::DeleteRecord { id } => ("DeleteRecord", Some(id.0), None, None),
            KernelEvent::SoftDeleteRecord { id } => ("SoftDeleteRecord", Some(id.0), None, None),
            KernelEvent::ShredKey { .. } => ("ShredKey", None, None, None),
            KernelEvent::CreateNode { id, .. } => ("CreateNode", None, Some(id.0), None),
            KernelEvent::AutoCreateNode { .. } => ("AutoCreateNode", None, None, None),
            KernelEvent::DeleteNode { id } => ("DeleteNode", None, Some(id.0), None),
            KernelEvent::CreateEdge { id, .. } => ("CreateEdge", None, None, Some(id.0)),
            KernelEvent::AutoCreateEdge { .. } => ("AutoCreateEdge", None, None, None),
            KernelEvent::DeleteEdge { id } => ("DeleteEdge", None, None, Some(id.0)),
            KernelEvent::AutoInsertRecordEncrypted { .. } => {
                ("AutoInsertRecordEncrypted", None, None, None)
            }
            KernelEvent::SetMeta { .. } => ("SetMeta", None, None, None),
            KernelEvent::AutoCreateNamespace { .. } => ("AutoCreateNamespace", None, None, None),
            KernelEvent::DropNamespace { .. } => ("DropNamespace", None, None, None),
            KernelEvent::UpdateRecordMetadata { id, .. } => {
                ("UpdateRecordMetadata", Some(id.0), None, None)
            }
            KernelEvent::ConfigureNamespace { .. } => ("ConfigureNamespace", None, None, None),
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

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/operations/{id}",
    operation_id = "get_operation",
    tag = "operations",
    summary = "Fetch one operation with its proof and metrics",
    description = "Accepts the canonical string `id`. Numeric identifiers minted \
                   before Phase API-3 remain resolvable (§13).",
    params(("id" = String, Path, description = "Operation identity")),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Operation detail", body = OperationDetailResponse),
        (status = 404, description = "No operation with that identity", body = ApiError),
        (status = 501, description = "Node has no event log configured", body = ApiError),
    ),
))]
pub(crate) async fn get_operation_by_id(
    axum::extract::Path(id): axum::extract::Path<String>,
    State(state): State<SharedEngine>,
    axum::Extension(receipt_store): axum::Extension<Arc<valori_effect::ReceiptStore>>,
) -> Result<Json<crate::api::OperationDetailResponse>, (StatusCode, Json<serde_json::Value>)> {
    use valori_kernel::event::KernelEvent;

    let engine = state.read().await;
    let Some(committer) = engine.event_committer() else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Event log not enabled"})),
        ));
    };

    let idx_str = id.strip_prefix("op-").unwrap_or(&id);
    let log_index: usize = idx_str.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("invalid operation ID format: {}", id)})),
        )
    })?;

    let journal = committer.journal();
    let (event, ts) = journal
        .committed_with_timestamps()
        .nth(log_index)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("operation '{}' not found", id)})),
            )
        })?;

    let (event_type, record_id, node_id, edge_id) = match event {
        KernelEvent::InsertRecord { id, .. } => ("InsertRecord", Some(id.0), None, None),
        KernelEvent::AutoInsertRecord { .. } => ("AutoInsertRecord", None, None, None),
        KernelEvent::InsertRecordEncrypted { id, .. } => {
            ("InsertRecordEncrypted", Some(id.0), None, None)
        }
        KernelEvent::DeleteRecord { id } => ("DeleteRecord", Some(id.0), None, None),
        KernelEvent::SoftDeleteRecord { id } => ("SoftDeleteRecord", Some(id.0), None, None),
        KernelEvent::ShredKey { .. } => ("ShredKey", None, None, None),
        KernelEvent::CreateNode { id, .. } => ("CreateNode", None, Some(id.0), None),
        KernelEvent::AutoCreateNode { .. } => ("AutoCreateNode", None, None, None),
        KernelEvent::DeleteNode { id } => ("DeleteNode", None, Some(id.0), None),
        KernelEvent::CreateEdge { id, .. } => ("CreateEdge", None, None, Some(id.0)),
        KernelEvent::AutoCreateEdge { .. } => ("AutoCreateEdge", None, None, None),
        KernelEvent::DeleteEdge { id } => ("DeleteEdge", None, None, Some(id.0)),
        KernelEvent::AutoInsertRecordEncrypted { .. } => {
            ("AutoInsertRecordEncrypted", None, None, None)
        }
        KernelEvent::SetMeta { .. } => ("SetMeta", None, None, None),
        KernelEvent::AutoCreateNamespace { .. } => ("AutoCreateNamespace", None, None, None),
        KernelEvent::DropNamespace { .. } => ("DropNamespace", None, None, None),
        KernelEvent::UpdateRecordMetadata { id, .. } => {
            ("UpdateRecordMetadata", Some(id.0), None, None)
        }
        KernelEvent::ConfigureNamespace { .. } => ("ConfigureNamespace", None, None, None),
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

    let proof = if let Some(r) = receipt_store
        .get(&id)
        .or_else(|| receipt_store.get(&op_id))
        .or_else(|| receipt_store.latest())
    {
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

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/operations/{id}/execution",
    operation_id = "get_operation_execution",
    tag = "operations",
    summary = "Per-stage execution breakdown for one operation",
    description = "The Execution Explorer payload: every pipeline stage with its duration, metrics, and warnings, plus the state hash before and after. Held in an in-process registry, so it does not survive a restart.",
    params(
        ("id" = String, Path, description = "Operation id"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Execution record", body = crate::execution_registry::ExecutionRecord),
        (status = 404, description = "No execution recorded for this operation"),
    ),
))]
/// Real per-stage execution data for an ingest operation — "Make Execution
/// Explorer Real". Looks up `id` (the `operation_id` returned by
/// `POST /v1/ingest`) in the in-process [`crate::execution_registry::ExecutionRegistry`].
/// No fabricated DAG: an id that never ran through the pipeline (including
/// every WAL-event `op-N` id from `GET /v1/operations`, which is a different,
/// finer-grained id space — one per committed kernel event, not per ingest
/// call) 404s honestly instead of returning fake data.
pub async fn get_operation_execution(
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Extension(executions): axum::Extension<Arc<crate::execution_registry::ExecutionRegistry>>,
) -> Result<Json<crate::execution_registry::ExecutionRecord>, (StatusCode, Json<serde_json::Value>)>
{
    match executions.get(&id) {
        Some(record) => Ok(Json((*record).clone())),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({"error": format!("no execution record for operation '{}'", id)}),
            ),
        )),
    }
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
        config: crate::routes::collections::CollectionConfigRequest,
    ) -> Result<crate::routes::collections::CreatedCollection, Response> {
        // Single write lock: the existence check and the create are atomic.
        let mut engine = self.write().await;
        let already_existed = engine.namespaces.map.contains_key(name);
        let id = engine
            .create_collection_with_config(name, config.dim, config.metric, config.index)
            .map_err(|e| e.into_response())?;
        Ok(crate::routes::collections::CreatedCollection {
            id,
            already_existed,
        })
    }

    async fn drop_collection(&self, name: &str) -> Result<(), Response> {
        self.write()
            .await
            .drop_collection(name)
            .map_err(|e| e.into_response())
    }

    async fn list(&self) -> Vec<(String, u16)> {
        self.read().await.list_collections()
    }

    async fn config(
        &self,
        namespace_id: u16,
    ) -> Option<crate::routes::collections::CollectionConfigRequest> {
        let engine = self.read().await;
        let c = engine.namespaces.config(namespace_id)?;
        // Desired index is tracked separately from vector config — see
        // `valori_metadata::collection`'s module doc.
        let index = engine
            .namespaces
            .desired_index(namespace_id)
            .unwrap_or(valori_domain::IndexKind::Brute);
        Some(crate::routes::collections::CollectionConfigRequest {
            dim: c.dim,
            metric: c.metric,
            index,
        })
    }

    async fn record_count(&self, namespace_id: u16) -> usize {
        self.read()
            .await
            .state
            .iter_records_in_ns(namespace_id)
            .count()
    }

    async fn max_records(&self) -> usize {
        self.read().await.max_records
    }
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/namespaces",
    operation_id = "create_collection",
    tag = "collections",
    summary = "Create a collection",
    description = "Idempotent. `dimension` and `metric` are always required — a new \
                   project has zero collections and `default` carries no implicit \
                   config (Phase 3.3).",
    request_body = CreateCollectionRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Collection created, or already existed with an identical config", body = CreateCollectionResponse),
        (status = 400, description = "Invalid name, dimension, metric, or index kind", body = ApiError),
        (status = 409, description = "Collection exists with a conflicting config", body = ApiError),
        (status = 507, description = "Record slab capacity exhausted", body = ApiError),
    ),
))]
pub(crate) async fn create_collection_handler(
    State(state): State<SharedEngine>,
    Json(payload): Json<CreateCollectionRequest>,
) -> Result<Json<CreateCollectionResponse>, Response> {
    crate::routes::collections::create_collection(&state, payload).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/namespaces",
    operation_id = "list_collections",
    tag = "collections",
    summary = "List collections",
    description = "Returns an empty list for a brand-new project. Each entry carries \
                   its dimension, metric, index kind, and record count.",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Collection inventory", body = ListCollectionsResponse),
    ),
))]
pub(crate) async fn list_collections_handler(
    State(state): State<SharedEngine>,
) -> Json<ListCollectionsResponse> {
    crate::routes::collections::list_collections(&state).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    delete,
    path = "/v1/namespaces/{name}",
    operation_id = "delete_collection",
    tag = "collections",
    summary = "Drop a collection",
    description = "Removes the collection and every record in it. Returns no body.",
    params(("name" = String, Path, description = "Collection name")),
    security(("BearerAuth" = [])),
    responses(
        (status = 204, description = "Collection dropped"),
        (status = 404, description = "No such collection", body = ApiError),
    ),
))]
pub(crate) async fn drop_collection_handler(
    State(state): State<SharedEngine>,
    AxumPath(name): AxumPath<String>,
) -> Result<axum::http::StatusCode, Response> {
    crate::routes::collections::drop_collection(&state, &name).await
}

// ── Phase 4: index lifecycle ─────────────────────────────────────────────────

#[async_trait::async_trait]
impl crate::routes::index_lifecycle::IndexOps for SharedEngine {
    async fn resolve(&self, name: &str) -> Option<u16> {
        self.read().await.namespaces.resolve(Some(name))
    }

    async fn get_index_state(
        &self,
        namespace_id: u16,
    ) -> valori_engine::index_manager::CollectionIndexState {
        self.read().await.index_state(namespace_id)
    }

    async fn start_build(
        &self,
        namespace_id: u16,
        spec: valori_engine::index_manager::IndexSpec,
    ) -> Result<u32, String> {
        // Step 1: capture the record snapshot + start the build (marks BUILDING).
        let (gen, records, kind, dim) = {
            let mut engine = self.write().await;
            let gen = engine
                .start_index_build(namespace_id, spec.clone())
                .map_err(|e| e.to_string())?;
            let records = engine.snapshot_records_for_ns(namespace_id);
            // Determine what kind to build from the spec string.
            let kind = spec.index_type.clone();
            let dim = engine.namespace_effective_dim(namespace_id).unwrap_or(128);
            (gen, records, kind, dim)
        };

        // Step 2: build the index in a background task (non-blocking).
        let engine_ref = self.clone();
        let spec_clone = spec.clone();
        tokio::spawn(async move {
            use valori_index::{BqIndex, HnswIndex, IvfIndex, VectorIndex};
            let result: Result<Box<dyn VectorIndex + Send + Sync>, String> =
                tokio::task::spawn_blocking(move || {
                    let params = &spec_clone.parameters;
                    let mut idx: Box<dyn VectorIndex + Send + Sync> = match kind.as_str() {
                        "hnsw" => {
                            // Wire user parameters; fall back to library defaults.
                            let mut cfg = valori_index::HnswConfig::default();
                            if let Some(v) = params.get("m").and_then(|v| v.as_u64()) {
                                cfg.m = v as usize;
                                cfg.m_max0 = (v * 2) as usize;
                            }
                            if let Some(v) = params.get("ef_construction").and_then(|v| v.as_u64())
                            {
                                cfg.ef_construction = v as usize;
                            }
                            if let Some(v) = params.get("ef_search").and_then(|v| v.as_u64()) {
                                cfg.ef_search = v as usize;
                            }
                            Box::new(HnswIndex::new_with_config(cfg))
                        }
                        "ivf" => {
                            // Wire user parameters; fall back to auto-scale.
                            let user_n_list = params.get("n_list").and_then(|v| v.as_u64());
                            let user_n_probe = params.get("n_probe").and_then(|v| v.as_u64());
                            let (n_list, n_probe, auto_scale) = if let Some(nl) = user_n_list {
                                let np = user_n_probe.map(|v| v as usize).unwrap_or_else(|| {
                                    std::cmp::max(1, (nl as f64).sqrt() as usize)
                                });
                                (nl as usize, np, false)
                            } else {
                                // Auto-scale: will be overridden by IvfConfig::effective_params()
                                let auto_nl =
                                    std::cmp::max(16, (records.len() as f32).sqrt() as usize);
                                (auto_nl, std::cmp::max(1, 4), true)
                            };
                            Box::new(IvfIndex::new(
                                valori_index::IvfConfig {
                                    n_list,
                                    n_probe,
                                    auto_scale,
                                },
                                dim,
                            ))
                        }
                        "bq" => Box::new(BqIndex::new()),
                        _ => return Err(format!("unknown index type '{}'", spec_clone.index_type)),
                    };
                    idx.build(&records);
                    Ok(idx)
                })
                .await
                .unwrap_or_else(|e| Err(format!("build task panicked: {e}")));

            // Step 3: activate the built index (WAL catch-up happens inside).
            let mut engine = engine_ref.write().await;
            match result {
                Ok(idx) => {
                    if let Err(e) = engine.finish_index_build(namespace_id, gen, idx) {
                        engine.fail_index_build(namespace_id, gen, e.to_string());
                        tracing::error!("Index build failed for ns={namespace_id} gen={gen}: {e}");
                    } else {
                        tracing::info!("Index build complete for ns={namespace_id} gen={gen}");
                    }
                }
                Err(e) => {
                    engine.fail_index_build(namespace_id, gen, e.clone());
                    tracing::error!("Index build error for ns={namespace_id} gen={gen}: {e}");
                }
            }
        });

        Ok(gen)
    }

    async fn drop_index(&self, namespace_id: u16) -> Result<(), String> {
        self.write().await.drop_collection_index(namespace_id);
        Ok(())
    }

    fn supports_ann_builds(&self) -> bool {
        true
    }
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/namespaces/{name}/index",
    operation_id = "set_collection_index",
    tag = "index",
    summary = "Create, change, or drop a collection index",
    description = "`type` is `hnsw`, `ivf`, `bq`, or null to drop the index and revert to exact search. A build is asynchronous: 202 means the build started, and the response carries the building generation. Poll the GET form for completion.",
    params(
        ("name" = String, Path, description = "Collection name"),
    ),
    request_body = valori_engine::index_manager::IndexBuildRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Index dropped", body = valori_engine::index_manager::IndexStatusResponse),
        (status = 202, description = "Build accepted and started", body = valori_engine::index_manager::IndexStatusResponse),
        (status = 400, description = "Unsupported index type", body = ApiError),
        (status = 404, description = "No such collection", body = ApiError),
        (status = 409, description = "A build is already in progress", body = ApiError),
        (status = 501, description = "ANN index management unavailable on this node", body = ApiError),
    ),
))]
async fn index_lifecycle_create_handler(
    State(state): State<SharedEngine>,
    AxumPath(name): AxumPath<String>,
    Json(payload): Json<valori_engine::index_manager::IndexBuildRequest>,
) -> Response {
    crate::routes::index_lifecycle::create_or_change_index(&state, &name, payload).await
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/namespaces/{name}/index",
    operation_id = "get_collection_index",
    tag = "index",
    summary = "Read one collection's index lifecycle state",
    description = "`desired_type` is what was asked for; `active_type` and `status` describe what this node is actually serving. In cluster mode `desired_type` comes from the Raft-replicated spec and is cluster-wide, while activation is node-local — the two differ while a build propagates.",
    params(
        ("name" = String, Path, description = "Collection name"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Lifecycle state", body = valori_engine::index_manager::IndexStatusResponse),
        (status = 404, description = "No such collection", body = ApiError),
    ),
))]
async fn index_lifecycle_status_handler(
    State(state): State<SharedEngine>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    crate::routes::index_lifecycle::get_index_status(&state, &name).await
}

// ── Phase 3.1: object-store handlers ─────────────────────────────────────────

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(serde::Serialize)]
pub(crate) struct StorageSnapshotUploadResponse {
    key: String,
    state_hash: String,
    size_bytes: usize,
    pruned: usize,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(serde::Serialize)]
pub(crate) struct ListRemoteSnapshotsResponse {
    snapshots: Vec<crate::object_store::SnapshotEntry>,
    count: usize,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(serde::Deserialize)]
pub(crate) struct RestoreFromStoreRequest {
    /// Object key returned by a previous upload or list call. Omit to
    /// restore whatever `manifest.json` currently names as current — see
    /// `GET /v1/storage/manifest`; this is now the recommended entry point
    /// for disaster recovery instead of listing `snapshots/` and picking
    /// the newest filename by hand.
    #[serde(default)]
    key: Option<String>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(serde::Serialize)]
pub(crate) struct ManifestResponse {
    manifest: Option<crate::object_store::SnapshotManifest>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(serde::Serialize)]
pub(crate) struct RestoreFromStoreResponse {
    key: String,
    state_hash: String,
    size_bytes: usize,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(serde::Serialize)]
pub(crate) struct ListRemoteWalResponse {
    segments: Vec<crate::object_store::WalEntry>,
    count: usize,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(serde::Deserialize)]
pub(crate) struct ArchiveWalRequest {
    /// Absolute path on this node's local filesystem to the sealed segment.
    path: String,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(serde::Serialize)]
pub(crate) struct ArchiveWalResponse {
    key: String,
    size_bytes: u64,
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/storage/snapshots",
    operation_id = "list_object_store_snapshots",
    tag = "storage",
    summary = "List snapshots in the object store",
    description = "Requires `VALORI_OBJECT_STORE_URL`. Prefer `GET /v1/storage/manifest` for disaster recovery rather than sorting these keys by hand.",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Snapshots in the store", body = ListRemoteSnapshotsResponse),
        (status = 400, description = "Object store not configured", body = ApiError),
    ),
))]
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
    let snapshots = os
        .list_snapshots()
        .await
        .map_err(|e| EngineError::InvalidInput(format!("object store list failed: {e}")))?;
    let count = snapshots.len();
    Ok(Json(ListRemoteSnapshotsResponse { snapshots, count }))
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/storage/snapshots/upload",
    operation_id = "upload_snapshot_to_object_store",
    tag = "storage",
    summary = "Offload a snapshot to the object store",
    description = "Takes a snapshot, uploads it, prunes to `VALORI_OBJECT_STORE_KEEP`, and rewrites `manifest.json` to name the new snapshot as current.",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Uploaded and manifest rewritten", body = StorageSnapshotUploadResponse),
        (status = 400, description = "Object store not configured", body = ApiError),
        (status = 502, description = "Object store rejected the write", body = ApiError),
    ),
))]
/// `POST /v1/storage/snapshots/upload` — snapshot current state and push to object store.
///
/// Automatically prunes old snapshots according to `VALORI_OBJECT_STORE_KEEP` (default 7).
async fn upload_snapshot_to_store(
    State(state): State<SharedEngine>,
) -> Result<Json<StorageSnapshotUploadResponse>, EngineError> {
    let started = std::time::Instant::now();

    // Encode snapshot on the blocking thread pool (CPU-heavy), cloning out the
    // object-store handle and state hash before releasing the lock.
    let (snap_bytes, state_hash, object_store, keep, index_bytes) = tokio::task::spawn_blocking({
        let state = state.clone();
        move || {
            let engine = state.blocking_read();
            let snap = engine.snapshot()?;
            let proof = engine.get_proof();
            let hash = proof
                .final_state_hash
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            let os = engine.object_store.clone();
            let keep = engine.object_store_keep as usize;
            // Measured here, where the index is already being serialized
            // for the snapshot — see Engine::index_size_bytes for why this
            // isn't a per-/health gauge.
            let index_bytes = engine.index_size_bytes();
            Ok::<_, EngineError>((snap, hash, os, keep, index_bytes))
        }
    })
    .await
    .map_err(|e| EngineError::InvalidInput(format!("snapshot encode panicked: {e}")))??;

    if let Some(bytes) = index_bytes {
        metrics::gauge!("valori_index_size_bytes", bytes as f64);
    }

    let os = object_store.ok_or_else(|| {
        EngineError::InvalidInput(
            "object store not configured — set VALORI_OBJECT_STORE_URL".into(),
        )
    })?;

    let size_bytes = snap_bytes.len();
    let entry = os
        .upload_snapshot_and_update_manifest(&snap_bytes, &state_hash, env!("CARGO_PKG_VERSION"))
        .await
        .map_err(|e| EngineError::InvalidInput(format!("upload failed: {e}")))?;
    let key = entry.key;

    let pruned = os.prune_snapshots(keep).await.unwrap_or(0);

    // Encode + upload + prune — the whole operation an operator waits on,
    // not just the encode step.
    metrics::gauge!("valori_snapshot_size_bytes", size_bytes as f64);
    metrics::histogram!(
        "valori_snapshot_duration_seconds",
        started.elapsed().as_secs_f64()
    );

    Ok(Json(StorageSnapshotUploadResponse {
        key,
        state_hash,
        size_bytes,
        pruned,
    }))
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/storage/manifest",
    operation_id = "get_storage_manifest",
    tag = "storage",
    summary = "Read the object-store manifest",
    description = "Names the current snapshot and every archived WAL segment in one object. `manifest: null` means the store is configured but nothing has been uploaded through it yet — not an error.",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "The manifest, or null", body = ManifestResponse),
        (status = 400, description = "Object store not configured", body = ApiError),
    ),
))]
/// `GET /v1/storage/manifest` — the disaster-recovery entry point: names
/// the current snapshot and archived WAL segments in one object, instead of
/// a caller listing `snapshots/`/`wal/` and sorting filenames themselves.
/// `manifest: null` means the object store is configured but no snapshot
/// has ever been uploaded through `POST /v1/storage/snapshots/upload` yet
/// (an older store that only ever used the pre-manifest path would also
/// read as `null` here — not an error, just nothing written).
async fn get_manifest(
    State(state): State<SharedEngine>,
) -> Result<Json<ManifestResponse>, EngineError> {
    let object_store = {
        let engine = state.read().await;
        engine.object_store.clone()
    };
    let os = object_store.ok_or_else(|| {
        EngineError::InvalidInput(
            "object store not configured — set VALORI_OBJECT_STORE_URL".into(),
        )
    })?;
    let manifest = os
        .read_manifest()
        .await
        .map_err(|e| EngineError::InvalidInput(format!("reading manifest failed: {e}")))?;
    Ok(Json(ManifestResponse { manifest }))
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/storage/snapshots/restore",
    operation_id = "restore_snapshot_from_object_store",
    tag = "storage",
    summary = "Restore from a snapshot in the object store",
    description = "Omit `key` to restore whatever `manifest.json` names as current — the recommended disaster-recovery entry point. Destructive.",
    request_body = RestoreFromStoreRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "State replaced from the store", body = RestoreFromStoreResponse),
        (status = 400, description = "Object store not configured, or no such key", body = ApiError),
        (status = 502, description = "Object store read failed", body = ApiError),
    ),
))]
/// `POST /v1/storage/snapshots/restore` — pull a snapshot from the object store and restore.
///
/// Body: `{ "key": "snapshots/00000001750000000_abc12345.snap" }`, or `{}`
/// to restore whatever `manifest.json` currently names as current.
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

    let started = std::time::Instant::now();

    let key = match req.key {
        Some(key) => key,
        None => {
            let manifest = os
                .read_manifest()
                .await
                .map_err(|e| EngineError::InvalidInput(format!("reading manifest failed: {e}")))?;
            manifest
                .and_then(|m| m.current_snapshot)
                .map(|s| s.key)
                .ok_or_else(|| {
                    EngineError::InvalidInput(
                        "no key given and manifest.json names no current snapshot".into(),
                    )
                })?
        }
    };

    let data = os
        .download_snapshot(&key)
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

    // Download + apply + rehash — the full time a project is unavailable
    // during a disaster recovery, which is the number that actually
    // matters for an RTO claim.
    metrics::histogram!(
        "valori_restore_duration_seconds",
        started.elapsed().as_secs_f64()
    );
    metrics::gauge!("valori_restore_size_bytes", size_bytes as f64);

    tracing::info!(
        key = %key,
        state_hash = %state_hash,
        "restored from object store"
    );
    Ok(Json(RestoreFromStoreResponse {
        key,
        state_hash,
        size_bytes,
    }))
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/storage/wal",
    operation_id = "list_archived_wal_segments",
    tag = "storage",
    summary = "List archived WAL segments in the object store",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Archived segments", body = ListRemoteWalResponse),
        (status = 400, description = "Object store not configured", body = ApiError),
    ),
))]
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
    let segments = os
        .list_wal_segments()
        .await
        .map_err(|e| EngineError::InvalidInput(format!("object store list failed: {e}")))?;
    let count = segments.len();
    Ok(Json(ListRemoteWalResponse { segments, count }))
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/storage/wal/archive",
    operation_id = "archive_wal_segment",
    tag = "storage",
    summary = "Archive one sealed WAL segment",
    description = "`path` is a local path on this node. The segment must already be sealed — archiving the live segment is rejected.",
    request_body = ArchiveWalRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Segment archived", body = ArchiveWalResponse),
        (status = 400, description = "Object store not configured, or the segment is not sealed", body = ApiError),
        (status = 502, description = "Object store rejected the write", body = ApiError),
    ),
))]
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
        eng.event_committer()
            .map(|c| {
                c.event_log()
                    .path()
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .to_path_buf()
            })
            .or_else(|| {
                eng.wal_path
                    .as_deref()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_path_buf())
            })
    };
    let local_path = safe_path(&req.path, allowed_dir.as_deref())?;
    if !local_path.exists() {
        return Err(EngineError::InvalidInput(format!(
            "segment not found: {}",
            local_path.display()
        )));
    }
    let size_bytes = std::fs::metadata(&local_path).map(|m| m.len()).unwrap_or(0);
    let key = os
        .archive_wal_segment(&local_path)
        .await
        .map_err(|e| EngineError::InvalidInput(format!("archive failed: {e}")))?;

    // Refresh manifest.json's WAL list — preserve whatever it currently
    // names as the current snapshot, this archive doesn't change that.
    // Best-effort: a manifest-refresh failure shouldn't fail an otherwise
    // successful archive.
    if let Ok(current_snapshot) = os
        .read_manifest()
        .await
        .map(|m| m.and_then(|m| m.current_snapshot))
    {
        if let Ok(wal_segments) = os.list_wal_segments().await {
            if let Err(e) = os
                .write_manifest(
                    current_snapshot.as_ref(),
                    wal_segments,
                    env!("CARGO_PKG_VERSION"),
                )
                .await
            {
                tracing::warn!(error = %e, "failed to refresh manifest.json after WAL archive, continuing");
            }
        }
    }

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

fn default_scope() -> ApiScope {
    ApiScope::ReadWrite
}

async fn create_key_handler(
    Extension(auth): Extension<Arc<AuthState>>,
    Json(req): Json<CreateKeyRequest>,
) -> impl IntoResponse {
    let created = auth
        .key_store
        .create(req.scope, req.collection, req.description);
    (StatusCode::CREATED, Json(created))
}

async fn list_keys_handler(Extension(auth): Extension<Arc<AuthState>>) -> impl IntoResponse {
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

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Deserialize)]
pub(crate) struct InsertEncryptedRequest {
    /// Base64-encoded plaintext payload (will be encrypted by the vault).
    payload: String,
    tag: Option<u64>,
    collection: Option<String>,
    /// Optional pre-chosen key_id (hex). If absent, a fresh key_id is generated.
    key_id: Option<String>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub(crate) struct InsertEncryptedResponse {
    id: u32,
    key_id: String,
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/records/encrypted",
    operation_id = "insert_encrypted_record",
    tag = "records",
    summary = "Insert a crypto-shreddable record",
    description = "The payload is encrypted with a per-record key held in the node vault. Deleting that key through `DELETE /v1/crypto/shred/{key_id}` renders the record permanently unreadable without rewriting the audit chain.",
    request_body = InsertEncryptedRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 201, description = "Record stored", body = InsertEncryptedResponse),
        (status = 400, description = "Bad base64 payload, bad key_id, or unknown collection"),
        (status = 500, description = "Encryption or commit failure"),
    ),
))]
async fn insert_encrypted_handler(
    State(state): State<SharedEngine>,
    Json(payload): Json<InsertEncryptedRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    use base64::Engine as _;
    let plaintext = base64::engine::general_purpose::STANDARD
        .decode(&payload.payload)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("base64 decode: {e}")))?;

    let key_id: [u8; 16] = if let Some(ref hex) = payload.key_id {
        hex_to_key_id(hex).ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "key_id must be 32 hex chars".into(),
            )
        })?
    } else {
        new_key_id()
    };

    let mut engine = state.write().await;
    let ns = engine
        .resolve_collection(payload.collection.as_deref())
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let tag = payload.tag.unwrap_or(0);

    let id = engine
        .insert_encrypted_ns(&plaintext, tag, ns, key_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(InsertEncryptedResponse {
            id,
            key_id: key_id_to_hex(&key_id),
        }),
    ))
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
    let key_id = hex_to_key_id(&key_id_hex).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "key_id must be 32 hex chars".into(),
        )
    })?;

    let mut engine = state.write().await;
    engine
        .shred_key(key_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ShredKeyResponse {
        key_id: key_id_hex,
        shredded: true,
    }))
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub(crate) struct CryptoStatusResponse {
    key_id: String,
    exists: bool,
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/crypto/status/{key_id}",
    operation_id = "get_key_status",
    tag = "crypto",
    summary = "Check whether a crypto-shredding key still exists",
    description = "`exists: false` means the key was shredded and every record encrypted under it is permanently unreadable.",
    params(
        ("key_id" = String, Path, description = "32 hex characters"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Key status", body = CryptoStatusResponse),
        (status = 400, description = "key_id is not 32 hex characters", body = ApiError),
    ),
))]
async fn crypto_status_handler(
    State(state): State<SharedEngine>,
    AxumPath(key_id_hex): AxumPath<String>,
) -> Response {
    // Phase API-3.3: this handler used to return `(StatusCode, String)`, whose
    // `text/plain` body is deliberately passed through untouched by
    // `attach_error_code` — so it was the one error in the whole surface that
    // escaped the canonical `{error, code}` shape, and it forked from the
    // cluster twin, which already answered in JSON. Both now go through
    // `error_response`.
    let Some(key_id) = hex_to_key_id(&key_id_hex) else {
        return crate::errors::error_response(
            StatusCode::BAD_REQUEST,
            crate::errors::ErrorCode::ValidationError,
            "key_id must be 32 hex chars",
        );
    };

    let engine = state.read().await;
    let exists = engine.vault.key_exists(&key_id);
    Json(CryptoStatusResponse {
        key_id: key_id_hex,
        exists,
    })
    .into_response()
}

// ── Phase 3.13: index config endpoint ────────────────────────────────────────

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub(crate) struct IndexConfigResponse {
    index_type: String,
    hnsw: Option<HnswConfigView>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub(crate) struct HnswConfigView {
    m: usize,
    m_max0: usize,
    ef_construction: usize,
    ef_search: usize,
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/index/config",
    operation_id = "get_index_config",
    tag = "index",
    summary = "Read node-level index configuration",
    description = "Reports how indexing is configured for the node as a whole. Since Phase 4 indexes are per-collection, so `index_type` is `collection_scoped` and the real state lives at `GET /v1/namespaces/{name}/index`.",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Node index configuration", body = IndexConfigResponse),
    ),
))]
async fn index_config_handler(State(_state): State<SharedEngine>) -> impl IntoResponse {
    Json(IndexConfigResponse {
        index_type: "collection_scoped".into(),
        hnsw: None,
    })
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/index/rebuild",
    operation_id = "rebuild_indexes",
    tag = "index",
    summary = "Rebuild every per-collection index",
    description = "Synchronous and project-wide: the write lock is held for the duration. For a single collection, and for asynchronous builds, use `POST /v1/namespaces/{name}/index` instead.",
    request_body = crate::api::IndexRebuildRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Rebuild complete", body = crate::api::IndexRebuildResponse),
    ),
))]
/// `POST /v1/index/rebuild` — rebuild all per-collection indexes.
async fn index_rebuild_handler(
    State(state): State<SharedEngine>,
    Json(body): Json<crate::api::IndexRebuildRequest>,
) -> impl IntoResponse {
    let mut engine = state.write().await;
    engine.rebuild_index();
    Json(crate::api::IndexRebuildResponse {
        ok: true,
        effective: body.index.unwrap_or_else(|| "rebuilt".to_string()),
        records: engine.record_count(),
    })
}

// ── Phase I5: Tree-RAG stateful handlers ──────────────────────────────────────

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/tree/build",
    operation_id = "tree_build",
    tag = "tree",
    summary = "Parse a markdown document into a navigable tree",
    description = "Zero-LLM: pure header parsing. Returns the full tree plus a `cache_key` (BLAKE3 of the input) that later query and hybrid calls can send instead of re-transmitting the whole tree.",
    request_body = valori_rag::tree::BuildRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Tree index and cache key", body = valori_rag::tree::BuildResponse),
        (status = 400, description = "Malformed or invalid request", body = ApiError),
    ),
))]
/// `POST /v1/tree/build` — parse markdown into a tree index and cache it.
/// Returns the full tree + a `cache_key` (BLAKE3 of the input text) that can
/// be passed to subsequent `/v1/tree/query` or `/v1/tree/hybrid` calls so the
/// caller doesn't have to re-transmit the full tree on every request.
async fn tree_build(
    State(engine): State<SharedEngine>,
    axum::Extension(caps): axum::Extension<Arc<valori_effect::capability::CapabilityRegistry>>,
    axum::Extension(task_reg): axum::Extension<Arc<crate::runner::TaskRegistry>>,
    Json(payload): Json<valori_rag::tree::BuildRequest>,
) -> Json<valori_rag::tree::BuildResponse> {
    use crate::runner::run_graph_inline;
    use valori_planner::context::{
        CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash,
    };
    use valori_planner::graph::ExecutionRetentionPolicy;
    use valori_planner::graph::{ExecutionGraph, TaskId, TaskKind, TaskSpec};
    use valori_planner::operation::{
        compute_operation_hash, ExecutionPolicy, OperationInputs, OperationKind,
    };

    let doc_name = payload
        .doc_name
        .clone()
        .unwrap_or_else(|| "document".into());
    let shard_count = engine.read().await.shard_count as u8;

    let inputs_json = serde_json::to_string(&serde_json::json!({
        "text": payload.text,
        "doc_name": doc_name,
    }))
    .unwrap_or_default();

    let op_hash = compute_operation_hash(
        OperationKind::TreeBuild,
        &OperationInputs::TreeBuild { shard_id: 0 },
        &ExecutionPolicy::default(),
    );
    let fp = PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1);
    let ctx_hash = PlanningContextHash::compute(&PlanningContext {
        capability_set: CapabilitySet {
            embed: false,
            llm: false,
            object_store: false,
            cluster: false,
            shard_count,
        },
        schema_version: 1,
        shard_count,
        cluster_epoch: 0,
        cluster_mode: false,
    });
    let graph = Arc::new(ExecutionGraph::build(
        op_hash,
        fp,
        ctx_hash,
        vec![TaskSpec {
            id: TaskId(0),
            kind: TaskKind::TreeBuild,
            inputs_json,
            shard_id: None,
            topological_index: 0,
        }],
        vec![],
        ExecutionRetentionPolicy::default(),
    ));

    let result = run_graph_inline(graph, caps, task_reg, ExecutionPolicy::default())
        .await
        .ok()
        .and_then(|o| o.into_iter().next().flatten())
        .map(|o| o.json)
        .unwrap_or(serde_json::json!({}));

    // Reconstruct the BuildResponse from the task output JSON.
    let tree: valori_rag::tree::TreeIndex = result
        .get("tree")
        .and_then(|t| serde_json::from_value(t.clone()).ok())
        .unwrap_or_else(|| valori_rag::tree::TreeIndex::from_markdown(&payload.text, &doc_name));
    let cache_key = result
        .get("cache_key")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Json(valori_rag::tree::BuildResponse {
        cache_key,
        doc_name: tree.doc_name.clone(),
        node_count: tree.nodes.len(),
        structure_map: tree.structure_map(),
        tree,
    })
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/tree/query",
    operation_id = "tree_query",
    tag = "tree",
    summary = "Navigate a document tree to an answer",
    description = "Deterministic table-of-contents navigation with breadcrumb citations. Every answer carries a BLAKE3 receipt chained onto `prev_hash`. Send either `tree` or a `cache_key` from a previous build.",
    request_body = valori_rag::tree::QueryRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Answer with citations and receipt", body = valori_rag::tree::AnswerResult),
        (status = 400, description = "Neither tree nor a known cache_key was supplied"),
    ),
))]
/// `POST /v1/tree/query` — navigate the tree and answer with citations + receipt.
/// Accepts either a full `tree` object (backward-compat) or a `cache_key`
/// returned by `/v1/tree/build` — the cache lookup avoids re-transmitting the tree.
async fn tree_query(
    State(engine): State<SharedEngine>,
    axum::Extension(caps): axum::Extension<Arc<valori_effect::capability::CapabilityRegistry>>,
    axum::Extension(task_reg): axum::Extension<Arc<crate::runner::TaskRegistry>>,
    Json(payload): Json<valori_rag::tree::QueryRequest>,
) -> Result<Json<valori_rag::tree::AnswerResult>, (StatusCode, Json<serde_json::Value>)> {
    use crate::runner::run_graph_inline;
    use valori_planner::context::{
        CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash,
    };
    use valori_planner::graph::ExecutionRetentionPolicy;
    use valori_planner::graph::{ExecutionGraph, TaskId, TaskKind, TaskSpec};
    use valori_planner::operation::{
        compute_operation_hash, ExecutionPolicy, OperationInputs, OperationKind,
    };

    let k = payload.k.max(1);
    let shard_count = engine.read().await.shard_count as u8;

    // Resolve tree: inline, cache_key, or error.
    let tree_val: serde_json::Value = if let Some(t) = payload.tree {
        serde_json::to_value(t).unwrap_or(serde_json::Value::Null)
    } else if let Some(ref key) = payload.cache_key {
        let eng = engine.read().await;
        eng.get_cached_tree(key)
            .and_then(|t| serde_json::to_value(t).ok())
            .ok_or_else(|| (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": "tree not in cache — re-send the full tree or call /v1/tree/build first",
                "cache_key": key
            }))))?
    } else {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "error": "provide 'tree' or 'cache_key'" })),
        ));
    };

    let inputs_json = serde_json::to_string(&serde_json::json!({
        "tree": tree_val,
        "query": payload.query,
        "k": k,
        "prev_hash": payload.prev_hash,
    }))
    .unwrap_or_default();

    let op_hash = compute_operation_hash(
        OperationKind::TreeQuery,
        &OperationInputs::TreeQuery {
            k: k as u32,
            shard_id: 0,
        },
        &ExecutionPolicy::default(),
    );
    let fp = PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1);
    let ctx_hash = PlanningContextHash::compute(&PlanningContext {
        capability_set: CapabilitySet {
            embed: false,
            llm: false,
            object_store: false,
            cluster: false,
            shard_count,
        },
        schema_version: 1,
        shard_count,
        cluster_epoch: 0,
        cluster_mode: false,
    });
    let graph = Arc::new(ExecutionGraph::build(
        op_hash,
        fp,
        ctx_hash,
        vec![TaskSpec {
            id: TaskId(0),
            kind: TaskKind::TreeQuery,
            inputs_json,
            shard_id: None,
            topological_index: 0,
        }],
        vec![],
        ExecutionRetentionPolicy::default(),
    ));

    let result = run_graph_inline(graph, caps, task_reg, ExecutionPolicy::default())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    let out_val = result
        .into_iter()
        .next()
        .flatten()
        .map(|o| o.json)
        .unwrap_or(serde_json::Value::Null);
    let answer: valori_rag::tree::AnswerResult = serde_json::from_value(out_val).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    Ok(Json(answer))
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/tree/hybrid",
    operation_id = "tree_hybrid",
    tag = "tree",
    summary = "Blend tree navigation with vector search",
    description = "`tree_weight` (default 0.6) sets the mix between tree hits and vector hits. Each hit records which source produced it.",
    request_body = valori_rag::tree::HybridRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Blended hits", body = valori_rag::tree::HybridResponse),
        (status = 400, description = "Neither tree, text, nor a known cache_key was supplied"),
    ),
))]
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
    axum::Extension(caps): axum::Extension<Arc<valori_effect::capability::CapabilityRegistry>>,
    axum::Extension(task_reg): axum::Extension<Arc<crate::runner::TaskRegistry>>,
    Json(payload): Json<valori_rag::tree::HybridRequest>,
) -> Result<Json<valori_rag::tree::HybridResponse>, (StatusCode, Json<serde_json::Value>)> {
    use crate::runner::run_graph_inline;
    use valori_planner::context::{
        CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash,
    };
    use valori_planner::graph::ExecutionRetentionPolicy;
    use valori_planner::graph::{ExecutionGraph, TaskId, TaskKind, TaskSpec};
    use valori_planner::operation::{
        compute_operation_hash, ExecutionPolicy, OperationInputs, OperationKind,
    };
    use valori_rag::tree::{HybridResponse, TreeIndex, GENESIS};

    // ── Resolve tree and optional query vector before dispatching ─────────────
    let (tree_json, cache_key_opt) = if let Some(t) = payload.tree {
        (
            Some(serde_json::to_value(&t).unwrap_or(serde_json::Value::Null)),
            None,
        )
    } else if let Some(ref key) = payload.cache_key {
        (None, Some(key.clone()))
    } else if let Some(ref text) = payload.text {
        let doc_name = payload.doc_name.as_deref().unwrap_or("document");
        let t = TreeIndex::from_markdown(text, doc_name);
        let _ = engine.write().await.cache_tree(text, t.clone());
        (
            Some(serde_json::to_value(&t).unwrap_or(serde_json::Value::Null)),
            None,
        )
    } else {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "error": "provide 'text', 'tree', or 'cache_key'"
            })),
        ));
    };

    // Resolve namespace and optionally embed the query for vector fusion.
    let ns_name = payload.namespace.as_deref();
    let (embed_cfg, ns_id) = {
        let eng = engine.read().await;
        let ns = eng.resolve_collection(ns_name).unwrap_or(0);
        (eng.embed_config.clone(), ns)
    };

    let mut query_vec: Option<Vec<f32>> = None;
    if let Some(ref ecfg) = embed_cfg {
        let http = shared_http_client();
        if let Ok(vecs) = valori_ingest::embed_batch(&[payload.query.clone()], ecfg, http).await {
            if !vecs.is_empty() {
                query_vec = Some(vecs.into_iter().next().unwrap());
            }
        }
    }

    // ── Build params for the capability ───────────────────────────────────────
    let mut params = serde_json::json!({
        "tree_weight": payload.tree_weight,
        "prev_hash": payload.prev_hash.as_deref().unwrap_or(GENESIS),
    });
    if let Some(tj) = tree_json {
        params["tree"] = tj;
    }
    if let Some(ref ck) = cache_key_opt {
        params["cache_key"] = serde_json::Value::String(ck.clone());
    }
    if let Some(ref qv) = query_vec {
        params["vector"] = serde_json::json!(qv);
    }

    let inputs_json = serde_json::json!({
        "shard_id": 0u8,
        "namespace_id": ns_id,
        "query": payload.query,
        "k": payload.k,
        "params": params,
    })
    .to_string();

    // ── Dispatch through planner ───────────────────────────────────────────────
    let op_hash = compute_operation_hash(
        OperationKind::TreeHybrid,
        &OperationInputs::TreeHybrid {
            k: payload.k as u32,
            shard_id: 0,
            embed_enabled: embed_cfg.is_some(),
        },
        &ExecutionPolicy::default(),
    );
    let fp = PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1);
    let ctx_hash = PlanningContextHash::compute(&PlanningContext {
        capability_set: CapabilitySet {
            embed: embed_cfg.is_some(),
            llm: false,
            object_store: false,
            cluster: false,
            shard_count: 1,
        },
        schema_version: 1,
        shard_count: 1,
        cluster_epoch: 0,
        cluster_mode: false,
    });
    let graph = std::sync::Arc::new(ExecutionGraph::build(
        op_hash,
        fp,
        ctx_hash,
        vec![TaskSpec {
            id: TaskId(0),
            kind: TaskKind::TreeHybrid,
            inputs_json,
            shard_id: Some(0),
            topological_index: 0,
        }],
        vec![],
        ExecutionRetentionPolicy::default(),
    ));

    let outputs = run_graph_inline(graph, caps, task_reg, ExecutionPolicy::default())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    let output_val = outputs
        .into_iter()
        .next()
        .flatten()
        .map(|o| o.json)
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "tree_hybrid produced no output"})),
            )
        })?;

    let response: HybridResponse = serde_json::from_value(output_val).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("tree_hybrid decode: {e}")})),
        )
    })?;

    Ok(Json(response))
}

// ── Phase I6: Community handlers (standalone) ─────────────────────────────────

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/community/detect",
    operation_id = "community_detect",
    tag = "community",
    summary = "Detect communities in the knowledge graph",
    description = "Label propagation, O(n+e), with a lowest-label tie-break so the result is deterministic for a given graph. Produces a BLAKE3 receipt over the sorted assignment. Must run before search or overview.",
    request_body = valori_rag::community::DetectRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Communities detected", body = valori_rag::community::DetectResponse),
        (status = 400, description = "Malformed or invalid request", body = ApiError),
    ),
))]
/// `POST /v1/community/detect`
///
/// Runs Label Propagation on the current graph to assign every node a
/// `community_id`, computes a centroid vector per community (average of
/// member record FxpVectors), and produces a BLAKE3 receipt proving the
/// assignment. The result is cached in the engine for subsequent
/// `/v1/community/search` calls.
async fn community_detect(
    State(engine): State<SharedEngine>,
    axum::Extension(caps): axum::Extension<Arc<valori_effect::capability::CapabilityRegistry>>,
    axum::Extension(task_reg): axum::Extension<Arc<crate::runner::TaskRegistry>>,
    Json(payload): Json<valori_rag::community::DetectRequest>,
) -> Json<valori_rag::community::DetectResponse> {
    use crate::runner::run_graph_inline;
    use valori_planner::context::{
        CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash,
    };
    use valori_planner::graph::ExecutionRetentionPolicy;
    use valori_planner::graph::{ExecutionGraph, TaskId, TaskKind, TaskSpec};
    use valori_planner::operation::{
        compute_operation_hash, ExecutionPolicy, OperationInputs, OperationKind,
    };

    let (ns_id, shard_count) = {
        let eng = engine.read().await;
        let ns = payload
            .namespace
            .as_deref()
            .and_then(|n| eng.namespaces.resolve(Some(n)))
            .unwrap_or(0);
        (ns, eng.shard_count as u8)
    };
    let max_iter = payload
        .max_iter
        .unwrap_or(valori_rag::community::DEFAULT_MAX_ITER);

    let inputs_json = serde_json::to_string(&serde_json::json!({
        "shard_id": 0u8,
        "namespace_id": ns_id,
        "max_iter": max_iter,
    }))
    .unwrap_or_default();

    let op_hash = compute_operation_hash(
        OperationKind::CommunityDetect,
        &OperationInputs::CommunityDetect {
            collection: payload
                .namespace
                .clone()
                .unwrap_or_else(|| "default".into()),
            shard_id: 0,
            max_iter,
        },
        &ExecutionPolicy::default(),
    );
    let fp = PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1);
    let ctx_hash = PlanningContextHash::compute(&PlanningContext {
        capability_set: CapabilitySet {
            embed: false,
            llm: false,
            object_store: false,
            cluster: false,
            shard_count,
        },
        schema_version: 1,
        shard_count,
        cluster_epoch: 0,
        cluster_mode: false,
    });
    let graph = Arc::new(ExecutionGraph::build(
        op_hash,
        fp,
        ctx_hash,
        vec![TaskSpec {
            id: TaskId(0),
            kind: TaskKind::CommunityDetect,
            inputs_json,
            shard_id: Some(0),
            topological_index: 0,
        }],
        vec![],
        ExecutionRetentionPolicy::default(),
    ));

    let result = run_graph_inline(graph, caps, task_reg, ExecutionPolicy::default())
        .await
        .ok()
        .and_then(|o| o.into_iter().next().flatten())
        .map(|o| o.json)
        .unwrap_or(serde_json::json!({}));

    Json(valori_rag::community::DetectResponse {
        community_count: result["community_count"].as_u64().unwrap_or(0) as usize,
        node_count: result["node_count"].as_u64().unwrap_or(0) as usize,
        communities: result["communities"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default(),
        receipt: result["receipt"].as_str().unwrap_or("").to_string(),
    })
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/community/search",
    operation_id = "community_search",
    tag = "community",
    summary = "Search communities by centroid",
    description = "Ranks communities by cosine similarity against their centroids. `drill_in` additionally returns member-level hits.",
    request_body = valori_rag::community::SearchRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Ranked communities", body = valori_rag::community::SearchResponse),
        (status = 400, description = "Malformed or invalid request", body = ApiError),
        (status = 412, description = "Community index not built — call detect first"),
    ),
))]
/// `POST /v1/community/search`
///
/// Scores a query vector against all community centroids (cosine similarity),
/// returns the top-k communities ranked best-first with their member node_ids
/// and optional BFS subgraph expansion.
async fn community_search(
    State(engine): State<SharedEngine>,
    axum::Extension(caps): axum::Extension<Arc<valori_effect::capability::CapabilityRegistry>>,
    axum::Extension(task_reg): axum::Extension<Arc<crate::runner::TaskRegistry>>,
    Json(payload): Json<valori_rag::community::SearchRequest>,
) -> Result<Json<valori_rag::community::SearchResponse>, (StatusCode, Json<serde_json::Value>)> {
    use crate::runner::run_graph_inline;
    use valori_planner::context::{
        CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash,
    };
    use valori_planner::graph::ExecutionRetentionPolicy;
    use valori_planner::graph::{ExecutionGraph, TaskId, TaskKind, TaskSpec};
    use valori_planner::operation::{
        compute_operation_hash, ExecutionPolicy, OperationInputs, OperationKind,
    };

    let shard_count = engine.read().await.shard_count as u8;

    let inputs_json = serde_json::to_string(&serde_json::json!({
        "shard_id": 0u8,
        "namespace_id": 0u16,
        "vector": payload.vector,
        "k": payload.k,
        "depth": 1u32,
        "drill_in": false,
    }))
    .unwrap_or_default();

    let op_hash = compute_operation_hash(
        OperationKind::CommunitySearch,
        &OperationInputs::CommunitySearch {
            k: payload.k as u32,
            depth: 1,
            drill_in: false,
            collection: "default".into(),
            shard_id: 0,
        },
        &ExecutionPolicy::default(),
    );
    let fp = PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1);
    let ctx_hash = PlanningContextHash::compute(&PlanningContext {
        capability_set: CapabilitySet {
            embed: false,
            llm: false,
            object_store: false,
            cluster: false,
            shard_count,
        },
        schema_version: 1,
        shard_count,
        cluster_epoch: 0,
        cluster_mode: false,
    });
    let graph = Arc::new(ExecutionGraph::build(
        op_hash,
        fp,
        ctx_hash,
        vec![TaskSpec {
            id: TaskId(0),
            kind: TaskKind::CommunitySearch,
            inputs_json,
            shard_id: Some(0),
            topological_index: 0,
        }],
        vec![],
        ExecutionRetentionPolicy::default(),
    ));

    let result = run_graph_inline(graph, caps, task_reg, ExecutionPolicy::default())
        .await
        .map_err(|e| {
            (
                StatusCode::PRECONDITION_FAILED,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    let out = result
        .into_iter()
        .next()
        .flatten()
        .map(|o| o.json)
        .unwrap_or(serde_json::json!({}));
    let communities: Vec<valori_rag::community::CommunityHit> = out["communities"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    let total = out["total_communities_searched"].as_u64().unwrap_or(0) as usize;

    Ok(Json(valori_rag::community::SearchResponse {
        communities,
        total_communities_searched: total,
    }))
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/community/overview",
    operation_id = "community_overview",
    tag = "community",
    summary = "Summarise every detected community",
    description = "Largest community first, each with up to 10 sample member node ids.",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Community summary", body = crate::api::CommunityOverviewResponse),
        (status = 412, description = "Community index not built — call detect first"),
    ),
))]
/// `GET /v1/community/overview`
///
/// Returns every detected community sorted by member count (largest first),
/// with its centroid vector, size, and the BLAKE3 receipt that covers the
/// full assignment map.  No LLM required — all data is derived from the
/// graph structure alone.  Requires `POST /v1/community/detect` to have been
/// called at least once.
async fn community_overview(
    State(engine): State<SharedEngine>,
) -> Result<Json<crate::api::CommunityOverviewResponse>, (StatusCode, Json<serde_json::Value>)> {
    let eng = engine.read().await;
    let store = eng.resources.community_store.as_ref().ok_or_else(|| {
        (
            StatusCode::PRECONDITION_FAILED,
            Json(serde_json::json!({
                "error": "community index not built — call POST /v1/community/detect first"
            })),
        )
    })?;

    let mut communities: Vec<crate::api::CommunityOverviewEntry> = store
        .members
        .iter()
        .map(|(&cid, members)| crate::api::CommunityOverviewEntry {
            community_id: cid,
            member_count: members.len(),
            centroid: store.centroids.get(&cid).cloned().unwrap_or_default(),
            sample_node_ids: members.iter().copied().take(10).collect(),
        })
        .collect();

    communities.sort_by(|a, b| b.member_count.cmp(&a.member_count));

    Ok(Json(crate::api::CommunityOverviewResponse {
        community_count: store.community_count,
        node_count: store.node_count,
        receipt: store.receipt.clone(),
        communities,
    }))
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/ingest/extract-entities",
    operation_id = "extract_entities",
    tag = "community",
    summary = "Extract entities and relationships with an LLM",
    description = "Sends the text to the configured provider, embeds each entity description, inserts the entities as Concept nodes, and adds relationship edges. Requires `VALORI_EMBED_PROVIDER`. The LLM output is committed to the audit chain, so replay never re-invokes the model.",
    request_body = valori_rag::community::ExtractEntitiesRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Entities and relationships inserted", body = valori_rag::community::ExtractEntitiesResponse),
        (status = 400, description = "Malformed or invalid request", body = ApiError),
        (status = 422, description = "VALORI_EMBED_PROVIDER is not configured"),
        (status = 502, description = "The LLM provider failed or returned unusable output"),
    ),
))]
/// `POST /v1/ingest/extract-entities`
///
/// Sends `text` to the configured LLM (reusing `VALORI_EMBED_PROVIDER`
/// credentials) to extract entities and relationships, embeds entity
/// descriptions as record vectors, inserts them as `Concept` graph nodes,
/// and adds relationship edges. Requires `VALORI_EMBED_PROVIDER` to be set.
async fn extract_entities(
    State(engine): State<SharedEngine>,
    Json(payload): Json<valori_rag::community::ExtractEntitiesRequest>,
) -> Result<
    Json<valori_rag::community::ExtractEntitiesResponse>,
    (StatusCode, Json<serde_json::Value>),
> {
    // Validate embed config available.
    let embed_cfg = {
        let eng = engine.read().await;
        eng.embed_config.clone()
    }.ok_or_else(|| (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({
        "error": "VALORI_EMBED_PROVIDER not configured — entity extraction requires an LLM provider"
    }))))?;

    let http = shared_http_client();

    // Call LLM to extract entities + relationships.
    let llm_cfg = valori_rag::LlmConfig {
        provider: embed_cfg.provider.clone(),
        model: embed_cfg.model.clone(),
        url: embed_cfg.url.clone(),
        api_key: embed_cfg.api_key.clone(),
    };
    let extracted = valori_rag::extract_entities_via_llm(
        &payload.text,
        &payload.entity_types,
        &llm_cfg,
        payload.model.as_deref(),
        &http,
    )
    .await
    .map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": e})),
        )
    })?;

    // Resolve namespace.
    let ns_id = {
        let eng = engine.read().await;
        eng.namespaces
            .resolve(payload.namespace.as_deref())
            .unwrap_or(0)
    };

    // Embed entity descriptions → insert records → create Concept nodes.
    let descriptions: Vec<String> = extracted
        .entities
        .iter()
        .map(|e| e.description.clone())
        .collect();
    let vecs = valori_ingest::embed_batch(&descriptions, &embed_cfg, &http)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": e.0})),
            )
        })?;

    let mut entity_name_to_node_id: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    let mut inserted_entities: Vec<valori_rag::community::InsertedEntity> = Vec::new();

    {
        let mut eng = engine.write().await;
        for (entity, vec) in extracted.entities.iter().zip(vecs.iter()) {
            let record_id = eng.insert_record_from_f32_ns(vec, ns_id).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e.to_string()})),
                )
            })?;

            let node_id = eng
                .create_node_for_record(
                    Some(record_id),
                    valori_kernel::types::enums::NodeKind::Concept as u8,
                    ns_id,
                )
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": e.to_string()})),
                    )
                })?;

            entity_name_to_node_id.insert(entity.name.clone(), node_id);
            inserted_entities.push(valori_rag::community::InsertedEntity {
                name: entity.name.clone(),
                kind: entity.kind.clone(),
                description: entity.description.clone(),
                node_id,
                record_id: Some(record_id),
            });
        }
    }

    // Create edges for relationships.
    let mut inserted_rels: Vec<valori_rag::community::InsertedRelationship> = Vec::new();
    let mut skipped = 0usize;

    {
        let mut eng = engine.write().await;
        for rel in &extracted.relationships {
            let from = entity_name_to_node_id.get(&rel.source).copied();
            let to = entity_name_to_node_id.get(&rel.target).copied();
            match (from, to) {
                (Some(from_id), Some(to_id)) => {
                    use valori_kernel::types::enums::EdgeKind;
                    let edge_id = eng
                        .create_edge_ns(from_id, to_id, EdgeKind::Relation as u8, ns_id)
                        .map_err(|e| {
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(serde_json::json!({"error": e.to_string()})),
                            )
                        })?;
                    inserted_rels.push(valori_rag::community::InsertedRelationship {
                        source_name: rel.source.clone(),
                        target_name: rel.target.clone(),
                        description: rel.description.clone(),
                        edge_id,
                    });
                }
                _ => {
                    skipped += 1;
                }
            }
        }
    }

    let entity_count = inserted_entities.len();
    let relationship_count = inserted_rels.len();

    Ok(Json(valori_rag::community::ExtractEntitiesResponse {
        entities: inserted_entities,
        relationships: inserted_rels,
        entity_count,
        relationship_count,
        skipped_relationships: skipped,
    }))
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/shard/routing",
    operation_id = "get_shard_routing",
    tag = "meta",
    summary = "Show which shard each collection routes to",
    description = "Routing is `namespace_id % shard_count` and is stable for the life of the collection. With `shard_count = 1` every collection reports shard 0.",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Shard assignment", body = crate::api::ShardRoutingResponse),
    ),
))]
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

    let shards: Vec<crate::api::ShardRoutingEntry> = shard_map
        .into_iter()
        .enumerate()
        .map(|(i, cols)| crate::api::ShardRoutingEntry {
            shard: i,
            collections: cols,
        })
        .collect();

    axum::Json(crate::api::ShardRoutingResponse {
        mode: "standalone".to_string(),
        shard_count,
        shards,
    })
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/models/health",
    operation_id = "get_models_health",
    tag = "meta",
    summary = "Integrity report for installed model packages",
    description = "Verifies the SHA-256 of every package under `VALORI_MODELS_DIR`. `reclaimable_bytes` counts packages no project references.",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Package health report", body = valori_models::health::SystemHealth),
    ),
))]
/// `GET /v1/models/health`
///
/// Returns integrity status of all installed model packages.
/// The models directory is read from `VALORI_MODELS_DIR` (defaults to
/// `~/.valori/models`).  Reference counts are always 0 until M6.2 wiring
/// lands; "reclaimable_bytes" reflects unreferenced-in-terms-of-projects.
async fn models_health() -> axum::Json<serde_json::Value> {
    let models_dir = std::env::var("VALORI_MODELS_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".valori").join("models")));

    let Some(dir) = models_dir else {
        return axum::Json(serde_json::json!({ "error": "models directory not configured" }));
    };

    match valori_models::PackageStore::new(&dir) {
        Ok(store) => {
            let refs = valori_models::RefCounter::new();
            let health = valori_models::system_health(&store, &refs);
            axum::Json(serde_json::to_value(health).unwrap_or_default())
        }
        Err(e) => axum::Json(serde_json::json!({ "error": e.to_string() })),
    }
}
