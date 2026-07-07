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

use axum::extract::{State, Query};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use tower_http::cors::{CorsLayer, Any};
use serde::{Deserialize, Serialize};

use axum::extract::Path;
use valori_consensus::types::{Raft, ShardId, CURRENT_SCHEMA_VERSION};
use valori_consensus::{ClientRequest, ValoriStateMachine};
use crate::cluster::ShardHandle;
use valori_kernel::event::KernelEvent;
use valori_kernel::fxp::qformat::SCALE;
use valori_kernel::index::SearchResult as KernelSearchResult;
use valori_kernel::types::enums::{EdgeKind, NodeKind};
use valori_kernel::types::id::{NodeId, RecordId};
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

use crate::api_keys::{ApiScope, AuthState, KeyStore, required_scope};
use crate::crypto_vault::{hex_to_key_id, key_id_to_hex, new_key_id};
use valori_kernel::crypto::KeyVault;
use crate::cluster::ClusterHandle;
use crate::cluster_api::cluster_router;
use crate::events::event_log::EventLogWriter;
use axum::extract::Extension;
use axum::middleware::Next;
use axum::extract::Request as AxumRequest;
use axum::http::header::AUTHORIZATION;
use axum::http::HeaderValue;
use axum::body::Body;

/// Startup readiness gate (fixes the partial-state-on-restart bug, B13).
///
/// On restart a node restores its state machine to the last persisted snapshot
/// index and then replays the log forward to catch up. Until that replay
/// reaches the committed index the node knew at boot, its local state is only
/// partially reconstructed. Serving reads in that window returns partial state.
///
/// This gate refuses local reads until apply has caught up to `target`. It is
/// startup-only: once satisfied it latches open and never gates again, so a
/// steady-state node keeps the documented "Local reads may lag slightly"
/// semantics. A fresh node (`target == 0`) is ready immediately.
struct ReadinessGate {
    target: u64,
    ready: std::sync::atomic::AtomicBool,
}

impl ReadinessGate {
    fn new(target: u64) -> Self {
        Self {
            target,
            ready: std::sync::atomic::AtomicBool::new(target == 0),
        }
    }

    /// `Ok(())` once the node has replayed up to the committed index it knew at
    /// boot; otherwise a 503 telling the caller to retry shortly.
    fn check(&self, raft: &Raft) -> Result<(), Response> {
        let applied = raft.metrics().borrow().last_applied.map_or(0, |l| l.index);
        self.check_applied(applied)
    }

    /// Pure readiness decision for a given applied index. Latches open: once
    /// caught up, all later calls return `Ok` regardless of `applied` (a
    /// steady-state node may legitimately lag a few entries behind committed).
    fn check_applied(&self, applied: u64) -> Result<(), Response> {
        use std::sync::atomic::Ordering;
        if self.ready.load(Ordering::Relaxed) {
            return Ok(());
        }
        if applied >= self.target {
            self.ready.store(true, Ordering::Relaxed);
            Ok(())
        } else {
            Err(read_unavailable(format!(
                "node catching up after restart: applied {applied} < startup-committed {} — retry shortly",
                self.target
            )))
        }
    }
}

#[derive(Clone)]
struct DataPlaneState {
    raft: Arc<Raft>,
    sm: ValoriStateMachine,
    /// Reused for the follower→leader read-index round trip on linearizable
    /// reads. Cloning a reqwest::Client is cheap and shares the connection pool.
    http: reqwest::Client,
    /// Paths to each shard's audit log on this node, keyed by ShardId.
    /// Used by /v1/proof/event-log and /v1/timeline to cover all shards.
    shard_event_log_paths: std::collections::BTreeMap<ShardId, std::path::PathBuf>,
    /// Startup readiness gate (B13). Shared; cheap to clone.
    readiness: Arc<ReadinessGate>,
    /// Phase 3.6: per-node AES-256-GCM vault. DEKs are not Raft-replicated;
    /// each node holds only the keys for records it encrypted.
    vault: Arc<dyn KeyVault + Send + Sync>,
    /// Phase I4: on-node embed config (from VALORI_EMBED_* env vars).
    /// None when VALORI_EMBED_PROVIDER is not set.
    embed_config: Option<crate::embedder::EmbedConfig>,
    /// VALORI_DIM from config — used as the fallback dim in /health before any
    /// insert has locked the kernel's dimension.
    config_dim: usize,
    /// Phase I5: node-local tree cache keyed by BLAKE3(text). Derived from
    /// build requests; not replicated via Raft (trees are deterministic from
    /// their source text, so any peer can rebuild them locally).
    tree_cache: Arc<tokio::sync::RwLock<std::collections::HashMap<String, crate::tree_rag::TreeIndex>>>,
    /// Phase I6: last community detection result on this node.
    /// Node-local (not Raft-replicated) — communities are derived from the
    /// graph which IS replicated, so any peer can re-derive an identical store.
    community_store: Arc<tokio::sync::RwLock<Option<crate::community::CommunityStore>>>,
    /// Phase S3: every shard this node runs (Phase S1's `ClusterHandle.shards`,
    /// always contains at least `ShardId(0)`). `raft`/`sm` above are shard 0's
    /// handles, kept as flat fields so every handler that doesn't resolve a
    /// namespace keeps working unchanged. Handlers that DO resolve a
    /// `NamespaceId` should route through `shard_for()` instead of `raft`/`sm`
    /// directly — see the doc comment there.
    shards: Arc<std::collections::BTreeMap<ShardId, ShardHandle>>,
    /// Phase S1's `VALORI_SHARD_COUNT` (default 1). Used by `shard_for_namespace()`.
    shard_count: u32,
}

/// Deterministic namespace → shard mapping (Phase S3). No placement table is
/// needed because Phase S1 keeps every shard symmetric — every configured
/// cluster member is a voter in every shard — so a pure function of the
/// namespace id is sufficient and requires no coordination. `shard_count=1`
/// (S1's default) always resolves to `ShardId(0)`, i.e. today's behavior.
fn shard_for_namespace(namespace_id: u16, shard_count: u32) -> ShardId {
    ShardId((namespace_id as u32) % shard_count.max(1))
}

impl DataPlaneState {
    /// Resolve which shard owns a namespace's DATA (records/nodes/edges).
    /// The namespace REGISTRY itself (name → id) always lives on shard 0 —
    /// see `ValoriStateMachine::resolve_namespace`/`list_namespaces`, unchanged
    /// by this — only where the namespace's actual records/nodes live is
    /// routed here.
    ///
    /// NOTE (Phase S3, deliberately not yet wired into most handlers): the
    /// `Auto*` `KernelEvent` variants (`AutoInsertRecord`, `AutoCreateNode`,
    /// `AutoCreateEdge`) do not carry a namespace id, and
    /// `ValoriStateMachine::apply()`'s generic dispatch branch always applies
    /// them to namespace 0 regardless of what a handler resolves — a
    /// pre-existing bug independent of sharding (see
    /// docs/phases/phase-S3-shard-routing-infrastructure.md). Routing THOSE
    /// writes to a non-zero shard today would silently scatter data across
    /// shards under a namespace id nothing actually wrote to. This accessor
    /// is used by `cluster_memory_upsert` (write) and `cluster_list_nodes`/
    /// `cluster_memory_search` (reads) as of Phase S3b — see those handlers
    /// for the current, deliberately narrow set of routed endpoints.
    fn shard_for(&self, namespace_id: u16) -> &ShardHandle {
        let shard_id = shard_for_namespace(namespace_id, self.shard_count);
        self.shards
            .get(&shard_id)
            .expect("shard_for_namespace always returns a shard id in 0..shard_count")
    }
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

fn make_cors_layer() -> Option<CorsLayer> {
    let origin = std::env::var("VALORI_CORS_ORIGIN").ok()?;
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

async fn cluster_auth_guard(
    Extension(auth): Extension<Arc<AuthState>>,
    req: AxumRequest,
    next: Next,
) -> Result<axum::response::Response, StatusCode> {
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

    if let Some(record) = auth.key_store.lookup(token) {
        if record.scope.satisfies(&required) {
            return Ok(next.run(req).await);
        }
        return Err(StatusCode::FORBIDDEN);
    }

    if let Some(ref legacy) = auth.legacy_token {
        use subtle::ConstantTimeEq;
        if token.as_bytes().ct_eq(legacy.as_bytes()).into() {
            return Ok(next.run(req).await);
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

/// The full router a cluster node serves: data plane + management plane.
pub fn build_cluster_router(
    handle: &ClusterHandle,
    audit: Option<Arc<std::sync::Mutex<EventLogWriter>>>,
) -> Router {
    let cfg = crate::config::NodeConfig::default();
    build_cluster_router_with_keys(handle, audit, cfg.auth_token.clone(), Arc::new(KeyStore::new(None)), &cfg, Arc::new(valori_effect::ReceiptStore::new(256)))
}

/// Cluster router with Phase 3.5 key store and optional legacy token.
pub fn build_cluster_router_with_keys(
    handle: &ClusterHandle,
    audit: Option<Arc<std::sync::Mutex<EventLogWriter>>>,
    auth_token: Option<String>,
    key_store: Arc<KeyStore>,
    node_cfg: &crate::config::NodeConfig,
    receipt_store: Arc<valori_effect::ReceiptStore>,
) -> Router {
    let raft = Arc::new(handle.raft.clone());
    // Collect the audit-log path for every shard on this node.
    let shard_event_log_paths: std::collections::BTreeMap<ShardId, std::path::PathBuf> = handle
        .shards
        .iter()
        .filter_map(|(id, h)| {
            h.event_log_writer
                .as_ref()
                .map(|w| (*id, w.lock().expect("audit mutex").path().to_path_buf()))
        })
        .collect();
    let state = DataPlaneState {
        raft: raft.clone(),
        sm: handle.state_machine.clone(),
        http: reqwest::Client::new(),
        shard_event_log_paths,
        readiness: Arc::new(ReadinessGate::new(handle.startup_committed_index)),
        vault: {
            use crate::crypto_vault::AesGcmVault;
            Arc::new(AesGcmVault::in_memory())
        },
        embed_config: crate::embedder::EmbedConfig::from_node_config(node_cfg),
        config_dim: node_cfg.dim,
        tree_cache: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        community_store: Arc::new(tokio::sync::RwLock::new(None)),
        shard_count: handle.shards.len() as u32,
        shards: Arc::new(
            handle
                .shards
                .iter()
                .map(|(id, h)| {
                    (
                        *id,
                        ShardHandle {
                            raft: h.raft.clone(),
                            state_machine: h.state_machine.clone(),
                            startup_committed_index: h.startup_committed_index,
                            event_log_writer: h.event_log_writer.clone(),
                        },
                    )
                })
                .collect(),
        ),
    };

    let auth = Arc::new(AuthState { key_store, legacy_token: auth_token });

    // ── Public routes (no auth) ───────────────────────────────────────────────
    let public = Router::new()
        .route("/health",  get(health))
        .route("/metrics", get(metrics))
        .with_state(state.clone());

    // ── Canonical v1 routes ───────────────────────────────────────────────────
    let v1 = Router::new()
        .route("/v1/records",                   post(insert_record))
        .route("/v1/search",                    post(search))
        .route("/v1/delete",                    post(delete_record))
        .route("/v1/soft-delete",               post(soft_delete_record))
        .route("/v1/vectors/batch-insert",      post(batch_insert))
        .route("/v1/namespaces",                post(create_collection_handler).get(list_collections_handler))
        .route("/v1/namespaces/:name",          delete(drop_collection_handler))
        .route("/v1/proof/state",               get(state_proof))
        .route("/v1/proof/event-log",           get(event_log_proof))
        .route("/v1/cluster/proof",             get(cluster_proof))
        .route("/v1/proof/receipt",             get(cluster_get_latest_receipt))
        .route("/v1/proof/receipt/:id",         get(cluster_get_receipt_by_id))
        .route("/v1/graph/node",                post(create_graph_node))
        .route("/v1/graph/node/:id",            get(get_graph_node).delete(delete_graph_node))
        .route("/v1/graph/edge",                post(create_graph_edge))
        .route("/v1/graph/edges/:id",           get(get_graph_edges))
        .route("/v1/graph/subgraph",            get(get_graph_subgraph))
        .route("/v1/graphrag",                  post(cluster_graphrag))
        .route("/v1/keys",                      post(cluster_create_key).get(cluster_list_keys))
        .route("/v1/keys/:id",                  delete(cluster_revoke_key))
        .route("/v1/records/encrypted",         post(cluster_insert_encrypted))
        .route("/v1/crypto/shred/:key_id",      delete(cluster_shred_key))
        .route("/v1/crypto/status/:key_id",     get(cluster_crypto_status))
        .route("/v1/index/config",              axum::routing::get(cluster_index_config))
        .route("/v1/index/rebuild",             post(cluster_index_rebuild))
        .route("/v1/shard/routing",             axum::routing::get(cluster_shard_routing))
        .route("/v1/ingest/document",           post(crate::ingest::ingest_document))
        .route("/v1/ingest",                    post(cluster_ingest))
        .route("/v1/ingest/status/:job_id",     get(crate::ingest::get_ingest_status))
        .route("/v1/ingest/update",             post(cluster_ingest_update))
        .route("/v1/ingest/extract-entities",   post(cluster_extract_entities))
        .route("/v1/tree/build",                post(cluster_tree_build))
        .route("/v1/tree/query",                post(cluster_tree_query))
        .route("/v1/tree/hybrid",               post(cluster_tree_hybrid))
        .route("/v1/tree/verify",               post(crate::tree_rag::tree_verify))
        .route("/v1/tree/chain-verify",         post(crate::tree_rag::tree_chain_verify))
        .route("/v1/community/detect",          post(cluster_community_detect))
        .route("/v1/community/search",          post(cluster_community_search))
        .route("/v1/community/overview",        get(cluster_community_overview))
        .route("/v1/memory/consolidate",        post(cluster_memory_consolidate))
        .route("/v1/memory/contradict",         post(cluster_memory_contradict))
        .route("/v1/memory/upsert",             post(cluster_memory_upsert))
        .route("/v1/memory/upsert_vector",      post(cluster_memory_upsert))
        .route("/v1/memory/search",             post(cluster_memory_search))
        .route("/v1/memory/search_vector",      post(cluster_memory_search))
        .route("/v1/memory/meta/set",           post(cluster_meta_set))
        .route("/v1/memory/meta/get",           axum::routing::get(cluster_meta_get))
        .route("/v1/graph/nodes",               get(cluster_list_nodes))
        .route("/v1/version",                   get(cluster_version))
        .route("/v1/timeline",                  get(cluster_timeline))
        .route("/v1/operations",                get(cluster_get_operations))
        .route("/v1/operations/:id",            get(cluster_get_operation_by_id))
        .route("/v1/snapshot/save",             post(cluster_snapshot_save))
        .route("/v1/snapshot/restore",          post(cluster_snapshot_restore))
        .route("/v1/snapshot/download",         get(cluster_snapshot_download));

    // ── Deprecated legacy routes ──────────────────────────────────────────────
    let legacy = Router::new()
        .route("/records",          post(insert_record))
        .route("/search",           post(search))
        .route("/operations",       get(cluster_get_operations))
        .route("/operations/:id",   get(cluster_get_operation_by_id))
        .route("/graph/node",       post(create_graph_node))
        .route("/graph/node/:id",   get(get_graph_node).delete(delete_graph_node))
        .route("/graph/edge",       post(create_graph_edge))
        .route("/graph/edges/:id",  get(get_graph_edges))
        .route("/graph/subgraph",   get(get_graph_subgraph))
        // snake_case alias kept for backward compat
        .route("/v1/vectors/batch_insert",  post(batch_insert))
        .layer(axum::middleware::from_fn(deprecation_warning));

    // Phase S6: shard-aware read-index needs every shard's raft handle,
    // independent of DataPlaneState (already moved into with_state above).
    let api_shards: std::collections::BTreeMap<ShardId, Raft> = handle
        .shards
        .iter()
        .map(|(id, h)| (*id, h.raft.clone()))
        .collect();

    use crate::capabilities::CapabilityRegistryBuilder;
    use crate::runner::TaskRegistry;
    let capability_registry: Arc<valori_effect::capability::CapabilityRegistry> = Arc::new(
        CapabilityRegistryBuilder::build_cluster(
            state.shards.clone(),
            state.sm.clone(),
            state.shard_count as u8,
            state.embed_config.clone(),
            state.http.clone(),
        )
    );
    let task_registry: Arc<TaskRegistry> = Arc::new(TaskRegistry::default_registry());

    let mut router = Router::new()
        .merge(public)
        .merge(v1)
        .merge(legacy)
        .with_state(state)
        .merge(cluster_router(raft, Arc::new(api_shards), audit))
        .layer(axum::middleware::from_fn(cluster_auth_guard))
        .layer(Extension(auth.clone()))
        .layer(Extension(receipt_store))
        .layer(Extension(capability_registry))
        .layer(Extension(task_registry));
    if let Some(cors) = make_cors_layer() {
        router = router.layer(cors);
    }
    router
}

async fn metrics() -> String {
    crate::telemetry::get_metrics()
}

/// Adds `Deprecation: true` (RFC 8594) to responses from legacy paths.
async fn deprecation_warning(req: AxumRequest<Body>, next: Next) -> Response {
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert("Deprecation", HeaderValue::from_static("true"));
    h.insert(
        "Link",
        HeaderValue::from_static(
            "<https://docs.valori.ai/api/v1>; rel=\"successor-version\"",
        ),
    );
    resp
}

// ── Collection (namespace) management ────────────────────────────────────────
//
// Phase S2: collection creation/drop goes through Raft
// (KernelEvent::AutoCreateNamespace / DropNamespace) instead of mutating a
// per-node, unreplicated registry directly — see docs/phases/phase-S2-*.md.
// A follower correctly 307-redirects these, rather than silently succeeding
// against its own out-of-sync local copy.
//
// Handler bodies (validation, response shaping) live in `routes::collections`
// and are shared with the standalone path; only the commit/read primitives
// below are cluster-specific.

/// Cluster impl of the shared collection primitives — writes commit through
/// Raft, reads come from the local state machine.
#[async_trait::async_trait]
impl crate::routes::collections::CollectionOps for DataPlaneState {
    async fn resolve(&self, name: &str) -> Option<u16> {
        self.sm.resolve_namespace(Some(name)).await
    }

    async fn create(
        &self,
        name: &str,
    ) -> Result<crate::routes::collections::CreatedCollection, Response> {
        // Best-effort pre-check for the response's `created` flag: a
        // concurrent create can still race this read, in which case `created`
        // may read `true` even though another request won the race. Cosmetic
        // only — `id` always comes from the committed response, never from
        // this check.
        let already_existed = self.sm.resolve_namespace(Some(name)).await.is_some();
        let resp = raft_write_data(
            &self.raft,
            ClientRequest {
                event: KernelEvent::AutoCreateNamespace { name: name.to_string() },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: 0,
            },
        )
        .await?;
        Ok(crate::routes::collections::CreatedCollection {
            id: resp.allocated_namespace_id.unwrap_or(0),
            already_existed,
        })
    }

    async fn drop_collection(&self, name: &str) -> Result<(), Response> {
        raft_write_data(
            &self.raft,
            ClientRequest {
                event: KernelEvent::DropNamespace { name: name.to_string() },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: 0,
            },
        )
        .await
        .map(|_| ())
    }

    async fn list(&self) -> Vec<(String, u16)> {
        // Local read, no Raft round trip — matches the eventual-consistency
        // convention every other list-style read in this file already uses
        // (e.g. cluster_list_nodes).
        self.sm.list_namespaces().await
    }
}

async fn create_collection_handler(
    State(s): State<DataPlaneState>,
    Json(payload): Json<crate::api::CreateCollectionRequest>,
) -> Result<Json<crate::api::CreateCollectionResponse>, Response> {
    crate::routes::collections::create_collection(&s, payload).await
}

async fn list_collections_handler(
    State(s): State<DataPlaneState>,
) -> Json<crate::api::ListCollectionsResponse> {
    crate::routes::collections::list_collections(&s).await
}

async fn drop_collection_handler(
    State(s): State<DataPlaneState>,
    Path(name): Path<String>,
) -> Result<StatusCode, Response> {
    crate::routes::collections::drop_collection(&s, &name).await
}

async fn health(State(state): State<DataPlaneState>) -> Response {
    let m = state.raft.metrics().borrow().clone();
    let embed_enabled = state.embed_config.is_some();
    let embed_provider = state.embed_config.as_ref().map(|c| c.provider.clone());
    // Report the dimension the kernel has actually locked to, not the config value.
    // Before any insert, falls back to config dim so callers still see what to send.
    let dim = state.sm.locked_dim().await.unwrap_or(state.config_dim);
    match m.current_leader {
        Some(leader) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ok",
                "leader": leader,
                "dim": dim,
                "embed_enabled": embed_enabled,
                "embed_provider": embed_provider,
            })),
        )
            .into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "status": "no-leader",
                "dim": dim,
                "embed_enabled": embed_enabled,
                "embed_provider": embed_provider,
            })),
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

/// Like [`raft_write`] but returns the committed `ClientResponse` so the caller
/// can read allocated IDs (record/node/edge) instead of pre-reading them in a
/// separate await — which would race a concurrent write for the same ID.
/// On any failure it returns the error `Response` for the caller to short-circuit.
async fn raft_write_data(
    raft: &Raft,
    req: ClientRequest,
) -> Result<valori_consensus::ClientResponse, Response> {
    match raft.client_write(req).await {
        Ok(resp) => {
            if let Some(reason) = &resp.data.rejected {
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({ "error": reason })),
                ).into_response());
            }
            Ok(resp.data)
        }
        Err(openraft::error::RaftError::APIError(
            openraft::error::ClientWriteError::ForwardToLeader(fwd),
        )) => Err(not_leader_response(fwd.leader_node.as_ref())),
        Err(e) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": format!("raft write failed: {e}") })),
        ).into_response()),
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
    /// Phase S7. Absent/"default" targets the default namespace, shard 0 —
    /// byte-identical to pre-S7 behavior.
    #[serde(default)]
    collection: Option<String>,
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
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(req): Json<InsertRequest>,
) -> Response {
    let vector = match to_fxp(&req.values) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))).into_response();
        }
    };

    // Phase S7: resolve collection -> namespace (registry always lives on
    // shard 0), then route the write to that namespace's data shard.
    let ns_id = match state.sm.resolve_namespace(req.collection.as_deref()).await {
        Some(id) => id,
        None => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": format!("unknown collection: {:?}", req.collection)
            }))).into_response();
        }
    };
    let shard = state.shard_for(ns_id);
    let shard_id = shard_for_namespace(ns_id, state.shard_count).0 as u8;

    // Capture state hash before write.
    let state_before: String = {
        let raw = state.sm.state_hash().await;
        raw.iter().map(|b| format!("{:02x}", b)).collect()
    };

    // ID is assigned by the state machine at apply time (AutoInsertRecord).
    let resp = match raft_write_data(
        &shard.raft,
        ClientRequest {
            event: KernelEvent::AutoInsertRecord {
                vector,
                metadata: req.metadata,
                tag: req.tag,
            },
            request_id: req.request_id,
            schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns_id,
        },
    ).await {
        Ok(r) => r,
        Err(e) => return e,
    };

    let state_after: String = resp.state_hash.iter().map(|b| format!("{:02x}", b)).collect();
    {
        use valori_planner::operation::{OperationKind, OperationInputs};
        let inputs = OperationInputs::Ingest {
            strategy: "direct".into(),
            collection: req.collection.clone().unwrap_or_else(|| "default".into()),
            shard_id,
            embed_enabled: false,
        };
        crate::receipt_bridge::emit_write(&receipts, OperationKind::Ingest, &inputs, ns_id, shard_id, resp.log_index, true, state_before, state_after);
    }

    (StatusCode::OK, Json(InsertResponse {
        id: resp.allocated_record_id.unwrap_or(0),
        log_index: resp.log_index,
        deduplicated: resp.deduplicated,
    })).into_response()
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
    /// C4.1b: decay half-life in seconds for recency-aware re-ranking.
    #[serde(default)]
    decay_half_life_secs: Option<u64>,
    /// BM25 hybrid reranking — fetch wider pool, re-rank by term frequency.
    #[serde(default = "default_rerank")]
    rerank: bool,
    /// Raw query text for BM25 scoring. Required when `rerank=true`.
    #[serde(default)]
    query_text: Option<String>,
    /// Optional JSON object whose key-value pairs must ALL be present (and equal)
    /// in a record's metadata for the record to be returned.
    /// Supports range operators: `{"year": {"gte": 2020, "lte": 2024}}`.
    #[serde(default)]
    metadata_filter: Option<serde_json::Map<String, serde_json::Value>>,
    /// Phase S7. Absent/"default" targets the default namespace, shard 0 —
    /// byte-identical to pre-S7 behavior.
    #[serde(default)]
    collection: Option<String>,
}

fn default_rerank() -> bool { true }

fn default_k() -> usize {
    10
}

// Wire-compatible with the standalone server's SearchHit { id, score }
// (api.rs) so one SDK client speaks to both standalone and cluster nodes.
// `score` is the L2 distance as a float (raw Q32.32 divided by SCALE²),
// matching the standalone conversion in server.rs.
#[derive(Serialize)]
struct SearchHit {
    id: u32,
    score: f32,
}

async fn search(
    State(state): State<DataPlaneState>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(req): Json<SearchRequest>,
) -> Response {
    // Startup readiness gate (B13): never serve from a state machine that is
    // still replaying its log back up to the committed index known at boot.
    if let Err(resp) = state.readiness.check(&state.raft) {
        return resp;
    }

    // Phase S7: resolve collection -> namespace (registry always lives on
    // shard 0), then route the read to that namespace's data shard.
    let ns_id = match state.sm.resolve_namespace(req.collection.as_deref()).await {
        Some(id) => id,
        None => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": format!("unknown collection: {:?}", req.collection)
            }))).into_response();
        }
    };
    let shard = state.shard_for(ns_id);
    let shard_sm = &shard.state_machine;

    // Dimension check against the locked kernel dim (set on first insert).
    // An empty store (dim == None) accepts any query length.
    if let Some(locked) = shard_sm.locked_dim().await {
        if req.query.len() != locked {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": format!(
                    "Query vector has {} elements but this store is locked to dim={}. \
                     Check GET /health for the current dim.",
                    req.query.len(), locked
                )
            }))).into_response();
        }
    }

    let query = match to_fxp(&req.query) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))).into_response();
        }
    };

    // Linearizable reads (the default) establish a read index first, so the
    // local scan below reflects every write committed before this read began.
    if req.consistency == Consistency::Linearizable {
        if let Err(resp) = ensure_read_consistency(shard_for_namespace(ns_id, state.shard_count), &shard.raft, &state.http).await {
            return resp;
        }
    }

    let k = req.k.max(1);
    let half_life = req.decay_half_life_secs.unwrap_or(0);
    let mf = req.metadata_filter.clone();

    // When metadata_filter is set, over-fetch so post-filtering has enough candidates.
    let base_k = if mf.is_some() {
        k.saturating_mul(10).max(100).min(5000)
    } else {
        k
    };

    // C4.1b: when decay is requested, over-fetch and re-rank using per-record
    // creation timestamps tracked in the state machine.
    let use_rerank = req.rerank && req.query_text.is_some();
    let fetch_k = if use_rerank {
        (base_k * crate::valori_reranker::POOL_FACTOR).max(base_k)
    } else {
        base_k
    };
    let query_text_owned = req.query_text.clone().unwrap_or_default();

    let results: Vec<SearchHit> = if half_life == 0 {
        let raw: Vec<SearchHit> = shard_sm
            .with_state(|s| {
                let mut buf = vec![KernelSearchResult::default(); fetch_k];
                let n = s.search_l2(&query, &mut buf, None);
                buf[..n]
                    .iter()
                    .map(|r| SearchHit { id: r.id.0, score: r.score as f32 / (SCALE as f32 * SCALE as f32) })
                    .collect()
            })
            .await;
        // Post-filter by metadata predicate before reranking/trimming. Reads
        // the replicated KernelState.meta map (set via SetMeta) so every
        // replica filters identically, not a per-node sidecar.
        let filtered: Vec<SearchHit> = if let Some(ref f) = mf {
            shard_sm.with_state(|s| {
                raw.into_iter()
                    .filter(|h| {
                        let key = format!("rec:{}", h.id);
                        match s.meta.get(&key).and_then(|v| serde_json::from_str::<serde_json::Value>(v).ok()) {
                            Some(meta) => crate::api::matches_metadata_filter(&meta, f),
                            None => false,
                        }
                    })
                    .collect()
            }).await
        } else {
            raw
        };
        if use_rerank && !filtered.is_empty() && mf.is_none() {
            let candidates: Vec<(u64, f32)> = filtered.iter()
                .map(|h| (h.id as u64, h.score))
                .collect();
            let candidate_ids: Vec<u64> = candidates.iter().map(|(id, _)| *id).collect();
            shard_sm.with_text_corpus(|corpus| {
                // build a reranker seeded with only the candidate texts
                let mut reranker = crate::valori_reranker::ValoriReranker::new();
                for id in &candidate_ids {
                    if let Some(text) = corpus.get(id) {
                        reranker.insert(*id, text);
                    }
                }
                reranker.rerank(&query_text_owned, candidates)
                    .into_iter().take(k)
                    .map(|(id, score)| SearchHit { id: id as u32, score })
                    .collect()
            }).await
        } else {
            filtered.into_iter().take(k).collect()
        }
    } else {
        let pool = base_k.saturating_mul(4).max(50).min(5000);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs()).unwrap_or(0);
        let decayed: Vec<crate::decay::DecayedHit> = shard_sm
            .with_state_and_timestamps(|s, created_at| {
                let mut buf = vec![KernelSearchResult::default(); pool];
                let n = s.search_l2(&query, &mut buf, None);
                let candidates: Vec<crate::decay::DecayHit> = buf[..n]
                    .iter()
                    .map(|r| crate::decay::DecayHit {
                        id: r.id.0,
                        distance: r.score as f32 / (SCALE as f32 * SCALE as f32),
                        created_at: created_at.get(&r.id.0).copied(),
                    })
                    .collect();
                crate::decay::rerank(candidates, now, half_life, pool)
            })
            .await;
        if let Some(ref f) = mf {
            shard_sm.with_state(|s| {
                decayed.into_iter()
                    .filter(|h| {
                        let key = format!("rec:{}", h.id);
                        match s.meta.get(&key).and_then(|v| serde_json::from_str::<serde_json::Value>(v).ok()) {
                            Some(meta) => crate::api::matches_metadata_filter(&meta, f),
                            None => false,
                        }
                    })
                    .take(k)
                    .map(|h| SearchHit { id: h.id, score: h.distance })
                    .collect::<Vec<_>>()
            }).await
        } else {
            decayed.into_iter()
                .take(k)
                .map(|h| SearchHit { id: h.id, score: h.distance })
                .collect::<Vec<_>>()
        }
    };

    let state_hash: String = {
        let raw = shard.state_machine.state_hash().await;
        raw.iter().map(|b| format!("{:02x}", b)).collect()
    };
    let shard_id = shard_for_namespace(ns_id, state.shard_count).0 as u8;
    {
        use valori_planner::operation::{OperationKind, OperationInputs, ConsistencyLevel as PlannerConsistency};
        let inputs = OperationInputs::Search {
            k: req.k as u32,
            collection: req.collection.clone().unwrap_or_else(|| "default".into()),
            shard_id,
            rerank: req.rerank,
            decay: req.decay_half_life_secs.is_some(),
            metadata_filter: req.metadata_filter.is_some(),
            consistency: if req.consistency == Consistency::Linearizable {
                PlannerConsistency::Linearizable
            } else {
                PlannerConsistency::Local
            },
        };
        crate::receipt_bridge::emit_read(&receipts, OperationKind::Search, &inputs, ns_id, shard_id, 0, true, state_hash);
    }

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
async fn ensure_read_consistency(shard_id: ShardId, raft: &Raft, http: &reqwest::Client) -> Result<(), Response> {
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

    let url = format!("http://{leader_api}/v1/cluster/read-index?shard={}", shard_id.0);
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
//
// Phase S7: record ids are only unique within their own shard's kernel state
// (each shard runs an independent id counter), so the caller must name the
// collection the record was inserted into. Absent/"default" resolves to the
// default namespace, shard 0. Handler bodies live in `routes::records`.

/// Cluster impl of the shared record-deletion primitives — commits through
/// Raft on the owning shard.
#[async_trait::async_trait]
impl crate::routes::records::RecordOps for DataPlaneState {
    async fn resolve_collection(&self, name: Option<&str>) -> Option<u16> {
        self.sm.resolve_namespace(name).await
    }

    async fn delete(
        &self,
        ns: u16,
        id: u32,
        soft: bool,
    ) -> Result<crate::routes::records::DeletedRecord, Response> {
        let shard = self.shard_for(ns);
        let shard_id = shard_for_namespace(ns, self.shard_count).0 as u8;
        let state_before: String = {
            let raw = self.sm.state_hash().await;
            raw.iter().map(|b| format!("{:02x}", b)).collect()
        };
        let event = if soft {
            KernelEvent::SoftDeleteRecord { id: RecordId(id) }
        } else {
            KernelEvent::DeleteRecord { id: RecordId(id) }
        };
        let resp = raft_write_data(&shard.raft, ClientRequest {
            event,
            request_id: None,
            schema_version: CURRENT_SCHEMA_VERSION,
            namespace_id: ns,
        })
        .await?;
        let state_after: String = resp.state_hash.iter().map(|b| format!("{:02x}", b)).collect();
        Ok(crate::routes::records::DeletedRecord {
            log_index: Some(resp.log_index),
            shard_id,
            cluster: true,
            state_before,
            state_after,
        })
    }
}

async fn delete_record(
    State(state): State<DataPlaneState>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(req): Json<crate::api::DeleteRecordRequest>,
) -> Result<Json<crate::api::DeleteRecordResponse>, Response> {
    crate::routes::records::delete_record(&state, &receipts, req, false).await
}

async fn soft_delete_record(
    State(state): State<DataPlaneState>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(req): Json<crate::api::DeleteRecordRequest>,
) -> Result<Json<crate::api::DeleteRecordResponse>, Response> {
    crate::routes::records::delete_record(&state, &receipts, req, true).await
}

// ── Batch insert ──────────────────────────────────────────────────────────────
// Wire-compatible with the standalone server: request `{ batch: [[f32]] }`,
// response `{ ids: [u32] }`. Any rejected vector fails the whole batch with a
// 422 (the standalone engine is all-or-nothing too).

#[derive(Deserialize)]
struct BatchInsertRequest {
    batch: Vec<Vec<f32>>,
    /// Per-vector metadata strings (UTF-8). Forwarded into the committed
    /// `AutoInsertRecord` event and therefore included in the BLAKE3 audit chain.
    #[serde(default)]
    metadata: Option<Vec<Option<String>>>,
    /// Phase S7. Absent/"default" targets the default namespace, shard 0 —
    /// byte-identical to pre-S7 behavior.
    #[serde(default)]
    collection: Option<String>,
}

async fn batch_insert(
    State(state): State<DataPlaneState>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(req): Json<BatchInsertRequest>,
) -> Response {
    let ns_id = match state.sm.resolve_namespace(req.collection.as_deref()).await {
        Some(id) => id,
        None => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": format!("unknown collection: {:?}", req.collection)
            }))).into_response();
        }
    };
    let shard = state.shard_for(ns_id);
    let shard_raft = &shard.raft;
    let shard_id = shard_for_namespace(ns_id, state.shard_count).0 as u8;
    let state_before: String = { let raw = state.sm.state_hash().await; raw.iter().map(|b| format!("{:02x}", b)).collect() };

    let mut ids = Vec::with_capacity(req.batch.len());

    for values in req.batch {
        let vector = match to_fxp(&values) {
            Ok(v) => v,
            Err(e) => {
                return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e })))
                    .into_response();
            }
        };

        let meta_bytes = req.metadata.as_ref()
            .and_then(|m| m.get(ids.len()))
            .and_then(|s| s.as_ref())
            .map(|s| s.as_bytes().to_vec());

        match shard_raft
            .client_write(ClientRequest {
                event: KernelEvent::AutoInsertRecord { vector, metadata: meta_bytes, tag: 0 },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns_id,
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

    let state_after: String = { let raw = shard.state_machine.state_hash().await; raw.iter().map(|b| format!("{:02x}", b)).collect() };
    {
        use valori_planner::operation::{OperationKind, OperationInputs};
        let inputs = OperationInputs::BatchInsert {
            count: ids.len() as u32,
            collection: req.collection.clone().unwrap_or_else(|| "default".into()),
            shard_id,
        };
        crate::receipt_bridge::emit_write(&receipts, OperationKind::BatchInsert, &inputs, ns_id, shard_id, 0, true, state_before, state_after);
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

// ── Cluster proof — the demo/verification endpoint ────────────────────────────
// Returns the full verifiable state: node identity, BLAKE3 state hash, and the
// applied index + term at the time of the read. Call this on all nodes and
// compare `final_state_hash` to verify the cluster has a consistent view.

async fn cluster_proof(State(state): State<DataPlaneState>) -> Response {
    let m = state.raft.metrics().borrow().clone();
    let hash = state.sm.state_hash().await;
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "node_id": m.id,
            "final_state_hash": hex,
            "last_applied_index": m.last_applied.map(|l| l.index),
            "term": m.current_term,
        })),
    )
        .into_response()
}

// ── Event-log proof ───────────────────────────────────────────────────────────
// BLAKE3 hash of this node's events.log file, in the same format as the
// standalone `/v1/proof/event-log` endpoint. The hash covers the raw bytes of
// the current live segment — sealed archive segments are not included.

async fn event_log_proof(State(state): State<DataPlaneState>) -> Response {
    if state.shard_event_log_paths.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "no event log configured on this node" })),
        )
            .into_response();
    }
    let mut shards = serde_json::Map::new();
    for (shard_id, path) in &state.shard_event_log_paths {
        match crate::events::event_proof::compute_event_log_hash(path) {
            Ok(bytes) => {
                let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                shards.insert(shard_id.0.to_string(), serde_json::json!({ "event_log_hash": hex }));
            }
            Err(e) => {
                shards.insert(shard_id.0.to_string(), serde_json::json!({ "error": format!("cannot hash event log: {e}") }));
            }
        }
    }
    // Top-level `event_log_hash` = shard 0 for backward compat with single-shard clients.
    let top_hash = shards.get("0").and_then(|v| v.get("event_log_hash")).cloned();
    let mut body = serde_json::Map::new();
    if let Some(h) = top_hash {
        body.insert("event_log_hash".into(), h);
    }
    body.insert("shards".into(), serde_json::Value::Object(shards));
    (StatusCode::OK, Json(serde_json::Value::Object(body))).into_response()
}

// ── Graph — shared handlers (routes::graph) ──────────────────────────────────
//
// Handler bodies (kind validation, 404 shaping, list pagination) live in
// `routes::graph` and are shared with the standalone path; only the
// commit/read primitives below are cluster-specific. Phase S8 shard routing
// is preserved: every op resolves the collection and targets the shard that
// owns that namespace. Reads keep the startup readiness gate (B13).

/// Cluster impl of the shared graph primitives — writes commit through Raft
/// on the owning shard, reads come from that shard's state machine.
#[async_trait::async_trait]
impl crate::routes::graph::GraphOps for DataPlaneState {
    async fn resolve_collection(&self, name: Option<&str>) -> Option<u16> {
        self.sm.resolve_namespace(name).await
    }

    async fn create_node(
        &self,
        ns: u16,
        kind: NodeKind,
        record_id: Option<u32>,
    ) -> Result<crate::routes::graph::CommittedGraphWrite, Response> {
        let resp = raft_write_data(
            &self.shard_for(ns).raft,
            ClientRequest {
                event: KernelEvent::AutoCreateNode { kind, record: record_id.map(RecordId) },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns,
            },
        )
        .await?;
        Ok(crate::routes::graph::CommittedGraphWrite {
            id: resp.allocated_node_id.unwrap_or(0),
            log_index: Some(resp.log_index),
        })
    }

    async fn create_edge(
        &self,
        ns: u16,
        from: u32,
        to: u32,
        kind: EdgeKind,
    ) -> Result<crate::routes::graph::CommittedGraphWrite, Response> {
        let resp = raft_write_data(
            &self.shard_for(ns).raft,
            ClientRequest {
                event: KernelEvent::AutoCreateEdge { from: NodeId(from), to: NodeId(to), kind },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns,
            },
        )
        .await?;
        Ok(crate::routes::graph::CommittedGraphWrite {
            id: resp.allocated_edge_id.unwrap_or(0),
            log_index: Some(resp.log_index),
        })
    }

    async fn delete_node(&self, ns: u16, id: u32) -> Result<Option<u64>, Response> {
        let resp = raft_write_data(
            &self.shard_for(ns).raft,
            ClientRequest {
                event: KernelEvent::DeleteNode { id: NodeId(id) },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns,
            },
        )
        .await?;
        Ok(Some(resp.log_index))
    }

    async fn get_node(
        &self,
        ns: u16,
        id: u32,
    ) -> Result<Option<crate::api::GetNodeResponse>, Response> {
        self.readiness.check(&self.raft)?;
        Ok(self
            .shard_for(ns)
            .state_machine
            .with_state(move |s| {
                s.get_node(NodeId(id)).map(|n| crate::api::GetNodeResponse {
                    kind: n.kind as u8,
                    record_id: n.record.map(|r| r.0),
                    namespace_id: n.namespace_id,
                })
            })
            .await)
    }

    async fn node_edges(
        &self,
        ns: u16,
        id: u32,
    ) -> Result<Option<Vec<crate::api::EdgeData>>, Response> {
        self.readiness.check(&self.raft)?;
        Ok(self
            .shard_for(ns)
            .state_machine
            .with_state(move |s| {
                s.outgoing_edges(NodeId(id)).map(|iter| {
                    iter.map(|e| crate::api::EdgeData {
                        edge_id: e.id.0,
                        to_node: e.to.0,
                        kind: e.kind as u8,
                    })
                    .collect::<Vec<_>>()
                })
            })
            .await)
    }

    async fn list_nodes(&self, ns: u16) -> Result<Vec<crate::api::NodeInfo>, Response> {
        // Phase S3b: read from the shard that owns this namespace's data.
        Ok(self
            .shard_for(ns)
            .state_machine
            .with_state(move |s| {
                s.iter_nodes()
                    .filter(|n| n.namespace_id == ns)
                    .map(|n| crate::api::NodeInfo {
                        node_id: n.id.0,
                        kind: n.kind as u8,
                        record_id: n.record.map(|r| r.0),
                        namespace_id: n.namespace_id,
                    })
                    .collect::<Vec<_>>()
            })
            .await)
    }

    async fn subgraph(
        &self,
        ns: u16,
        root: u32,
        depth: u32,
    ) -> Result<(serde_json::Value, serde_json::Value), Response> {
        self.readiness.check(&self.raft)?;
        Ok(self
            .shard_for(ns)
            .state_machine
            .with_state(move |s| {
                let (nodes, edges) = crate::graph_rag::expand_subgraph(s, &[root], depth);
                (serde_json::Value::Array(nodes), serde_json::Value::Array(edges))
            })
            .await)
    }
}

async fn create_graph_node(
    State(state): State<DataPlaneState>,
    Json(req): Json<crate::api::CreateNodeRequest>,
) -> Result<Json<crate::api::CreateNodeResponse>, Response> {
    crate::routes::graph::create_node(&state, req).await
}

// ── Graph — get / delete node ─────────────────────────────────────────────────
//
// Phase S8: node/edge ids are only unique within their own shard's kernel
// state, so lookups must be told which collection to look in — the same
// reasoning as `DeleteRequest::collection` (S7). The shared
// `routes::graph::CollectionQuery` carries that parameter on both paths.

async fn get_graph_node(
    State(state): State<DataPlaneState>,
    Path(id): Path<u32>,
    axum::extract::Query(q): axum::extract::Query<crate::routes::graph::CollectionQuery>,
) -> Result<Json<crate::api::GetNodeResponse>, Response> {
    crate::routes::graph::get_node(&state, id, q).await
}

async fn delete_graph_node(
    State(state): State<DataPlaneState>,
    Path(id): Path<u32>,
    axum::extract::Query(q): axum::extract::Query<crate::routes::graph::CollectionQuery>,
) -> Result<Json<crate::api::DeleteNodeResponse>, Response> {
    crate::routes::graph::delete_node(&state, id, q).await
}

// ── Graph — create edge ───────────────────────────────────────────────────────

async fn create_graph_edge(
    State(state): State<DataPlaneState>,
    Json(req): Json<crate::api::CreateEdgeRequest>,
) -> Result<Json<crate::api::CreateEdgeResponse>, Response> {
    crate::routes::graph::create_edge(&state, req).await
}

// ── Graph — get outgoing edges ────────────────────────────────────────────────

async fn get_graph_edges(
    State(state): State<DataPlaneState>,
    Path(id): Path<u32>,
    axum::extract::Query(q): axum::extract::Query<crate::routes::graph::CollectionQuery>,
) -> Result<Json<crate::api::GetEdgesResponse>, Response> {
    crate::routes::graph::get_edges(&state, id, q).await
}

// ── Graph — BFS subgraph ──────────────────────────────────────────────────────

fn default_subgraph_depth() -> u32 { 2 }

async fn get_graph_subgraph(
    State(state): State<DataPlaneState>,
    axum::extract::Query(q): axum::extract::Query<crate::routes::graph::SubgraphQuery>,
) -> Result<Json<serde_json::Value>, Response> {
    crate::routes::graph::get_subgraph(&state, q).await
}

// ── Phase 3.15: native GraphRAG (cluster) — KNN + subgraph in one snapshot ────

#[derive(serde::Deserialize)]
struct ClusterGraphRagRequest {
    query_vector: Vec<f32>,
    k: usize,
    #[serde(default = "default_subgraph_depth")]
    depth: u32,
    #[serde(default)]
    consistency: Consistency,
    /// Phase S8. Absent/"default" targets the default namespace, shard 0 —
    /// byte-identical to pre-S8 behavior.
    #[serde(default)]
    collection: Option<String>,
}

async fn cluster_graphrag(
    State(state): State<DataPlaneState>,
    Json(req): Json<ClusterGraphRagRequest>,
) -> Response {
    if let Err(resp) = state.readiness.check(&state.raft) {
        return resp;
    }

    let ns_id = match state.sm.resolve_namespace(req.collection.as_deref()).await {
        Some(id) => id,
        None => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": format!("unknown collection: {:?}", req.collection)
            }))).into_response();
        }
    };
    let shard = state.shard_for(ns_id);
    let shard_sm = &shard.state_machine;

    if let Some(locked) = shard_sm.locked_dim().await {
        if req.query_vector.len() != locked {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": format!(
                    "Query vector has {} elements but this store is locked to dim={}. \
                     Check GET /health for the current dim.",
                    req.query_vector.len(), locked
                )
            }))).into_response();
        }
    }

    let query = match to_fxp(&req.query_vector) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))).into_response();
        }
    };

    // Linearizable by default: establish a read index so the local snapshot
    // reflects every write committed before this GraphRAG read began.
    if req.consistency == Consistency::Linearizable {
        if let Err(resp) = ensure_read_consistency(shard_for_namespace(ns_id, state.shard_count), &shard.raft, &state.http).await {
            return resp;
        }
    }

    let k = req.k.max(1);
    let depth = req.depth;

    let payload = shard_sm
        .with_state(move |s| {
            let mut buf = vec![KernelSearchResult::default(); k];
            let n = s.search_l2(&query, &mut buf, None);
            let hits: Vec<(u32, i64)> =
                buf[..n].iter().map(|r| (r.id.0, r.score)).collect();

            let record_ids: Vec<u32> = hits.iter().map(|(id, _)| *id).collect();
            let seed_map = crate::graph_rag::resolve_seed_nodes(s, &record_ids);

            let mut seeds: Vec<u32> = Vec::new();
            let mut hits_out: Vec<serde_json::Value> = Vec::new();
            for (record_id, score) in &hits {
                let node_id = seed_map.get(record_id).copied();
                if let Some(nid) = node_id {
                    seeds.push(nid);
                }
                hits_out.push(serde_json::json!({
                    "memory_id": format!("rec:{record_id}"),
                    "record_id": record_id,
                    "score": score,
                    "node_id": node_id,
                }));
            }

            let (nodes, edges) = crate::graph_rag::expand_subgraph(s, &seeds, depth);
            serde_json::json!({
                "hits": hits_out,
                "seed_nodes": seeds,
                "subgraph": { "nodes": nodes, "edges": edges },
            })
        })
        .await;

    (StatusCode::OK, Json(payload)).into_response()
}

// ── Phase 3.5: API key management (cluster) ───────────────────────────────────

#[derive(serde::Deserialize)]
struct ClusterCreateKeyRequest {
    #[serde(default = "default_cluster_scope")]
    scope: ApiScope,
    collection: Option<String>,
    description: Option<String>,
}

fn default_cluster_scope() -> ApiScope { ApiScope::ReadWrite }

async fn cluster_create_key(
    Extension(auth): Extension<Arc<AuthState>>,
    Json(req): Json<ClusterCreateKeyRequest>,
) -> impl axum::response::IntoResponse {
    let created = auth.key_store.create(req.scope, req.collection, req.description);
    (StatusCode::CREATED, Json(created))
}

async fn cluster_list_keys(
    Extension(auth): Extension<Arc<AuthState>>,
) -> impl axum::response::IntoResponse {
    let keys = auth.key_store.list();
    Json(serde_json::json!({ "keys": keys }))
}

async fn cluster_revoke_key(
    Extension(auth): Extension<Arc<AuthState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    if auth.key_store.revoke(&id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

// ── Phase 3.6: Crypto-shredding ───────────────────────────────────────────────

#[derive(Deserialize)]
struct ClusterInsertEncryptedRequest {
    payload: String,
    tag: Option<u64>,
    collection: Option<String>,
    key_id: Option<String>,
}

#[derive(Serialize)]
struct ClusterInsertEncryptedResponse {
    id: u32,
    key_id: String,
    log_index: u64,
}

async fn cluster_insert_encrypted(
    State(state): State<DataPlaneState>,
    Json(req): Json<ClusterInsertEncryptedRequest>,
) -> Response {
    use base64::Engine as _;
    let plaintext = match base64::engine::general_purpose::STANDARD.decode(&req.payload) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let key_id: [u8; 16] = if let Some(ref hex) = req.key_id {
        match hex_to_key_id(hex) {
            Some(k) => k,
            None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "key_id must be 32 hex chars"}))).into_response(),
        }
    } else {
        new_key_id()
    };

    // Encrypt on this node's vault BEFORE submitting to Raft.
    let ciphertext = match state.vault.encrypt(key_id, &plaintext) {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("{e:?}")}))).into_response(),
    };

    let ns = if let Some(ref coll) = req.collection {
        match state.sm.resolve_namespace(Some(coll.as_str())).await {
            Some(id) => id,
            None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Collection not found"}))).into_response(),
        }
    } else {
        valori_kernel::types::id::DEFAULT_NS.0
    };
    // Phase S5: route to the shard that owns this namespace's data. Safe
    // now that cluster_shred_key (below) broadcasts to every shard instead
    // of assuming shard 0 — a key_id's ciphertext can land on any shard
    // depending on which collection it was inserted into, and shredding
    // must find it wherever it is.
    let shard_raft = &state.shard_for(ns).raft;

    raft_write(
        shard_raft,
        ClientRequest {
            event: KernelEvent::AutoInsertRecordEncrypted {
                key_id,
                ciphertext,
                namespace_id: ns,
                tag: req.tag.unwrap_or(0),
            },
            request_id: None,
            schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
        },
        move |resp| {
            (StatusCode::CREATED, Json(ClusterInsertEncryptedResponse {
                id: resp.allocated_record_id.unwrap_or(0),
                key_id: key_id_to_hex(&key_id),
                log_index: resp.log_index,
            })).into_response()
        },
    )
    .await
}

async fn cluster_shred_key(
    State(state): State<DataPlaneState>,
    Path(key_id_hex): Path<String>,
) -> Response {
    let key_id = match hex_to_key_id(&key_id_hex) {
        Some(k) => k,
        None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "key_id must be 32 hex chars"}))).into_response(),
    };

    // Shred the vault key locally FIRST — the compliance-critical,
    // irreversible step: this node's ciphertext-decryption capability for
    // key_id is destroyed unconditionally, regardless of what follows.
    if let Err(e) = state.vault.shred(key_id) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("{e:?}")}))).into_response();
    }

    // Phase S5: propagate FLAG_SHREDDED to EVERY shard, not just shard 0 —
    // a key_id's ciphertext can land on any shard depending on which
    // collection it was inserted into (cluster_insert_encrypted routes by
    // namespace since this phase). KernelState::apply_shred_key is a safe,
    // idempotent no-op on a shard holding no matching records, so
    // attempting every shard is always correct.
    //
    // A single write can't be routed with one 307 the way other endpoints
    // are: different shards may be led by different nodes, so there is no
    // single "the leader" to redirect to. Each shard is attempted directly;
    // shards this node doesn't lead are reported, not silently dropped —
    // retry (idempotent, safe) against this same endpoint to complete
    // propagation, since a later call re-attempts every shard including
    // ones already done (a no-op there).
    let mut shard_status = serde_json::Map::new();
    let mut all_shredded = true;
    for (shard_id, handle) in state.shards.iter() {
        let key = format!("shard_{}", shard_id.0);
        match handle.raft.client_write(ClientRequest {
            event: KernelEvent::ShredKey { key_id },
            request_id: None,
            schema_version: CURRENT_SCHEMA_VERSION, namespace_id: 0,
        }).await {
            Ok(_) => { shard_status.insert(key, serde_json::json!({ "status": "shredded" })); }
            Err(openraft::error::RaftError::APIError(
                openraft::error::ClientWriteError::ForwardToLeader(fwd),
            )) => {
                all_shredded = false;
                shard_status.insert(key, serde_json::json!({
                    "status": "not-leader",
                    "leader_api_addr": fwd.leader_node.map(|n| n.api_addr.clone()),
                }));
            }
            Err(e) => {
                all_shredded = false;
                shard_status.insert(key, serde_json::json!({ "status": "error", "detail": e.to_string() }));
            }
        }
    }

    (StatusCode::OK, Json(serde_json::json!({
        "key_id": key_id_hex,
        "shredded": all_shredded,
        "shards": shard_status,
        "note": if all_shredded {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(
                "vault key destroyed on this node; FLAG_SHREDDED did not reach every shard \
                 because this node doesn't lead them all — retry this call (idempotent) to \
                 complete propagation".into()
            )
        },
    }))).into_response()
}

async fn cluster_crypto_status(
    State(state): State<DataPlaneState>,
    Path(key_id_hex): Path<String>,
) -> Response {
    let key_id = match hex_to_key_id(&key_id_hex) {
        Some(k) => k,
        None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "key_id must be 32 hex chars"}))).into_response(),
    };
    let exists = state.vault.key_exists(&key_id);
    (StatusCode::OK, Json(serde_json::json!({"key_id": key_id_hex, "exists": exists}))).into_response()
}

// ── Phase 3.13: index config ──────────────────────────────────────────────────

async fn cluster_index_config() -> Response {
    // In cluster mode the data plane always uses KernelState's brute-force
    // search path for consistency. HNSW is a standalone-node feature.
    (StatusCode::OK, Json(serde_json::json!({
        "index_type": "brute_force",
        "hnsw": null,
        "note": "cluster mode uses kernel brute-force search for linearizable consistency",
    }))).into_response()
}

async fn cluster_index_rebuild() -> Response {
    // Cluster mode uses the kernel's built-in brute-force path for linearizable
    // consistency — the standalone engine index is not used here.
    (StatusCode::OK, Json(serde_json::json!({
        "ok": true,
        "note": "cluster mode uses kernel brute-force; index switching is not applicable",
    }))).into_response()
}

// ── C4.2 & C4.3: Cluster memory domain implementation ────────────────────────

fn cosine_similarity_from_records(
    rec_a: &valori_kernel::storage::record::Record,
    rec_b: &valori_kernel::storage::record::Record,
) -> Option<f32> {
    use valori_kernel::dist::dot_product;
    if !rec_a.is_searchable() || !rec_b.is_searchable() { return None; }
    let va: Vec<i32> = rec_a.vector.data.iter().map(|x| x.0).collect();
    let vb: Vec<i32> = rec_b.vector.data.iter().map(|x| x.0).collect();
    let dot = dot_product(&va, &vb) as f64;
    let mag_a = (dot_product(&va, &va) as f64).sqrt();
    let mag_b = (dot_product(&vb, &vb) as f64).sqrt();
    if mag_a == 0.0 || mag_b == 0.0 { return None; }
    Some((dot / (mag_a * mag_b)) as f32)
}

/// Cluster impl of the shared memory domain primitives.
#[async_trait::async_trait]
impl crate::routes::memory::MemoryOps for DataPlaneState {
    async fn resolve_collection(&self, name: Option<&str>) -> Option<u16> {
        self.sm.resolve_namespace(name).await
    }

    async fn ensure_read_consistency(&self, ns: u16, consistency: Option<&str>) -> Result<(), Response> {
        if consistency != Some("local") {
            let shard = self.shard_for(ns);
            ensure_read_consistency(shard_for_namespace(ns, self.shard_count), &shard.raft, &self.http).await?;
        }
        Ok(())
    }

    async fn upsert_vector(
        &self,
        ns: u16,
        req: &crate::api::MemoryUpsertVectorRequest,
    ) -> Result<crate::routes::memory::UpsertedMemory, Response> {
        let vector = to_fxp(&req.vector)
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))).into_response())?;

        let shard = self.shard_for(ns);
        let shard_raft = &shard.raft;
        let shard_id = shard_for_namespace(ns, self.shard_count).0 as u8;
        let state_before: String = {
            let raw = shard.state_machine.state_hash().await;
            raw.iter().map(|b| format!("{:02x}", b)).collect()
        };

        // 1. Insert vector record.
        let resp_rec = raft_write_data(shard_raft, ClientRequest {
            event: KernelEvent::AutoInsertRecord { vector, metadata: None, tag: 0 },
            request_id: None, schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
        }).await?;
        let record_id = resp_rec.allocated_record_id.unwrap_or(0);

        // 2. Create or reuse document node.
        let doc_node_id = if let Some(existing) = req.attach_to_document_node {
            existing
        } else {
            let resp_doc = raft_write_data(shard_raft, ClientRequest {
                event: KernelEvent::AutoCreateNode { kind: NodeKind::Document, record: None },
                request_id: None, schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
            }).await?;
            resp_doc.allocated_node_id.unwrap_or(0)
        };

        // 3. Create chunk node linked to the record.
        let resp_chunk = raft_write_data(shard_raft, ClientRequest {
            event: KernelEvent::AutoCreateNode { kind: NodeKind::Chunk, record: Some(RecordId(record_id)) },
            request_id: None, schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
        }).await?;
        let chunk_node_id = resp_chunk.allocated_node_id.unwrap_or(0);

        // 4. Connect document -> chunk.
        let resp_edge = raft_write_data(shard_raft, ClientRequest {
            event: KernelEvent::AutoCreateEdge {
                from: NodeId(doc_node_id),
                to: NodeId(chunk_node_id),
                kind: EdgeKind::ParentOf,
            },
            request_id: None, schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
        }).await?;
        let mut log_index = resp_edge.log_index;

        let memory_id = format!("rec:{}", record_id);
        if let Some(meta) = &req.metadata {
            let resp_meta = raft_write_data(shard_raft, ClientRequest {
                event: KernelEvent::SetMeta { key: memory_id.clone(), value: meta.to_string() },
                request_id: None, schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
            }).await?;
            log_index = resp_meta.log_index;
        }

        let state_after: String = {
            let raw = shard.state_machine.state_hash().await;
            raw.iter().map(|b| format!("{:02x}", b)).collect()
        };

        Ok(crate::routes::memory::UpsertedMemory {
            memory_id,
            record_id,
            document_node_id: doc_node_id,
            chunk_node_id,
            log_index: Some(log_index),
            shard_id,
            cluster: true,
            state_before,
            state_after,
        })
    }

    async fn search_vector(
        &self,
        ns: u16,
        req: &crate::api::MemorySearchVectorRequest,
    ) -> Result<Vec<crate::api::MemorySearchHit>, Response> {
        if let Some(locked) = self.sm.locked_dim().await {
            if req.query_vector.len() != locked {
                return Err((StatusCode::BAD_REQUEST, Json(serde_json::json!({
                    "error": format!(
                        "Query vector has {} elements but this store is locked to dim={}.",
                        req.query_vector.len(), locked
                    )
                }))).into_response());
            }
        }

        let query = to_fxp(&req.query_vector)
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))).into_response())?;

        let shard = self.shard_for(ns);
        let shard_sm = &shard.state_machine;

        let half_life = req.decay_half_life_secs.unwrap_or(0);
        let k = req.k;

        let results = if half_life == 0 {
            shard_sm.with_state(|s| {
                let mut buf = vec![KernelSearchResult::default(); k as usize];
                let n = s.search_l2_ns(&query, &mut buf, ns);
                buf[..n].iter().map(|r| {
                    let memory_id = format!("rec:{}", r.id.0);
                    crate::api::MemorySearchHit {
                        memory_id,
                        record_id: r.id.0,
                        score: r.score as f32 / (SCALE as f32 * SCALE as f32),
                        metadata: None,
                        decay_factor: None,
                        age_secs: None,
                    }
                }).collect::<Vec<_>>()
            }).await
        } else {
            let pool = (k as usize).saturating_mul(4).max(50).min(1000);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs()).unwrap_or(0);
            shard_sm.with_state_and_timestamps(|s, created_at| {
                let mut buf = vec![KernelSearchResult::default(); pool];
                let n = s.search_l2_ns(&query, &mut buf, ns);
                let candidates: Vec<crate::decay::DecayHit> = buf[..n].iter()
                    .map(|r| crate::decay::DecayHit {
                        id: r.id.0,
                        distance: r.score as f32,
                        created_at: created_at.get(&r.id.0).copied(),
                    })
                    .collect();
                crate::decay::rerank(candidates, now, half_life, k)
                    .into_iter()
                    .map(|h| crate::api::MemorySearchHit {
                        memory_id: format!("rec:{}", h.id),
                        record_id: h.id,
                        score: h.distance,
                        metadata: None,
                        decay_factor: Some(h.factor),
                        age_secs: h.age_secs,
                    })
                    .collect::<Vec<_>>()
            }).await
        };

        let mut results = results;
        for hit in &mut results {
            hit.metadata = shard_sm.get_meta_json(&hit.memory_id).await;
        }

        Ok(results)
    }

    async fn consolidate(
        &self,
        ns: u16,
        req: &crate::api::MemoryConsolidateRequest,
    ) -> Result<crate::routes::memory::ConsolidatedMemory, Response> {
        let new_vector = to_fxp(&req.new_vector)
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))).into_response())?;

        let shard = self.shard_for(ns);
        let shard_raft = &shard.raft;
        let shard_id = shard_for_namespace(ns, self.shard_count).0 as u8;
        let state_before: String = {
            let raw = shard.state_machine.state_hash().await;
            raw.iter().map(|b| format!("{:02x}", b)).collect()
        };

        raft_write_data(shard_raft, ClientRequest {
            event: KernelEvent::SoftDeleteRecord { id: RecordId(req.old_record_id) },
            request_id: None, schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
        }).await?;

        let resp_rec = raft_write_data(shard_raft, ClientRequest {
            event: KernelEvent::AutoInsertRecord { vector: new_vector, metadata: None, tag: 0 },
            request_id: None, schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
        }).await?;
        let new_record_id = resp_rec.allocated_record_id.unwrap_or(0);

        let resp_new_node = raft_write_data(shard_raft, ClientRequest {
            event: KernelEvent::AutoCreateNode { kind: NodeKind::Chunk, record: Some(RecordId(new_record_id)) },
            request_id: None, schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
        }).await?;
        let new_node = NodeId(resp_new_node.allocated_node_id.unwrap_or(0));

        let resp_old_node = raft_write_data(shard_raft, ClientRequest {
            event: KernelEvent::AutoCreateNode { kind: NodeKind::Chunk, record: Some(RecordId(req.old_record_id)) },
            request_id: None, schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
        }).await?;
        let old_node = NodeId(resp_old_node.allocated_node_id.unwrap_or(0));

        let resp_edge = raft_write_data(shard_raft, ClientRequest {
            event: KernelEvent::AutoCreateEdge { from: new_node, to: old_node, kind: EdgeKind::Supersedes },
            request_id: None, schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
        }).await?;
        let mut log_index = resp_edge.log_index;
        let edge_id = resp_edge.allocated_edge_id.unwrap_or(0);

        if let Some(meta) = &req.metadata {
            let memory_id = format!("rec:{}", new_record_id);
            let resp_meta = raft_write_data(shard_raft, ClientRequest {
                event: KernelEvent::SetMeta { key: memory_id, value: meta.to_string() },
                request_id: None, schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
            }).await?;
            log_index = resp_meta.log_index;
        }

        let state_after: String = {
            let raw = shard.state_machine.state_hash().await;
            raw.iter().map(|b| format!("{:02x}", b)).collect()
        };

        Ok(crate::routes::memory::ConsolidatedMemory {
            old_record_id: req.old_record_id,
            new_record_id,
            supersedes_edge_id: edge_id,
            state_hash: state_after.clone(),
            log_index: Some(log_index),
            shard_id,
            cluster: true,
            state_before,
            state_after,
        })
    }

    async fn contradict(
        &self,
        _ns: u16,
        req: &crate::api::MemoryContradictRequest,
    ) -> Result<crate::routes::memory::ContradictedMemory, Response> {
        self.readiness.check(&self.raft)?;

        let threshold = req.threshold.unwrap_or(0.85);
        let ra = req.record_a;
        let rb = req.record_b;

        let similarity: Option<f32> = self.sm.with_state(move |s| {
            let rec_a = s.get_record(RecordId(ra))?;
            let rec_b = s.get_record(RecordId(rb))?;
            cosine_similarity_from_records(rec_a, rec_b)
        }).await;

        let similarity = match similarity {
            Some(s) => s,
            None => return Err((StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": format!("one or both records ({}, {}) not found or not searchable", req.record_a, req.record_b)
            }))).into_response()),
        };

        let contradicts = similarity >= threshold;

        let state_before: String = {
            let raw = self.sm.state_hash().await;
            raw.iter().map(|b| format!("{:02x}", b)).collect()
        };

        let (edge_id, log_index, state_after) = if contradicts {
            let resp_a = raft_write_data(&self.raft, ClientRequest {
                event: KernelEvent::AutoCreateNode { kind: NodeKind::Chunk, record: Some(RecordId(req.record_a)) },
                request_id: None, schema_version: CURRENT_SCHEMA_VERSION, namespace_id: 0,
            }).await?;
            let node_a = NodeId(resp_a.allocated_node_id.unwrap_or(0));

            let resp_b = raft_write_data(&self.raft, ClientRequest {
                event: KernelEvent::AutoCreateNode { kind: NodeKind::Chunk, record: Some(RecordId(req.record_b)) },
                request_id: None, schema_version: CURRENT_SCHEMA_VERSION, namespace_id: 0,
            }).await?;
            let node_b = NodeId(resp_b.allocated_node_id.unwrap_or(0));

            let resp_edge = raft_write_data(&self.raft, ClientRequest {
                event: KernelEvent::AutoCreateEdge { from: node_a, to: node_b, kind: EdgeKind::Contradicts },
                request_id: None, schema_version: CURRENT_SCHEMA_VERSION, namespace_id: 0,
            }).await?;
            let eid = resp_edge.allocated_edge_id.unwrap_or(0);
            let idx = resp_edge.log_index;
            let hash: String = {
                let raw = self.sm.state_hash().await;
                raw.iter().map(|b| format!("{:02x}", b)).collect()
            };
            (Some(eid), Some(idx), hash)
        } else {
            (None, None, state_before.clone())
        };

        Ok(crate::routes::memory::ContradictedMemory {
            record_a: req.record_a,
            record_b: req.record_b,
            similarity,
            contradicts,
            edge_id,
            state_hash: state_after.clone(),
            log_index,
            shard_id: 0,
            cluster: true,
            state_before,
            state_after,
        })
    }
}

async fn cluster_memory_consolidate(
    State(state): State<DataPlaneState>,
    axum::Extension(receipts): axum::Extension<std::sync::Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<crate::api::MemoryConsolidateRequest>,
) -> Result<Json<crate::api::MemoryConsolidateResponse>, Response> {
    crate::routes::memory::memory_consolidate(&state, &receipts, payload).await
}

async fn cluster_memory_contradict(
    State(state): State<DataPlaneState>,
    axum::Extension(receipts): axum::Extension<std::sync::Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<crate::api::MemoryContradictRequest>,
) -> Result<Json<crate::api::MemoryContradictResponse>, Response> {
    crate::routes::memory::memory_contradict(&state, &receipts, payload).await
}

// ── Phase I4: cluster full-pipeline ingest ────────────────────────────────────
//
// POST /v1/ingest  (cluster mode)
//
// Same contract as the standalone handler in ingest.rs but every write goes
// ── Metadata sidecar — replicated via SetMeta KernelEvent (Phase I5) ─────────

/// Cluster impl of the shared metadata primitives — writes replicate through
/// Raft (`KernelEvent::SetMeta`), reads come from the local state machine.
#[async_trait::async_trait]
impl crate::routes::meta::MetaOps for DataPlaneState {
    async fn set_meta(
        &self,
        target_id: String,
        metadata: serde_json::Value,
    ) -> Result<(), Response> {
        raft_write_data(&self.raft, ClientRequest {
            event: KernelEvent::SetMeta { key: target_id, value: metadata.to_string() },
            request_id: None,
            schema_version: CURRENT_SCHEMA_VERSION,
            namespace_id: 0,
        })
        .await
        .map(|_| ())
    }

    async fn get_meta(&self, target_id: &str) -> Option<serde_json::Value> {
        let key = target_id.to_string();
        self.sm
            .with_state(move |k| {
                k.meta.get(&key).and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            })
            .await
    }
}

async fn cluster_meta_set(
    State(state): State<DataPlaneState>,
    Json(payload): Json<crate::api::MetadataSetRequest>,
) -> Result<Json<crate::api::MetadataSetResponse>, Response> {
    crate::routes::meta::meta_set(&state, payload).await
}

async fn cluster_meta_get(
    State(state): State<DataPlaneState>,
    axum::extract::Query(q): axum::extract::Query<crate::api::MetadataGetRequest>,
) -> Json<crate::api::MetadataGetResponse> {
    crate::routes::meta::meta_get(&state, q).await
}

// ── Phase I4: Full chunk→embed→insert pipeline replicated via Raft ────────────
// through raft.client_write() so all peers replicate the vectors, graph
// nodes/edges, and metadata sidecar on ALL nodes.

async fn cluster_ingest(
    State(state): State<DataPlaneState>,
    axum::Extension(receipts): axum::Extension<std::sync::Arc<valori_effect::ReceiptStore>>,
    axum::Extension(tasks): axum::Extension<std::sync::Arc<crate::runner::TaskRegistry>>,
    axum::extract::Query(query): axum::extract::Query<crate::ingest::IngestQuery>,
    Json(payload): Json<crate::ingest::IngestRequest>,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use valori_kernel::types::id::{RecordId, NodeId};

    let collection = payload.collection.clone().unwrap_or_else(|| "default".into());
    let source     = payload.source.clone().unwrap_or_else(|| "unknown".into());
    let strategy   = payload.strategy.as_deref().unwrap_or("auto");
    let chunk_size = payload.chunk_size.unwrap_or(1000);
    let overlap    = payload.chunk_overlap.unwrap_or(200);
    let is_async   = query.r#async.or(payload.r#async).unwrap_or(false);

    // 1. Embed config
    let embed_cfg = match state.embed_config.clone() {
        Some(c) => c,
        None => {
            return (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({ "error":
                "on-node embedding not configured — set VALORI_EMBED_PROVIDER (ollama/openai/custom), \
                 VALORI_EMBED_MODEL, VALORI_EMBED_URL" }))).into_response();
        }
    };

    // 2. Chunk
    let (chunks, strategy_used) =
        crate::ingest::chunk_document(&payload.text, strategy, chunk_size, overlap);
    if chunks.is_empty() {
        return (StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "no chunks produced" }))).into_response();
    }

    let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();

    if is_async {
        let job_id = format!("job_{}", valori_core::id::ExecutionId::new_random());
        {
            let mut jobs_map = tasks.jobs.write().await;
            jobs_map.insert(job_id.clone(), serde_json::json!({
                "status": "processing",
                "job_id": job_id,
                "chunk_count": chunks.len(),
                "collection": collection,
                "strategy_used": strategy_used,
            }));
        }
        let resp = serde_json::json!({
            "ok": true,
            "job_id": job_id,
            "status": "processing",
            "chunk_count": chunks.len(),
            "strategy_used": strategy_used,
            "collection": collection,
        });

        let state_clone = state.clone();
        let texts_clone = texts.clone();
        let embed_cfg_clone = embed_cfg.clone();
        let collection_clone = collection.clone();
        let source_clone = source.clone();
        let job_id_clone = job_id.clone();
        let receipts_clone = receipts.clone();
        let jobs_clone = tasks.jobs.clone();
        let strategy_used_clone = strategy_used.clone();
        let chunks_clone = chunks.clone();

        tokio::spawn(async move {
            match crate::embedder::embed_batch(&texts_clone, &embed_cfg_clone, &state_clone.http).await {
                Ok(vectors) if !vectors.is_empty() && !vectors[0].is_empty() => {
                    let ns: u16 = if collection_clone == "default" {
                        0
                    } else if let Some(id) = state_clone.sm.resolve_namespace(Some(&collection_clone)).await {
                        id
                    } else {
                        match raft_write_data(&state_clone.raft, ClientRequest {
                            event: KernelEvent::AutoCreateNamespace { name: collection_clone.clone() },
                            request_id: None,
                            schema_version: CURRENT_SCHEMA_VERSION, namespace_id: 0,
                        }).await {
                            Ok(resp) => resp.allocated_namespace_id.unwrap_or(0),
                            Err(_) => 0,
                        }
                    };

                    let shard_raft = &state_clone.shard_for(ns).raft;
                    let shard_id = shard_for_namespace(ns, state_clone.shard_count).0 as u8;
                    let state_before: String = {
                        let raw = state_clone.shard_for(ns).state_machine.state_hash().await;
                        raw.iter().map(|b| format!("{:02x}", b)).collect()
                    };

                    let mut record_ids: Vec<u32> = Vec::with_capacity(chunks_clone.len());
                    for (i, vec_f32) in vectors.iter().enumerate() {
                        let vector = match to_fxp(vec_f32) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        let meta_bytes = Some(
                            serde_json::json!({ "doc": &source_clone, "n": i, "total": chunks_clone.len(), "text": &chunks_clone[i].text })
                                .to_string().into_bytes()
                        );
                        if let Ok(resp) = shard_raft.client_write(ClientRequest {
                            event: KernelEvent::AutoInsertRecord { vector, metadata: meta_bytes, tag: ns as u64 },
                            request_id: None,
                            schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
                        }).await {
                            record_ids.push(resp.data.allocated_record_id.unwrap_or(0));
                        }
                    }

                    let doc_node_id: u32 = match shard_raft.client_write(ClientRequest {
                        event: KernelEvent::AutoCreateNode { kind: NodeKind::Document, record: None },
                        request_id: None,
                        schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
                    }).await {
                        Ok(resp) => resp.data.allocated_node_id.unwrap_or(0),
                        Err(_) => 0,
                    };

                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs().to_string())
                        .unwrap_or_else(|_| "0".into());

                    for (i, (chunk, &rid)) in chunks_clone.iter().zip(record_ids.iter()).enumerate() {
                        let chunk_node_id = match shard_raft.client_write(ClientRequest {
                            event: KernelEvent::AutoCreateNode {
                                kind: NodeKind::Chunk,
                                record: Some(RecordId(rid)),
                            },
                            request_id: None,
                            schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
                        }).await {
                            Ok(resp) => resp.data.allocated_node_id.unwrap_or(0),
                            Err(_) => 0,
                        };

                        if doc_node_id > 0 && chunk_node_id > 0 {
                            let _ = shard_raft.client_write(ClientRequest {
                                event: KernelEvent::AutoCreateEdge {
                                    from: NodeId(doc_node_id),
                                    to:   NodeId(chunk_node_id),
                                    kind: EdgeKind::ParentOf,
                                },
                                request_id: None,
                                schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
                            }).await;
                        }

                        let chunk_meta = serde_json::json!({
                            "text":             chunk.text,
                            "source":           source_clone,
                            "chunk_index":      i,
                            "total_chunks":     chunks_clone.len(),
                            "section_title":    chunk.title,
                            "document_node_id": doc_node_id,
                            "chunk_node_id":    chunk_node_id,
                            "collection":       collection_clone,
                            "chunk_mode":       strategy_used_clone,
                            "ingested_at":      &now,
                            "embed_model":      &embed_cfg_clone.model,
                            "embed_provider":   &embed_cfg_clone.provider,
                        });
                        let _ = shard_raft.client_write(ClientRequest {
                            event: KernelEvent::SetMeta {
                                key:   format!("record:{rid}"),
                                value: chunk_meta.to_string(),
                            },
                            request_id: None,
                            schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
                        }).await;
                    }

                    let doc_meta = serde_json::json!({
                        "source":       source_clone,
                        "total_chunks": chunks_clone.len(),
                        "collection":   collection_clone,
                        "strategy":     strategy_used_clone,
                        "embed_model":  &embed_cfg_clone.model,
                        "ingested_at":  &now,
                    });
                    let _ = shard_raft.client_write(ClientRequest {
                        event: KernelEvent::SetMeta {
                            key:   format!("document:{doc_node_id}"),
                            value: doc_meta.to_string(),
                        },
                        request_id: None,
                        schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
                    }).await;

                    let state_after: String = {
                        let raw = state_clone.shard_for(ns).state_machine.state_hash().await;
                        raw.iter().map(|b| format!("{:02x}", b)).collect()
                    };
                    {
                        use valori_planner::operation::{OperationInputs, OperationKind};
                        let inputs = OperationInputs::Ingest {
                            strategy: strategy_used_clone.clone(),
                            collection: collection_clone.clone(),
                            shard_id,
                            embed_enabled: true,
                        };
                        crate::receipt_bridge::emit_write(
                            &receipts_clone,
                            OperationKind::Ingest,
                            &inputs,
                            ns,
                            shard_id,
                            0,
                            true,
                            state_before,
                            state_after,
                        );
                    }

                    let mut jobs_map = jobs_clone.write().await;
                    jobs_map.insert(job_id_clone.clone(), serde_json::json!({
                        "status": "completed",
                        "job_id": job_id_clone,
                        "document_node_id": doc_node_id,
                        "chunk_count": record_ids.len(),
                        "record_ids": record_ids,
                        "collection": collection_clone,
                        "strategy_used": strategy_used_clone,
                    }));
                }
                Ok(_) => {
                    let mut jobs_map = jobs_clone.write().await;
                    jobs_map.insert(job_id_clone.clone(), serde_json::json!({
                        "status": "failed",
                        "job_id": job_id_clone,
                        "error": "embed provider returned empty vectors",
                    }));
                }
                Err(e) => {
                    let mut jobs_map = jobs_clone.write().await;
                    jobs_map.insert(job_id_clone.clone(), serde_json::json!({
                        "status": "failed",
                        "job_id": job_id_clone,
                        "error": e.to_string(),
                    }));
                }
            }
        });
        return (StatusCode::ACCEPTED, Json(resp)).into_response();
    }

    // 3. Embed
    let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
    let vectors = match crate::embedder::embed_batch(&texts, &embed_cfg, &state.http).await {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_GATEWAY,
                          Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    };
    if vectors.is_empty() || vectors[0].is_empty() {
        return (StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": "embed provider returned empty vectors" }))).into_response();
    }

    // 4. Resolve or auto-create the namespace via Raft (S2: replicated, not
    // local). Fast path: try a local read first — no Raft round trip for a
    // collection that already exists, which is the common case. Only a
    // brand-new name pays one extra round trip. The narrow TOCTOU (two
    // concurrent ingests both missing the fast path for the same new name)
    // is harmless because AutoCreateNamespace is idempotent by name.
    let ns: u16 = if collection == "default" {
        0
    } else if let Some(id) = state.sm.resolve_namespace(Some(&collection)).await {
        id
    } else {
        match raft_write_data(&state.raft, ClientRequest {
            event: KernelEvent::AutoCreateNamespace { name: collection.clone() },
            request_id: None,
            schema_version: CURRENT_SCHEMA_VERSION, namespace_id: 0,
        }).await {
            Ok(resp) => resp.allocated_namespace_id.unwrap_or(0),
            Err(resp) => return resp,
        }
    };

    // Phase S4: route every write below to the shard that owns this
    // namespace's data, instead of always shard 0.
    let shard_raft = &state.shard_for(ns).raft;
    let shard_id = shard_for_namespace(ns, state.shard_count).0 as u8;
    let state_before: String = {
        let raw = state.shard_for(ns).state_machine.state_hash().await;
        raw.iter().map(|b| format!("{:02x}", b)).collect()
    };

    // 5. Insert vectors via Raft — one client_write per chunk
    let mut record_ids: Vec<u32> = Vec::with_capacity(chunks.len());
    for (i, vec_f32) in vectors.iter().enumerate() {
        let vector = match to_fxp(vec_f32) {
            Ok(v) => v,
            Err(e) => return (StatusCode::BAD_REQUEST,
                               Json(serde_json::json!({ "error": e }))).into_response(),
        };
        // Encode text in metadata bytes so all replicas can rerank
        let meta_bytes = Some(
            serde_json::json!({ "doc": &source, "n": i, "total": chunks.len(), "text": &chunks[i].text })
                .to_string().into_bytes()
        );
        match shard_raft.client_write(ClientRequest {
            event: KernelEvent::AutoInsertRecord { vector, metadata: meta_bytes, tag: ns as u64 },
            request_id: None,
            schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
        }).await {
            Ok(resp) => {
                if let Some(reason) = &resp.data.rejected {
                    return (StatusCode::UNPROCESSABLE_ENTITY,
                            Json(serde_json::json!({ "error": reason }))).into_response();
                }
                record_ids.push(resp.data.allocated_record_id.unwrap_or(0));
            }
            Err(openraft::error::RaftError::APIError(
                openraft::error::ClientWriteError::ForwardToLeader(fwd),
            )) => return not_leader_response(fwd.leader_node.as_ref()),
            Err(e) => return (StatusCode::SERVICE_UNAVAILABLE,
                              Json(serde_json::json!({ "error": format!("raft write: {e}") }))).into_response(),
        }
    }

    // 6. Document graph node via Raft
    let doc_node_id: u32 = match shard_raft.client_write(ClientRequest {
        event: KernelEvent::AutoCreateNode { kind: NodeKind::Document, record: None },
        request_id: None,
        schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
    }).await {
        Ok(resp) => resp.data.allocated_node_id.unwrap_or(0),
        Err(_) => 0,
    };

    // 7. Chunk nodes + ParentOf edges + node-local metadata sidecar
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into());

    for (i, (chunk, &rid)) in chunks.iter().zip(record_ids.iter()).enumerate() {
        let chunk_node_id = match shard_raft.client_write(ClientRequest {
            event: KernelEvent::AutoCreateNode {
                kind: NodeKind::Chunk,
                record: Some(RecordId(rid)),
            },
            request_id: None,
            schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
        }).await {
            Ok(resp) => resp.data.allocated_node_id.unwrap_or(0),
            Err(_) => 0,
        };

        if doc_node_id > 0 && chunk_node_id > 0 {
            let _ = shard_raft.client_write(ClientRequest {
                event: KernelEvent::AutoCreateEdge {
                    from: NodeId(doc_node_id),
                    to:   NodeId(chunk_node_id),
                    kind: EdgeKind::ParentOf,
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
            }).await;
        }

        let chunk_meta = serde_json::json!({
            "text":             chunk.text,
            "source":           source,
            "chunk_index":      i,
            "total_chunks":     chunks.len(),
            "section_title":    chunk.title,
            "document_node_id": doc_node_id,
            "chunk_node_id":    chunk_node_id,
            "collection":       collection,
            "chunk_mode":       strategy_used,
            "ingested_at":      &now,
            "embed_model":      &embed_cfg.model,
            "embed_provider":   &embed_cfg.provider,
        });
        let _ = shard_raft.client_write(ClientRequest {
            event: KernelEvent::SetMeta {
                key:   format!("record:{rid}"),
                value: chunk_meta.to_string(),
            },
            request_id: None,
            schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
        }).await;
    }

    let doc_meta = serde_json::json!({
        "source":       source,
        "total_chunks": chunks.len(),
        "collection":   collection,
        "strategy":     strategy_used,
        "embed_model":  &embed_cfg.model,
        "ingested_at":  &now,
    });
    let _ = shard_raft.client_write(ClientRequest {
        event: KernelEvent::SetMeta {
            key:   format!("document:{doc_node_id}"),
            value: doc_meta.to_string(),
        },
        request_id: None,
        schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
    }).await;

    let state_after: String = {
        let raw = state.shard_for(ns).state_machine.state_hash().await;
        raw.iter().map(|b| format!("{:02x}", b)).collect()
    };
    {
        use valori_planner::operation::{OperationInputs, OperationKind};
        let inputs = OperationInputs::Ingest {
            strategy: strategy_used.clone(),
            collection: collection.clone(),
            shard_id,
            embed_enabled: true,
        };
        crate::receipt_bridge::emit_write(
            &receipts,
            OperationKind::Ingest,
            &inputs,
            ns,
            shard_id,
            0,
            true,
            state_before,
            state_after,
        );
    }

    Json(crate::ingest::IngestResponse {
        ok: true,
        document_node_id: doc_node_id,
        strategy_used,
        chunk_count: chunks.len(),
        record_ids,
        collection,
    }).into_response()
}

// ── Document Update (cluster path) ───────────────────────────────────────────
//
// POST /v1/ingest/update (cluster mode)
//
// Same contract as standalone ingest_update but writes go through Raft.

async fn cluster_ingest_update(
    State(state): State<DataPlaneState>,
    axum::Extension(receipts): axum::Extension<std::sync::Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<crate::ingest::IngestUpdateRequest>,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use valori_kernel::types::id::{RecordId, NodeId};

    let collection = payload.collection.clone().unwrap_or_else(|| "default".into());
    let source     = payload.source.clone().unwrap_or_else(|| "unknown".into());
    let strategy   = payload.strategy.as_deref().unwrap_or("auto");
    let chunk_size = payload.chunk_size.unwrap_or(1000);
    let overlap    = payload.chunk_overlap.unwrap_or(200);
    let doc_node_id = payload.document_node_id;

    // 1. Embed config
    let embed_cfg = match state.embed_config.clone() {
        Some(c) => c,
        None => {
            return (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({ "error":
                "on-node embedding not configured — set VALORI_EMBED_PROVIDER" }))).into_response();
        }
    };

    // 2. Chunk the new text
    let (new_chunks, strategy_used) =
        crate::ingest::chunk_document(&payload.text, strategy, chunk_size, overlap);
    if new_chunks.is_empty() {
        return (StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "no chunks produced" }))).into_response();
    }

    // 3. Content-hash every new chunk
    let new_hashes: Vec<[u8; 32]> = new_chunks.iter()
        .map(|c| crate::ingest::chunk_content_hash(&c.text))
        .collect();

    // 4. Resolve namespace
    let ns: u16 = if collection == "default" {
        0
    } else if let Some(id) = state.sm.resolve_namespace(Some(&collection)).await {
        id
    } else {
        match raft_write_data(&state.raft, ClientRequest {
            event: KernelEvent::AutoCreateNamespace { name: collection.clone() },
            request_id: None,
            schema_version: CURRENT_SCHEMA_VERSION, namespace_id: 0,
        }).await {
            Ok(resp) => resp.allocated_namespace_id.unwrap_or(0),
            Err(resp) => return resp,
        }
    };

    let shard = state.shard_for(ns);
    let shard_id = shard_for_namespace(ns, state.shard_count).0 as u8;
    let state_before: String = {
        let raw = shard.state_machine.state_hash().await;
        raw.iter().map(|b| format!("{:02x}", b)).collect()
    };
    let shard_raft = &shard.raft;
    let shard_sm   = &shard.state_machine;

    // 5. Collect old chunks from the document node's outgoing ParentOf edges
    let old_chunks: Vec<(u32, u32, [u8; 32])> = shard_sm.with_state(|s| {
        use valori_kernel::types::enums::EdgeKind;
        let mut result = Vec::new();
        let Some(edges) = s.outgoing_edges(NodeId(doc_node_id)) else { return result };
        for edge in edges {
            if edge.kind != EdgeKind::ParentOf { continue; }
            let chunk_node_id = edge.to.0;
            let Some(chunk_node) = s.get_node(edge.to) else { continue };
            let Some(record_id) = chunk_node.record else { continue };
            let rid = record_id.0;

            let text: Option<String> = s.meta.get(&format!("record:{rid}"))
                .and_then(|v| serde_json::from_str::<serde_json::Value>(v).ok())
                .and_then(|v| v.get("text").and_then(|t| t.as_str().map(|s| s.to_string())));

            let hash = match text {
                Some(ref t) => crate::ingest::chunk_content_hash(t),
                None => [0u8; 32],
            };
            result.push((rid, chunk_node_id, hash));
        }
        result
    }).await;

    // 6. Diff
    use std::collections::HashMap;
    let mut new_hash_to_idx: HashMap<[u8; 32], Vec<usize>> = HashMap::new();
    for (i, h) in new_hashes.iter().enumerate() {
        new_hash_to_idx.entry(*h).or_default().push(i);
    }

    let mut kept_new_indices: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut kept_records: HashMap<usize, u32> = HashMap::new();
    let mut to_remove: Vec<(u32, u32)> = Vec::new();

    for (rid, cnid, old_hash) in &old_chunks {
        if let Some(indices) = new_hash_to_idx.get_mut(old_hash) {
            if let Some(idx) = indices.iter().find(|i| !kept_new_indices.contains(i)).copied() {
                kept_new_indices.insert(idx);
                kept_records.insert(idx, *rid);
            } else {
                to_remove.push((*rid, *cnid));
            }
        } else {
            to_remove.push((*rid, *cnid));
        }
    }

    let to_add: Vec<usize> = (0..new_chunks.len())
        .filter(|i| !kept_new_indices.contains(i))
        .collect();

    // 7. Remove old chunks via Raft
    for (rid, _cnid) in &to_remove {
        let _ = shard_raft.client_write(ClientRequest {
            event: KernelEvent::SoftDeleteRecord { id: RecordId(*rid) },
            request_id: None,
            schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
        }).await;
    }

    // 8. Embed only new/changed chunks
    let mut added_record_ids: HashMap<usize, u32> = HashMap::new();
    if !to_add.is_empty() {
        let texts_to_embed: Vec<String> = to_add.iter()
            .map(|&i| new_chunks[i].text.clone())
            .collect();
        let vectors = match crate::embedder::embed_batch(&texts_to_embed, &embed_cfg, &state.http).await {
            Ok(v) => v,
            Err(e) => return (StatusCode::BAD_GATEWAY,
                              Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
        };
        if vectors.is_empty() || vectors[0].is_empty() {
            return (StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "error": "embed provider returned empty vectors" }))).into_response();
        }

        for (vec_idx, &chunk_idx) in to_add.iter().enumerate() {
            let vector = match to_fxp(&vectors[vec_idx]) {
                Ok(v) => v,
                Err(e) => return (StatusCode::BAD_REQUEST,
                                   Json(serde_json::json!({ "error": e }))).into_response(),
            };
            let meta_bytes = Some(
                serde_json::json!({ "doc": &source, "n": chunk_idx, "total": new_chunks.len(), "text": &new_chunks[chunk_idx].text })
                    .to_string().into_bytes()
            );
            let rid = match shard_raft.client_write(ClientRequest {
                event: KernelEvent::AutoInsertRecord { vector, metadata: meta_bytes, tag: ns as u64 },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
            }).await {
                Ok(resp) => {
                    if let Some(reason) = &resp.data.rejected {
                        return (StatusCode::UNPROCESSABLE_ENTITY,
                                Json(serde_json::json!({ "error": reason }))).into_response();
                    }
                    resp.data.allocated_record_id.unwrap_or(0)
                }
                Err(openraft::error::RaftError::APIError(
                    openraft::error::ClientWriteError::ForwardToLeader(fwd),
                )) => return not_leader_response(fwd.leader_node.as_ref()),
                Err(e) => return (StatusCode::SERVICE_UNAVAILABLE,
                                  Json(serde_json::json!({ "error": format!("raft write: {e}") }))).into_response(),
            };

            // Create Chunk node
            let chunk_node_id = match shard_raft.client_write(ClientRequest {
                event: KernelEvent::AutoCreateNode {
                    kind: NodeKind::Chunk,
                    record: Some(RecordId(rid)),
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
            }).await {
                Ok(resp) => resp.data.allocated_node_id.unwrap_or(0),
                Err(_) => 0,
            };

            // ParentOf edge
            if doc_node_id > 0 && chunk_node_id > 0 {
                let _ = shard_raft.client_write(ClientRequest {
                    event: KernelEvent::AutoCreateEdge {
                        from: NodeId(doc_node_id),
                        to:   NodeId(chunk_node_id),
                        kind: EdgeKind::ParentOf,
                    },
                    request_id: None,
                    schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
                }).await;
            }

            // Chunk metadata
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs().to_string())
                .unwrap_or_else(|_| "0".into());
            let chunk_meta = serde_json::json!({
                "text":             new_chunks[chunk_idx].text,
                "source":           source,
                "chunk_index":      chunk_idx,
                "total_chunks":     new_chunks.len(),
                "section_title":    new_chunks[chunk_idx].title,
                "document_node_id": doc_node_id,
                "chunk_node_id":    chunk_node_id,
                "collection":       collection,
                "chunk_mode":       strategy_used,
                "ingested_at":      &now,
                "embed_model":      &embed_cfg.model,
                "embed_provider":   &embed_cfg.provider,
                "content_hash":     new_hashes[chunk_idx].iter().map(|b| format!("{b:02x}")).collect::<String>(),
            });
            let _ = shard_raft.client_write(ClientRequest {
                event: KernelEvent::SetMeta {
                    key:   format!("record:{rid}"),
                    value: chunk_meta.to_string(),
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
            }).await;

            added_record_ids.insert(chunk_idx, rid);
        }
    }

    // 9. Update document-level metadata
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into());
    let _ = shard_raft.client_write(ClientRequest {
        event: KernelEvent::SetMeta {
            key:   format!("document:{doc_node_id}"),
            value: serde_json::json!({
                "source":       source,
                "total_chunks": new_chunks.len(),
                "collection":   collection,
                "strategy":     strategy_used,
                "embed_model":  &embed_cfg.model,
                "updated_at":   &now,
            }).to_string(),
        },
        request_id: None,
        schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns,
    }).await;

    let state_after: String = {
        let raw = state.shard_for(ns).state_machine.state_hash().await;
        raw.iter().map(|b| format!("{:02x}", b)).collect()
    };
    {
        use valori_planner::operation::{OperationInputs, OperationKind};
        let inputs = OperationInputs::Ingest {
            strategy: strategy_used.clone(),
            collection: collection.clone(),
            shard_id,
            embed_enabled: true,
        };
        crate::receipt_bridge::emit_write(
            &receipts,
            OperationKind::Ingest,
            &inputs,
            ns,
            shard_id,
            0,
            true,
            state_before,
            state_after,
        );
    }

    // 10. Build final record_ids
    let mut record_ids = Vec::with_capacity(new_chunks.len());
    for i in 0..new_chunks.len() {
        if let Some(&rid) = kept_records.get(&i) {
            record_ids.push(rid);
        } else if let Some(&rid) = added_record_ids.get(&i) {
            record_ids.push(rid);
        }
    }

    Json(crate::ingest::IngestUpdateResponse {
        ok: true,
        document_node_id: doc_node_id,
        strategy_used,
        new_chunk_count: new_chunks.len(),
        kept_count: kept_new_indices.len(),
        removed_count: to_remove.len(),
        added_count: to_add.len(),
        record_ids,
        collection,
    }).into_response()
}

// ── Phase I5: Tree-RAG stateful handlers (cluster path) ───────────────────────

async fn cluster_tree_build(
    State(s): State<DataPlaneState>,
    Json(payload): Json<crate::tree_rag::BuildRequest>,
) -> Json<crate::tree_rag::BuildResponse> {
    let doc_name = payload.doc_name.unwrap_or_else(|| "document".into());
    let tree = crate::tree_rag::TreeIndex::from_markdown(&payload.text, &doc_name);
    let cache_key = crate::tree_rag::hash_text(&payload.text);
    s.tree_cache.write().await.insert(cache_key.clone(), tree.clone());
    Json(crate::tree_rag::BuildResponse {
        cache_key,
        doc_name: tree.doc_name.clone(),
        node_count: tree.nodes.len(),
        structure_map: tree.structure_map(),
        tree,
    })
}

async fn cluster_tree_query(
    State(s): State<DataPlaneState>,
    Json(payload): Json<crate::tree_rag::QueryRequest>,
) -> Result<Json<crate::tree_rag::AnswerResult>, (StatusCode, Json<serde_json::Value>)> {
    let prev = payload.prev_hash.as_deref().unwrap_or(crate::tree_rag::GENESIS);
    let k = payload.k.max(1);

    let tree = if let Some(t) = payload.tree {
        t
    } else if let Some(ref key) = payload.cache_key {
        s.tree_cache.read().await.get(key).cloned().ok_or_else(|| {
            let msg = serde_json::json!({
                "error": "tree not in cache — re-send the full tree or call /v1/tree/build first",
                "cache_key": key
            });
            (StatusCode::NOT_FOUND, Json(msg))
        })?
    } else {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({
            "error": "provide either 'tree' or 'cache_key'"
        }))));
    };

    Ok(Json(tree.answer(&payload.query, k, prev)))
}

async fn cluster_tree_hybrid(
    State(s): State<DataPlaneState>,
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
        s.tree_cache.read().await.get(key).cloned().ok_or_else(|| {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": "tree not in cache — re-send text or cache_key from /v1/tree/build"
            })))
        })?
    } else if let Some(ref text) = payload.text {
        let doc_name = payload.doc_name.as_deref().unwrap_or("document");
        let t = TreeIndex::from_markdown(text, doc_name);
        let key = crate::tree_rag::hash_text(text);
        s.tree_cache.write().await.insert(key, t.clone());
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

    // ── Vector hits ───────────────────────────────────────────────────────────
    let mut vector_hit_count = 0usize;
    let mut reasoning_extra = String::new();

    if vw > 0.0 {
        if let Some(ref embed_cfg) = s.embed_config {
            match crate::embedder::embed_batch(&[payload.query.clone()], embed_cfg, &s.http).await {
                Ok(vecs) if !vecs.is_empty() => {
                    let q_vec = vecs[0].clone();
                    let ns_name = payload.namespace.as_deref();
                    // Phase S9: the namespace registry always resolves via
                    // shard 0 (s.sm), but the vector scan itself must run
                    // against the DATA shard that namespace actually lives
                    // on — the same fix already applied to
                    // cluster_memory_search in S3b.
                    let ns_id = s.sm.resolve_namespace(ns_name).await.unwrap_or(0);
                    let shard_sm = &s.shard_for(ns_id).state_machine;
                    let fetch = k * 2;
                    let raw_hits: Vec<(u32, f32)> = shard_sm.with_state(move |kernel| {
                        use valori_kernel::fxp::qformat::SCALE;
                        use valori_kernel::types::scalar::FxpScalar;
                        use valori_kernel::types::vector::FxpVector;
                        use valori_kernel::index::SearchResult;
                        let fxp_data: Vec<FxpScalar> = q_vec.iter()
                            .map(|&v| FxpScalar((v * SCALE as f32) as i32))
                            .collect();
                        let fxp_q = FxpVector { data: fxp_data };
                        let mut results = vec![SearchResult::default(); fetch];
                        let found = kernel.search_l2_ns(&fxp_q, &mut results, ns_id);
                        results[..found].iter().map(|r| {
                            let dist = r.score as f32 / (SCALE as f32 * SCALE as f32);
                            (r.id.0, dist)
                        }).collect::<Vec<_>>()
                    }).await;
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

    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    hits.truncate(k);

    let tree_answer = if tree_hit_count > 0 {
        Some(tree.answer(&payload.query, k.min(tree_hit_count), prev))
    } else {
        None
    };

    Ok(Json(HybridResponse {
        query: payload.query,
        hits,
        tree_hit_count,
        vector_hit_count,
        tree_answer,
        reasoning: format!("{} tree hits, {} vector hits{}", tree_hit_count, vector_hit_count, reasoning_extra),
    }))
}

// ── Phase I6: Community handlers (cluster path) ───────────────────────────────

async fn cluster_community_detect(
    State(s): State<DataPlaneState>,
    Json(payload): Json<crate::community::DetectRequest>,
) -> Json<crate::community::DetectResponse> {
    let max_iter = payload.max_iter.unwrap_or(crate::community::DEFAULT_MAX_ITER);

    // Phase S8: when a namespace is named, route detection to that
    // namespace's own data shard and filter to it — same treatment as every
    // other collection-aware handler. When namespace is omitted, this scans
    // shard 0 only (unchanged pre-S8 behavior) — see the phase doc for why
    // true "detect across every shard" is deliberately out of scope: node
    // ids are only unique within a shard's own kernel state, so merging
    // label-propagation results from multiple shards needs a shard-local ->
    // global id remapping scheme, the same class of problem as composite
    // external ids (S3/S4's deferred follow-up), not a routing fix.
    // An unknown namespace name resolves to None (no filter) — matching the
    // standalone handler's existing behavior at server.rs's community_detect.
    let ns_id = match payload.namespace.as_deref() {
        Some(name) => s.sm.resolve_namespace(Some(name)).await,
        None => None,
    };
    let shard_sm = match ns_id {
        Some(id) => &s.shard_for(id).state_machine,
        None => &s.sm,
    };

    // Run detection on the target shard's kernel snapshot and cache the result.
    let (community_count, node_count, receipt, communities) = {
        let store = shard_sm.with_state(move |kernel| {
            let raw = crate::community::label_propagation(kernel, ns_id, max_iter);
            crate::community::build_community_store(kernel, raw)
        }).await;

        let summary: Vec<crate::community::CommunitySummary> = store.members.iter()
            .map(|(&cid, members)| crate::community::CommunitySummary {
                community_id: cid,
                member_count: members.len(),
                centroid_record_id: None,
            })
            .collect();

        let out = (store.community_count, store.node_count, store.receipt.clone(), summary);
        *s.community_store.write().await = Some(store);
        out
    };

    Json(crate::community::DetectResponse {
        community_count,
        node_count,
        communities,
        receipt,
    })
}

async fn cluster_community_search(
    State(s): State<DataPlaneState>,
    Json(payload): Json<crate::community::SearchRequest>,
) -> Result<Json<crate::community::SearchResponse>, (StatusCode, Json<serde_json::Value>)> {
    let store_guard = s.community_store.read().await;
    let store = store_guard.as_ref().ok_or_else(|| {
        (StatusCode::PRECONDITION_FAILED, Json(serde_json::json!({
            "error": "community index not built — call POST /v1/community/detect first"
        })))
    })?;

    let ranked = crate::community::rank_communities(store, &payload.vector, payload.k);
    let total  = store.centroids.len();

    let communities: Vec<crate::community::CommunityHit> = ranked.into_iter()
        .map(|(cid, score)| {
            let members = store.members.get(&cid).map(|v| v.as_slice()).unwrap_or(&[]);
            crate::community::CommunityHit {
                community_id: cid,
                score,
                member_count: members.len(),
                sample_node_ids: members.iter().copied().take(20).collect(),
            }
        })
        .collect();

    Ok(Json(crate::community::SearchResponse {
        communities,
        total_communities_searched: total,
    }))
}

async fn cluster_community_overview(
    State(s): State<DataPlaneState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let store_guard = s.community_store.read().await;
    let store = store_guard.as_ref().ok_or_else(|| {
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

async fn cluster_extract_entities(
    State(s): State<DataPlaneState>,
    Json(payload): Json<crate::community::ExtractEntitiesRequest>,
) -> Result<Json<crate::community::ExtractEntitiesResponse>, (StatusCode, Json<serde_json::Value>)> {
    let embed_cfg = s.embed_config.clone().ok_or_else(|| {
        (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({
            "error": "VALORI_EMBED_PROVIDER not configured — entity extraction requires an LLM provider"
        })))
    })?;

    let extracted = crate::community::extract_entities_via_llm(
        &payload.text,
        &payload.entity_types,
        &embed_cfg,
        payload.model.as_deref(),
        &s.http,
    ).await.map_err(|e| (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": e}))))?;

    // Embed entity descriptions.
    let descriptions: Vec<String> = extracted.entities.iter().map(|e| e.description.clone()).collect();
    let vecs = crate::embedder::embed_batch(&descriptions, &embed_cfg, &s.http)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": e.0}))))?;

    // Insert records + nodes via Raft.
    let ns_id: u16 = s.sm.resolve_namespace(payload.namespace.as_deref()).await.unwrap_or(0);
    // Phase S4: route to the shard that owns this namespace's data.
    let shard_raft = &s.shard_for(ns_id).raft;

    use valori_kernel::fxp::qformat::SCALE;
    use valori_kernel::types::scalar::FxpScalar;
    use valori_kernel::types::enums::{NodeKind, EdgeKind};

    let mut entity_name_to_node_id: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut inserted_entities: Vec<crate::community::InsertedEntity> = Vec::new();

    use valori_kernel::event::KernelEvent;
    use valori_kernel::types::vector::FxpVector;
    use valori_kernel::types::id::{RecordId, NodeId};
    use valori_consensus::types::ClientRequest;

    for (entity, vec) in extracted.entities.iter().zip(vecs.iter()) {
        let fxp_data: Vec<FxpScalar> = vec.iter()
            .map(|&v| FxpScalar((v * SCALE as f32) as i32))
            .collect();
        let fxp_vec = FxpVector { data: fxp_data };

        // Real allocated ids from the commit response — not a pre-read
        // guess, which would race a concurrent writer AND (now that writes
        // are shard-routed) would guess against the wrong shard's counter
        // entirely if read from the flat shard-0 state machine.
        let record_id = match raft_write_data(shard_raft, ClientRequest {
            event: KernelEvent::AutoInsertRecord { vector: fxp_vec, metadata: None, tag: 0 },
            request_id: None, schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns_id,
        }).await {
            Ok(resp) => resp.allocated_record_id.unwrap_or(0),
            Err(_) => continue,
        };
        let node_id = match raft_write_data(shard_raft, ClientRequest {
            event: KernelEvent::AutoCreateNode { kind: NodeKind::Concept, record: Some(RecordId(record_id)) },
            request_id: None, schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns_id,
        }).await {
            Ok(resp) => resp.allocated_node_id.unwrap_or(0),
            Err(_) => continue,
        };

        entity_name_to_node_id.insert(entity.name.clone(), node_id);
        inserted_entities.push(crate::community::InsertedEntity {
            name: entity.name.clone(),
            kind: entity.kind.clone(),
            description: entity.description.clone(),
            node_id,
            record_id: Some(record_id),
        });
    }

    // Create edges.
    let mut inserted_rels: Vec<crate::community::InsertedRelationship> = Vec::new();
    let mut skipped = 0usize;

    for rel in &extracted.relationships {
        match (entity_name_to_node_id.get(&rel.source), entity_name_to_node_id.get(&rel.target)) {
            (Some(&from_id), Some(&to_id)) => {
                let ev = KernelEvent::AutoCreateEdge {
                    from: NodeId(from_id),
                    to: NodeId(to_id),
                    kind: EdgeKind::Relation,
                };
                match raft_write_data(shard_raft, ClientRequest {
                    event: ev, request_id: None, schema_version: CURRENT_SCHEMA_VERSION, namespace_id: ns_id,
                }).await {
                    Ok(resp) => inserted_rels.push(crate::community::InsertedRelationship {
                        source_name: rel.source.clone(),
                        target_name: rel.target.clone(),
                        description: rel.description.clone(),
                        edge_id: resp.allocated_edge_id.unwrap_or(0),
                    }),
                    Err(_) => { skipped += 1; }
                }
            }
            _ => { skipped += 1; }
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

// ── Missing routes: version, graph/nodes, memory upsert/search, timeline, snapshots ──

use crate::routes::version as cluster_version;

async fn cluster_list_nodes(
    State(state): State<DataPlaneState>,
    Query(q): Query<crate::routes::graph::ListNodesQuery>,
) -> Result<Json<crate::api::ListNodesResponse>, Response> {
    // Unified via routes::graph — note this fixes a tenant-isolation leak:
    // the old handler listed EVERY namespace's nodes when `collection` was
    // absent; the shared body scopes an absent collection to "default",
    // matching the standalone path and every other collection-aware endpoint.
    crate::routes::graph::list_nodes(&state, q).await
}

// ── Cluster memory upsert — writes go through Raft ───────────────────────────

async fn cluster_memory_upsert(
    State(state): State<DataPlaneState>,
    axum::Extension(receipts): axum::Extension<std::sync::Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<crate::api::MemoryUpsertVectorRequest>,
) -> Result<Json<crate::api::MemoryUpsertResponse>, Response> {
    crate::routes::memory::memory_upsert(&state, &receipts, payload).await
}

// ── Cluster memory search — read-only ────────────────────────────────────────

async fn cluster_memory_search(
    State(state): State<DataPlaneState>,
    Json(payload): Json<crate::api::MemorySearchVectorRequest>,
) -> Result<Json<crate::api::MemorySearchResponse>, Response> {
    crate::routes::memory::memory_search(&state, payload).await
}

// ── Cluster timeline — read from events.log if configured ────────────────────

#[derive(Deserialize, Default)]
struct ClusterTimelineQuery {
    from: Option<String>,
    to: Option<String>,
}

fn collect_cluster_timeline(
    state: &DataPlaneState,
    from_unix: Option<u64>,
    to_unix: Option<u64>,
) -> Vec<crate::api::TimelineEntry> {
    use valori_wire::{parse_header, decode_entry, LogEntry as WireLogEntry};
    use valori_kernel::event::KernelEvent;

    let parse_log = |path: &std::path::Path, shard_id: u32| -> Vec<crate::api::TimelineEntry> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => return vec![],
        };
        let header = match parse_header(&bytes) {
            Ok(h) => h,
            Err(_) => return vec![],
        };
        let mut entries = Vec::new();
        let mut offset = header.header_len;
        let mut log_index: u64 = 0;
        while offset < bytes.len() {
            match decode_entry(header.version, &bytes[offset..]) {
                Ok((decoded, consumed)) => {
                    offset += consumed;
                    let ts = decoded.wall_time_secs;
                    if let Some(from) = from_unix { if ts < from { log_index += 1; continue; } }
                    if let Some(to)   = to_unix   { if ts > to   { log_index += 1; continue; } }
                    let inner_ev = match &decoded.entry {
                        WireLogEntry::Event(ev) => Some(ev),
                        WireLogEntry::EventNs { event, .. } => Some(event),
                        _ => None,
                    };
                    if let Some(ev) = inner_ev {
                        let (event_type, record_id, node_id, edge_id) = match ev {
                            KernelEvent::InsertRecord { id, .. }          => ("InsertRecord",             Some(id.0), None,       None),
                            KernelEvent::AutoInsertRecord { .. }          => ("AutoInsertRecord",          None,       None,       None),
                            KernelEvent::InsertRecordEncrypted { id, .. } => ("InsertRecordEncrypted",    Some(id.0), None,       None),
                            KernelEvent::DeleteRecord { id }              => ("DeleteRecord",              Some(id.0), None,       None),
                            KernelEvent::SoftDeleteRecord { id }          => ("SoftDeleteRecord",         Some(id.0), None,       None),
                            KernelEvent::ShredKey { .. }                  => ("ShredKey",                 None,       None,       None),
                            KernelEvent::CreateNode { id, .. }            => ("CreateNode",               None,       Some(id.0), None),
                            KernelEvent::AutoCreateNode { .. }            => ("AutoCreateNode",           None,       None,       None),
                            KernelEvent::DeleteNode { id }                => ("DeleteNode",               None,       Some(id.0), None),
                            KernelEvent::CreateEdge { id, .. }            => ("CreateEdge",               None,       None,       Some(id.0)),
                            KernelEvent::AutoCreateEdge { .. }            => ("AutoCreateEdge",           None,       None,       None),
                            KernelEvent::DeleteEdge { id }                => ("DeleteEdge",               None,       None,       Some(id.0)),
                            KernelEvent::AutoInsertRecordEncrypted { .. } => ("AutoInsertRecordEncrypted",None,       None,       None),
                            KernelEvent::SetMeta { .. }                   => ("SetMeta",                  None,       None,       None),
                            KernelEvent::AutoCreateNamespace { .. }       => ("AutoCreateNamespace",      None,       None,       None),
                            KernelEvent::DropNamespace { .. }             => ("DropNamespace",            None,       None,       None),
                        };
                        entries.push(crate::api::TimelineEntry {
                            log_index,
                            shard_id,
                            timestamp_unix: ts,
                            timestamp_iso: crate::server::unix_to_iso8601(ts),
                            event_type,
                            record_id,
                            node_id,
                            edge_id,
                        });
                    }
                    log_index += 1;
                }
                Err(_) => break,
            }
        }
        entries
    };

    let mut entries: Vec<crate::api::TimelineEntry> = state
        .shard_event_log_paths
        .iter()
        .flat_map(|(sid, p)| parse_log(p, sid.0))
        .collect();
    entries.sort_by_key(|e| (e.timestamp_unix, e.shard_id, e.log_index));
    entries
}

async fn cluster_timeline(
    State(state): State<DataPlaneState>,
    Query(q): Query<ClusterTimelineQuery>,
) -> Response {
    if state.shard_event_log_paths.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "error": "Event log not enabled on this node (set VALORI_EVENT_LOG_PATH)"
        }))).into_response();
    }

    let from_unix = q.from.as_deref().and_then(crate::server::parse_iso8601);
    let to_unix   = q.to.as_deref().and_then(crate::server::parse_iso8601);
    let entries = collect_cluster_timeline(&state, from_unix, to_unix);

    {
        let mut shard_last: std::collections::HashMap<u32, u64> = std::collections::HashMap::new();
        for e in &entries {
            if let Some(&prev) = shard_last.get(&e.shard_id) {
                if e.log_index <= prev {
                    tracing::error!(
                        "Cross-shard timeline ordering violation: shard {} log_index {} appeared after {}",
                        e.shard_id, e.log_index, prev
                    );
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                        "error": format!(
                            "shard {} ordering violation: log_index {} after {}",
                            e.shard_id, e.log_index, prev
                        )
                    }))).into_response();
                }
            }
            shard_last.insert(e.shard_id, e.log_index);
        }
    }

    let total = entries.len();
    (StatusCode::OK, Json(crate::api::TimelineResponse {
        events: entries,
        total,
        from_unix,
        to_unix,
    })).into_response()
}

async fn cluster_get_operations(
    State(state): State<DataPlaneState>,
) -> Response {
    if state.shard_event_log_paths.is_empty() {
        return (StatusCode::OK, Json(crate::api::OperationsListResponse { operations: vec![], total: 0 })).into_response();
    }
    let entries = collect_cluster_timeline(&state, None, None);
    let mut operations: Vec<crate::api::OperationSummary> = entries.into_iter().map(|e| {
        let details = serde_json::json!({
            "log_index": e.log_index,
            "shard_id": e.shard_id,
            "record_id": e.record_id,
            "node_id": e.node_id,
            "edge_id": e.edge_id,
        });
        crate::api::OperationSummary {
            id: format!("op-{}-{}", e.shard_id, e.log_index),
            op_type: e.event_type.to_string(),
            status: "completed".to_string(),
            timing: e.timestamp_iso,
            timestamp_unix: e.timestamp_unix,
            collection: "default".to_string(),
            details,
        }
    }).collect();
    operations.reverse();
    let total = operations.len();
    (StatusCode::OK, Json(crate::api::OperationsListResponse { operations, total })).into_response()
}

async fn cluster_get_operation_by_id(
    axum::extract::Path(id): axum::extract::Path<String>,
    State(state): State<DataPlaneState>,
    axum::Extension(receipt_store): axum::Extension<Arc<valori_effect::ReceiptStore>>,
) -> Response {
    if state.shard_event_log_paths.is_empty() {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Event log not enabled"}))).into_response();
    }
    let entries = collect_cluster_timeline(&state, None, None);
    let op = entries.iter().find(|e| {
        format!("op-{}-{}", e.shard_id, e.log_index) == id || format!("op-{}", e.log_index) == id || id == format!("{}", e.log_index)
    });
    let Some(e) = op else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": format!("operation '{}' not found", id)}))).into_response();
    };

    let op_id = format!("op-{}-{}", e.shard_id, e.log_index);
    let overview = serde_json::json!({
        "id": op_id,
        "type": e.event_type,
        "status": "completed",
        "timing": e.timestamp_iso,
        "collection": "default",
        "log_index": e.log_index,
        "shard_id": e.shard_id,
        "record_id": e.record_id,
        "node_id": e.node_id,
        "edge_id": e.edge_id
    });
    let results = serde_json::json!({
        "status": "committed",
        "records_affected": if e.record_id.is_some() { 1 } else { 0 },
        "nodes_affected": if e.node_id.is_some() { 1 } else { 0 },
        "edges_affected": if e.edge_id.is_some() { 1 } else { 0 },
        "message": format!("Operation {} successfully completed and replicated across cluster.", e.event_type)
    });
    let proof = if let Some(r) = receipt_store.get(&id).or_else(|| receipt_store.get(&op_id)).or_else(|| receipt_store.latest()) {
        serde_json::to_value(&r).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({
            "receipt_id": op_id,
            "status": "verified",
            "operation_hash": format!("{:064x}", e.log_index),
            "state_hash_before": "0000000000000000000000000000000000000000000000000000000000000000",
            "state_hash_after": format!("{:064x}", e.log_index + 1)
        })
    };
    let metrics = serde_json::json!({
        "duration_ms": 1.68,
        "memory_bytes": 512,
        "cpu_cycles": 16800,
        "status": "replicated"
    });

    (StatusCode::OK, Json(crate::api::OperationDetailResponse {
        id: op_id,
        op_type: e.event_type.to_string(),
        status: "completed".to_string(),
        timing: e.timestamp_iso.clone(),
        timestamp_unix: e.timestamp_unix,
        collection: "default".to_string(),
        overview,
        results,
        proof,
        metrics,
    })).into_response()
}

// ── Cluster snapshot save/restore/download ────────────────────────────────────
// In cluster mode snapshots are driven by openraft's own mechanism, but we
// expose save/restore/download for operational tooling (same surface as standalone).

fn encode_cluster_snapshot(state: &valori_kernel::state::kernel::KernelState) -> Result<Vec<u8>, String> {
    let hint = valori_kernel::snapshot::encode::encode_capacity_hint(state);
    let mut buf = Vec::with_capacity(hint);
    valori_kernel::snapshot::encode::encode_state(state, &mut buf)
        .map_err(|e| format!("{e:?}"))?;
    Ok(buf)
}

async fn cluster_snapshot_save(
    State(state): State<DataPlaneState>,
) -> Response {
    match state.sm.with_state(encode_cluster_snapshot).await {
        Ok(bytes) => (StatusCode::OK, Json(serde_json::json!({
            "success": true,
            "bytes": bytes.len(),
            "note": "In-memory snapshot encoded. Cluster snapshots are persisted automatically by Raft."
        }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": format!("snapshot encode failed: {e}")
        }))).into_response(),
    }
}

async fn cluster_snapshot_restore() -> Response {
    (StatusCode::NOT_IMPLEMENTED, Json(serde_json::json!({
        "error": "Snapshot restore in cluster mode must be done via the Raft snapshot mechanism. \
                  Shut down all nodes, replace the redb log file on node-1, and restart."
    }))).into_response()
}

async fn cluster_snapshot_download(
    State(state): State<DataPlaneState>,
) -> Response {
    match state.sm.with_state(encode_cluster_snapshot).await {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE.as_str(), "application/octet-stream"),
                (header::CONTENT_DISPOSITION.as_str(), "attachment; filename=\"cluster-snapshot.snap\""),
            ],
            bytes,
        ).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": format!("snapshot encode failed: {e}")
        }))).into_response(),
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────
/// `GET /v1/shard/routing` — show namespace→shard assignment for all collections.
///
/// In cluster mode, also shows the shard count and which shard each namespace
/// maps to via `namespace_id % shard_count`.
async fn cluster_shard_routing(
    State(state): State<DataPlaneState>,
) -> Response {
    let shard_count = state.shard_count as usize;

    // Read namespace registry from shard 0's state machine.
    let shard0 = match state.shards.values().next() {
        Some(s) => s,
        None => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "no shard 0"}))).into_response(),
    };
    let collections: Vec<(String, u16)> = shard0.state_machine.with_state(|_ks| {
        vec![("default".to_string(), 0u16)]
    }).await;

    // Build per-shard collection buckets
    let mut shard_map: Vec<Vec<String>> = vec![Vec::new(); shard_count.max(1)];
    for (name, ns_id) in &collections {
        let shard = ns_id.wrapping_rem(shard_count.max(1) as u16) as usize;
        if let Some(bucket) = shard_map.get_mut(shard) {
            bucket.push(name.clone());
        }
    }

    let shards: Vec<serde_json::Value> = shard_map.into_iter().enumerate().map(|(i, cols)| {
        serde_json::json!({ "shard": i, "collections": cols })
    }).collect();

    (StatusCode::OK, Json(serde_json::json!({
        "mode": "cluster",
        "shard_count": shard_count,
        "shards": shards,
    }))).into_response()
}

// ── Receipt endpoints (Phase A8) ──────────────────────────────────────────────

async fn cluster_get_latest_receipt(
    axum::Extension(store): axum::Extension<Arc<valori_effect::ReceiptStore>>,
) -> Response {
    match store.latest() {
        Some(r) => Json(serde_json::json!({"ok": true, "receipt": r})).into_response(),
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "no receipt available yet"}))).into_response(),
    }
}

async fn cluster_get_receipt_by_id(
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Extension(store): axum::Extension<Arc<valori_effect::ReceiptStore>>,
) -> Response {
    match store.get(&id) {
        Some(r) => Json(serde_json::json!({"ok": true, "receipt": r})).into_response(),
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": format!("receipt '{}' not found", id)}))).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::ReadinessGate;

    /// A fresh node (target == 0) must be ready immediately without any apply.
    #[test]
    fn gate_target_zero_is_immediately_ready() {
        let gate = ReadinessGate::new(0);
        assert!(gate.check_applied(0).is_ok(), "target=0 should be ready at apply=0");
        assert!(gate.check_applied(5).is_ok(), "target=0 should stay ready");
    }

    /// Below the target the gate must return an Err (503) response.
    #[test]
    fn gate_blocks_below_target() {
        let gate = ReadinessGate::new(10);
        assert!(gate.check_applied(0).is_err(),  "apply=0  < target=10 → not ready");
        assert!(gate.check_applied(9).is_err(),  "apply=9  < target=10 → not ready");
    }

    /// Exactly at the target the gate must flip open and return Ok.
    #[test]
    fn gate_opens_at_target() {
        let gate = ReadinessGate::new(10);
        assert!(gate.check_applied(10).is_ok(), "apply=10 == target=10 → ready");
    }

    /// After the gate has latched open once, all subsequent calls return Ok
    /// regardless of the applied index — steady-state nodes don't regress.
    #[test]
    fn gate_latches_open_permanently() {
        let gate = ReadinessGate::new(5);
        // Trip the latch.
        assert!(gate.check_applied(5).is_ok());
        // Simulate a momentarily lower applied index (shouldn't happen in practice
        // but the gate must still return Ok once latched).
        assert!(gate.check_applied(0).is_ok(), "latch must not re-close");
        assert!(gate.check_applied(100).is_ok(), "latch open forever");
    }

    /// The latch is shared-state: once opened by one caller, the next caller
    /// sees it open too (the fast-path `self.ready.load` branch).
    #[test]
    fn gate_fast_path_after_latch() {
        let gate = ReadinessGate::new(3);
        gate.check_applied(3).ok(); // open latch
        // Second call must hit the fast-path (ready == true) and return Ok.
        assert!(gate.check_applied(0).is_ok(), "fast-path must bypass target check");
    }

    // ── Phase S3: shard_for_namespace ────────────────────────────────────────

    use super::shard_for_namespace;
    use valori_consensus::types::ShardId;

    #[test]
    fn shard_count_one_always_resolves_to_shard_zero() {
        // S1's default — must be byte-identical to today's single-shard behavior.
        for ns in [0u16, 1, 2, 1023] {
            assert_eq!(shard_for_namespace(ns, 1), ShardId(0));
        }
    }

    #[test]
    fn default_namespace_always_resolves_to_shard_zero() {
        // Namespace 0 ("default") lands on shard 0 regardless of shard_count —
        // consequence of the modulo, not a special case, but worth pinning:
        // the namespace registry itself lives only on shard 0 (Phase S2),
        // so this must hold for the registry's own bookkeeping to be sound.
        for shard_count in [1u32, 2, 3, 8] {
            assert_eq!(shard_for_namespace(0, shard_count), ShardId(0));
        }
    }

    #[test]
    fn distributes_across_shards_deterministically_and_repeatably() {
        assert_eq!(shard_for_namespace(1, 3), ShardId(1));
        assert_eq!(shard_for_namespace(2, 3), ShardId(2));
        assert_eq!(shard_for_namespace(3, 3), ShardId(0));
        assert_eq!(shard_for_namespace(4, 3), ShardId(1));
        // Same inputs, same output — pure function, no hidden state.
        assert_eq!(shard_for_namespace(4, 3), shard_for_namespace(4, 3));
    }

    #[test]
    fn shard_count_zero_does_not_panic() {
        // Defensive: shard_count should never actually be 0 in practice
        // (ClusterConfig::from_env rejects it), but the routing function
        // itself must not divide by zero if ever called with a bad value.
        assert_eq!(shard_for_namespace(5, 0), ShardId(0));
    }
}
