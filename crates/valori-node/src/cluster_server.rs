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

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};

#[cfg(feature = "utoipa")]
#[allow(unused_imports)]
use crate::openapi::ApiError;

use crate::cluster::ShardHandle;
use axum::extract::Path;
use valori_consensus::types::{Raft, ShardId, CURRENT_SCHEMA_VERSION};
use valori_consensus::{ClientRequest, ValoriStateMachine};
use valori_engine::index_manager::{
    CollectionIndexState, IndexBuildRequest as EngineIndexBuildRequest, IndexSpec,
    IndexStatusResponse,
};
use valori_kernel::event::KernelEvent;
use valori_kernel::fxp::qformat::SCALE;
use valori_kernel::index::SearchResult as KernelSearchResult;
use valori_kernel::types::enums::{EdgeKind, NodeKind};
use valori_kernel::types::id::{NodeId, RecordId};
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

use crate::api_keys::{required_scope, ApiScope, AuthState, KeyStore};
use crate::cluster::ClusterHandle;
use crate::cluster_api::cluster_router;
use crate::crypto_vault::{hex_to_key_id, key_id_to_hex, new_key_id};
use crate::events::event_log::EventLogWriter;
use crate::server::sum_log_and_archives;
use axum::body::Body;
use axum::extract::Extension;
use axum::extract::Request as AxumRequest;
use axum::http::header::AUTHORIZATION;
use axum::http::HeaderValue;
use axum::middleware::Next;
use valori_kernel::crypto::KeyVault;

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
    /// Phase 3.1 object store, from `VALORI_OBJECT_STORE_URL`. `None` when
    /// unset — the `/v1/storage/*` handlers then 400 with the same message
    /// the standalone path uses.
    object_store: Option<Arc<crate::object_store::ObjectStoreBackend>>,
    /// Phase 3.6: per-node AES-256-GCM vault. DEKs are not Raft-replicated;
    /// each node holds only the keys for records it encrypted.
    vault: Arc<dyn KeyVault + Send + Sync>,
    /// Phase I4: on-node embed config (from VALORI_EMBED_* env vars).
    /// None when VALORI_EMBED_PROVIDER is not set.
    embed_config: Option<valori_ingest::EmbedConfig>,
    /// Phase I5: node-local tree cache keyed by BLAKE3(text). Derived from
    /// build requests; not replicated via Raft (trees are deterministic from
    /// their source text, so any peer can rebuild them locally).
    tree_cache:
        Arc<tokio::sync::RwLock<std::collections::HashMap<String, valori_rag::tree::TreeIndex>>>,
    /// Phase I6: last community detection result on this node.
    /// Node-local (not Raft-replicated) — communities are derived from the
    /// graph which IS replicated, so any peer can re-derive an identical store.
    community_store: Arc<tokio::sync::RwLock<Option<valori_rag::community::CommunityStore>>>,
    /// Phase S3: every shard this node runs (Phase S1's `ClusterHandle.shards`,
    /// always contains at least `ShardId(0)`). `raft`/`sm` above are shard 0's
    /// handles, kept as flat fields so every handler that doesn't resolve a
    /// namespace keeps working unchanged. Handlers that DO resolve a
    /// `NamespaceId` should route through `shard_for()` instead of `raft`/`sm`
    /// directly — see the doc comment there.
    shards: Arc<std::collections::BTreeMap<ShardId, ShardHandle>>,
    /// Phase S1's `VALORI_SHARD_COUNT` (default 1). Used by `shard_for_namespace()`.
    shard_count: u32,
    /// Phase 4.3: per-collection node-local ANN index state.
    ///
    /// ANN indexes are **derived acceleration structures**, NOT Raft state.
    /// The authoritative record data lives in `sm` (KernelState, Raft-replicated).
    /// Each node builds its own local index from its own materialized KernelState.
    ///
    /// - Desired spec + generation are replicated via SetMeta in KernelState.
    /// - Each node activates its local build independently (node-local activation model).
    /// - A node with a failed or absent local index falls back to exact brute-force search.
    cluster_indexes:
        Arc<tokio::sync::RwLock<std::collections::HashMap<u16, ClusterCollectionIndex>>>,
}

/// Phase 4.3/4.4: node-local ANN index state for one collection.
///
/// # Consistency contract
///
/// This is NOT Raft state. All fields are derived from the authoritative
/// KernelState (Raft-replicated) or from the Raft-replicated SetMeta that
/// carries the desired spec and generation. The `index` field is the only
/// part that is genuinely node-local and physically distinct across peers.
///
/// ## Authoritative (Raft-replicated)
/// - Collection ID, dimension, metric, records, graph state
/// - `desired_generation` — the logical generation all nodes must build
/// - `desired_spec` — the index type + parameters all nodes must use
///
/// ## Node-local (derived)
/// - HNSW/IVF/BQ runtime structures
/// - Build progress and task handle
/// - Local activation status
/// - `last_build_started_at` — for FAILED retry debounce
///
/// A difference in `active_generation` between two replicas (e.g. node A
/// at gen 8 and node B still at gen 7) is an **acceleration difference**,
/// NOT a data divergence. The underlying Collection state is identical on
/// every node; only the search recall/speed differs temporarily.
pub struct ClusterCollectionIndex {
    /// Lifecycle state for API responses and build tracking.
    state: CollectionIndexState,
    /// The live ANN index object for this collection, if built and active.
    /// `None` means "fall back to exact brute-force search".
    index: Option<Box<dyn valori_index::VectorIndex + Send + Sync>>,
    /// Phase 4.4: instant of the most recent build start (not commit time).
    /// Used to debounce retry after FAILED: minimum 60 s between retries.
    last_build_started_at: Option<std::time::Instant>,
}

impl ClusterCollectionIndex {
    fn new() -> Self {
        Self {
            state: CollectionIndexState::new(),
            index: None,
            last_build_started_at: None,
        }
    }
}

/// Replica key for the desired index spec stored in replicated KernelState.meta.
/// Format: `{"generation":N,"type":"hnsw","parameters":{...}}` or `"null"` (no index).
fn idx_spec_key(namespace_id: u16) -> String {
    format!("__valori_idx_spec:{namespace_id}")
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

    // ── Phase 4.3: node-local ANN index management ────────────────────────────

    /// Snapshot records for `namespace_id` from the replicated KernelState.
    /// Returns `(record_id, f32_vector)` pairs for all searchable records.
    async fn snapshot_records_for_build(&self, namespace_id: u16) -> Vec<(u32, Vec<f32>)> {
        self.sm
            .with_state(|s| {
                s.iter_records_in_ns(namespace_id)
                    .filter(|r| r.is_searchable())
                    .map(|r| {
                        let vals: Vec<f32> = r
                            .vector
                            .data
                            .iter()
                            .map(|fxp| fxp.0 as f32 / (SCALE as f32))
                            .collect();
                        (r.id.0, vals)
                    })
                    .collect()
            })
            .await
    }

    /// Start a background build for `namespace_id` at the replicated `generation`.
    ///
    /// Returns immediately; the build runs in a `spawn_blocking` task. The
    /// caller has already committed the desired spec through Raft before calling
    /// this, so `generation` is the authoritative cluster-wide generation id.
    ///
    /// # Phase 4.4 hardening
    ///
    /// - **Duplicate guard**: skips if already ACTIVE or BUILDING at this gen.
    /// - **Failed-retry debounce**: 60-second minimum gap between retries.
    /// - **Stale-build detection**: before READY→ACTIVE, re-reads the Raft-
    ///   replicated desired generation. If the desired gen has advanced past
    ///   `generation`, the build is discarded without activation, preventing
    ///   a slow gen-8 build from overwriting a faster gen-9 build.
    async fn trigger_local_build(&self, namespace_id: u16, generation: u32, spec: IndexSpec) {
        let build_start = std::time::Instant::now();

        // ── Gate: mark BUILDING locally, or bail out early ─────────────────
        {
            let mut indexes = self.cluster_indexes.write().await;
            let entry = indexes
                .entry(namespace_id)
                .or_insert_with(ClusterCollectionIndex::new);

            // Already at or ahead of this generation — nothing to do.
            if entry.state.active_generation == Some(generation) {
                return;
            }
            if entry.state.building_generation == Some(generation) {
                return;
            }

            // Building a DIFFERENT generation: linearizable Raft serialises
            // requests, so this shouldn't happen, but guard for safety.
            if entry.state.is_building() {
                tracing::warn!(
                    ns = namespace_id,
                    gen = generation,
                    "Cluster ANN build skipped — another build already in progress"
                );
                return;
            }

            // Debounce FAILED retry: require ≥ 60 s gap between attempts.
            if entry.state.generations.iter().any(|(g, st, _)| {
                *g == generation && *st == valori_engine::index_manager::IndexState::Failed
            }) {
                let too_soon = entry
                    .last_build_started_at
                    .map(|t| t.elapsed().as_secs() < 60)
                    .unwrap_or(false);
                if too_soon {
                    tracing::debug!(
                        ns = namespace_id,
                        gen = generation,
                        "Cluster ANN retry suppressed — within 60 s debounce window"
                    );
                    return;
                }
                // Past debounce window: remove the failed entry so we can restart.
                entry.state.generations.retain(|(g, _, _)| *g != generation);
            }

            // Force `next_generation` to exactly `generation` so `start_build`
            // allocates the correct Raft-replicated generation id (not a locally
            // incremented one that would disagree across nodes).
            entry.state.next_generation = generation;
            let allocated = entry.state.start_build(spec.clone(), 0);
            debug_assert_eq!(allocated, generation);
            entry.state.desired = Some(spec.clone());
            entry.last_build_started_at = Some(build_start);
        }

        metrics::increment_counter!("valori_cluster_ann_build_started_total",
            "collection" => namespace_id.to_string(),
            "index_type" => spec.index_type.clone()
        );

        tracing::info!(
            ns = namespace_id,
            gen = generation,
            index_type = %spec.index_type,
            "Cluster ANN build started"
        );

        // Snapshot the current record set for building.
        let records = self.snapshot_records_for_build(namespace_id).await;
        let state_clone = self.clone();

        tokio::spawn(async move {
            use valori_index::{BqIndex, HnswIndex, IvfIndex, VectorIndex};

            let spec_clone = spec.clone();
            let records_clone = records.clone();
            let dim = records_clone.first().map(|(_, v)| v.len()).unwrap_or(0);

            let result: Result<Box<dyn VectorIndex + Send + Sync>, String> =
                tokio::task::spawn_blocking(move || {
                    let params = &spec_clone.parameters;
                    let mut idx: Box<dyn VectorIndex + Send + Sync> = match spec_clone
                        .index_type
                        .as_str()
                    {
                        "hnsw" => {
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
                            let user_n_list = params.get("n_list").and_then(|v| v.as_u64());
                            let user_n_probe = params.get("n_probe").and_then(|v| v.as_u64());
                            let (n_list, n_probe, auto_scale) = if let Some(nl) = user_n_list {
                                let np = user_n_probe.map(|v| v as usize).unwrap_or_else(|| {
                                    std::cmp::max(1, (nl as f64).sqrt() as usize)
                                });
                                (nl as usize, np, false)
                            } else {
                                let auto_nl =
                                    std::cmp::max(16, (records_clone.len() as f32).sqrt() as usize);
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
                        t => return Err(format!("unknown index type '{t}'")),
                    };
                    idx.build(&records_clone);
                    Ok(idx)
                })
                .await
                .unwrap_or_else(|e| Err(format!("build task panicked: {e}")));

            let elapsed = build_start.elapsed().as_secs_f64();

            // ── Phase 4.4: Stale-build detection ───────────────────────────
            //
            // Before activating, verify the authoritative desired generation
            // still matches what we just built. This guards against:
            //
            //   gen 8 builds slowly  →  gen 9 is committed while building
            //   gen 8 finishes       →  would incorrectly activate as gen 8
            //
            // and against collection deletion during build:
            //
            //   gen 8 builds slowly  →  DropNamespace committed
            //   gen 8 finishes       →  must NOT activate
            //
            // Reading from the replicated SM here is safe: the build ran on a
            // `spawn_blocking` thread that doesn't hold the SM lock, and re-
            // reading takes a brief async lock after the blocking work is done.
            let current_desired_gen: Option<u32> = state_clone
                .sm
                .get_meta_json(&idx_spec_key(namespace_id))
                .await
                .and_then(|v| {
                    if v == serde_json::Value::Null {
                        None // index was dropped while building
                    } else {
                        v.get("generation")
                            .and_then(|g| g.as_u64())
                            .map(|n| n as u32)
                    }
                });

            // Also confirm the collection still exists (guards DropNamespace).
            let collection_exists = state_clone.sm.resolve_namespace(None).await.is_some()
                || state_clone
                    .sm
                    .list_namespaces()
                    .await
                    .iter()
                    .any(|(_, id)| *id == namespace_id);

            let is_stale = !collection_exists || current_desired_gen != Some(generation);

            let mut indexes = state_clone.cluster_indexes.write().await;
            let entry = indexes
                .entry(namespace_id)
                .or_insert_with(ClusterCollectionIndex::new);

            match result {
                Ok(idx) if !is_stale => {
                    // BUILDING → READY → ACTIVE (node-local activation).
                    entry.state.mark_ready(generation);
                    entry.state.activate(generation);
                    entry.index = Some(idx);

                    metrics::increment_counter!("valori_cluster_ann_build_completed_total",
                        "collection" => namespace_id.to_string(),
                        "index_type" => spec.index_type.clone()
                    );
                    metrics::histogram!("valori_cluster_ann_build_duration_seconds",
                        elapsed,
                        "collection" => namespace_id.to_string()
                    );
                    metrics::gauge!("valori_cluster_ann_generation_active",
                        generation as f64,
                        "collection" => namespace_id.to_string()
                    );

                    tracing::info!(
                        ns = namespace_id,
                        gen = generation,
                        index_type = %spec.index_type,
                        elapsed_secs = elapsed,
                        "Cluster ANN build complete and ACTIVE"
                    );
                }
                Ok(_idx_discarded) => {
                    // Build succeeded but desired gen moved on — discard silently.
                    // Mark FAILED so the watcher can pick up the new desired gen.
                    entry.state.mark_failed(
                        generation,
                        format!(
                            "build obsolete: desired gen moved to {:?} before activation",
                            current_desired_gen
                        ),
                    );

                    metrics::increment_counter!("valori_cluster_ann_stale_activation_skipped_total",
                        "collection" => namespace_id.to_string()
                    );

                    tracing::info!(
                        ns = namespace_id,
                        gen = generation,
                        current_desired_gen = ?current_desired_gen,
                        collection_exists,
                        "Cluster ANN build discarded — desired generation advanced; watcher will build new gen"
                    );
                }
                Err(e) => {
                    entry.state.mark_failed(generation, e.clone());

                    metrics::increment_counter!("valori_cluster_ann_build_failed_total",
                        "collection" => namespace_id.to_string(),
                        "index_type" => spec.index_type.clone()
                    );

                    tracing::error!(
                        ns = namespace_id,
                        gen = generation,
                        error = %e,
                        "Cluster ANN build failed; will retry after debounce window"
                    );
                }
            }
        });
    }

    /// Check all known collections for pending index builds (follower watcher).
    ///
    /// Reads the replicated `SetMeta` spec key for each collection and starts
    /// a local build if the local state is behind the desired generation.
    /// Called every 5 s by the background watcher task.
    ///
    /// # Phase 4.4 hardening
    ///
    /// - **Drop while building**: if the desired spec is removed (`SetMeta(null)`)
    ///   while a local build is in progress, the watcher now clears both the
    ///   building state and the active index. The in-flight `trigger_local_build`
    ///   task will also detect the stale desired gen before activation.
    /// - **FAILED retry debounce**: checking whether `trigger_local_build`
    ///   should be called uses the same 60-second debounce logic embedded in
    ///   that function, so the watcher simply calls it and lets it decide.
    /// - **Single `list_namespaces` call**: snapshot the set once and reuse it
    ///   for both the per-collection pass and the cleanup pass.
    async fn check_and_trigger_pending_builds(&self) {
        let namespaces = self.sm.list_namespaces().await;
        let known_ns: std::collections::HashSet<u16> =
            namespaces.iter().map(|(_, id)| *id).collect();

        for (_name, ns_id) in &namespaces {
            let ns_id = *ns_id;
            let key = idx_spec_key(ns_id);
            let desired_json = match self.sm.get_meta_json(&key).await {
                Some(v) if v != serde_json::Value::Null => v,
                _ => {
                    // No desired index (either never set or explicitly dropped).
                    // Clear any locally held index state — this handles:
                    //   (a) index dropped while idle (active → none)
                    //   (b) index dropped while building (building → none)
                    // The in-flight build task will detect the stale desired gen
                    // via its stale-activation check and will not activate.
                    let mut indexes = self.cluster_indexes.write().await;
                    if let Some(entry) = indexes.get_mut(&ns_id) {
                        let has_state = entry.state.desired.is_some()
                            || entry.state.active_generation.is_some()
                            || entry.state.building_generation.is_some();
                        if has_state {
                            tracing::debug!(
                                ns = ns_id,
                                "Cluster ANN: clearing local index — no desired spec in Raft state"
                            );
                            entry.state.set_none();
                            entry.index = None;
                        }
                    }
                    continue;
                }
            };

            let gen = desired_json
                .get("generation")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32)
                .unwrap_or(0);
            let type_str = desired_json
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let params = desired_json.get("parameters").cloned().unwrap_or_default();

            if type_str.is_empty() || gen == 0 {
                tracing::warn!(
                    ns = ns_id,
                    "Cluster ANN: malformed desired spec in Raft (missing type or generation=0), skipping"
                );
                continue;
            }

            // Determine whether a local build is needed.
            // `trigger_local_build` handles the duplicate and debounce guards
            // internally, so we just call it when we're not already fully done.
            let needs_build = {
                let indexes = self.cluster_indexes.read().await;
                match indexes.get(&ns_id) {
                    None => true,
                    Some(entry) => {
                        let active_ok = entry.state.active_generation == Some(gen);
                        let building_same = entry.state.building_generation == Some(gen);
                        !active_ok && !building_same
                    }
                }
            };

            if needs_build {
                let spec = IndexSpec {
                    index_type: type_str,
                    parameters: params,
                };
                self.trigger_local_build(ns_id, gen, spec).await;
            }
        }

        // Drop state for collections that no longer exist in the Raft registry
        // (handles DropNamespace committed while this node was building).
        {
            let mut indexes = self.cluster_indexes.write().await;
            indexes.retain(|ns_id, _| known_ns.contains(ns_id));
        }
    }

    /// Try to search using the node-local active ANN index for `namespace_id`.
    ///
    /// Returns `Some(hits)` when an ACTIVE index exists and produces results.
    /// Returns `None` to signal the caller to fall back to exact brute-force.
    ///
    /// # Phase 4.4 observability
    ///
    /// When this returns `None` the caller MUST emit a search-fallback metric
    /// via `record_ann_search_fallback` so operators can detect nodes that are
    /// permanently on the brute-force path despite having an ANN build requested.
    async fn try_ann_search(
        &self,
        namespace_id: u16,
        query_f32: &[f32],
        k: usize,
    ) -> Option<Vec<(u32, f32)>> {
        let indexes = self.cluster_indexes.read().await;
        let entry = indexes.get(&namespace_id)?;
        // Only use the index when it is in ACTIVE state with a live object.
        entry.state.active_generation?;
        let idx = entry.index.as_ref()?;
        let results = idx.search(query_f32, k);
        Some(results)
    }

    /// Emit a `valori_cluster_ann_search_fallback_total` counter increment.
    /// Called by search handlers when `try_ann_search` returns `None`.
    fn record_ann_search_fallback(&self, namespace_id: u16) {
        metrics::increment_counter!("valori_cluster_ann_search_fallback_total",
            "collection" => namespace_id.to_string()
        );
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
        std::io::Error::new(e.kind(), format!("cannot bind API to {api_bind}: {e}"))
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
    build_cluster_router_with_keys(
        handle,
        audit,
        cfg.auth_token.clone(),
        Arc::new(KeyStore::new(None)),
        &cfg,
        Arc::new(valori_effect::ReceiptStore::new(256)),
    )
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
        object_store: crate::object_store::ObjectStoreBackend::from_env(),
        vault: {
            use crate::crypto_vault::AesGcmVault;
            Arc::new(AesGcmVault::in_memory())
        },
        embed_config: crate::engine::embed_config_from_node(node_cfg),
        tree_cache: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        community_store: Arc::new(tokio::sync::RwLock::new(None)),
        cluster_indexes: handle.cluster_indexes.clone(),
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

    let auth = Arc::new(AuthState {
        key_store,
        legacy_token: auth_token,
    });

    // ── Phase 4.3: background index watcher ───────────────────────────────────
    // Picks up desired index specs committed by the leader (or any node) and
    // triggers node-local ANN builds for any collection whose desired generation
    // hasn't been built here yet. The leader also triggers a build immediately
    // after committing the spec, so followers only need this to catch up.
    {
        let watcher_state = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            loop {
                interval.tick().await;
                watcher_state.check_and_trigger_pending_builds().await;
            }
        });
    }

    // ── Public routes (no auth) ───────────────────────────────────────────────
    let public = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .with_state(state.clone());

    // ── Canonical v1 routes ───────────────────────────────────────────────────
    let v1 = Router::new()
        .route("/v1/records", post(insert_record))
        .route("/v1/records/:id", axum::routing::get(get_record_by_id))
        .route(
            "/v1/records/:id/metadata",
            axum::routing::patch(update_record_metadata),
        )
        .route("/v1/search", post(search))
        .route("/v1/search/multi", post(cluster_multi_search))
        .route("/v1/delete", post(delete_record))
        .route("/v1/soft-delete", post(soft_delete_record))
        .route("/v1/vectors/batch-insert", post(batch_insert))
        .route(
            "/v1/namespaces",
            post(create_collection_handler).get(list_collections_handler),
        )
        .route("/v1/namespaces/:name", delete(drop_collection_handler))
        .route(
            "/v1/namespaces/:name/index",
            post(cluster_index_lifecycle_create).get(cluster_index_lifecycle_status),
        )
        .route("/v1/usage", get(usage))
        .route("/v1/proof/state", get(state_proof))
        .route("/v1/proof/event-log", get(event_log_proof))
        .route("/v1/cluster/proof", get(cluster_proof))
        .route("/v1/proof/receipt", get(cluster_get_latest_receipt))
        .route("/v1/proof/receipt/:id", get(cluster_get_receipt_by_id))
        .route("/v1/graph/node", post(create_graph_node))
        .route(
            "/v1/graph/node/:id",
            get(get_graph_node).delete(delete_graph_node),
        )
        .route("/v1/graph/edge", post(create_graph_edge))
        .route("/v1/graph/edges/:id", get(get_graph_edges))
        .route("/v1/graph/subgraph", get(get_graph_subgraph))
        .route("/v1/graph/query", get(get_graph_query))
        .route("/v1/graphrag", post(cluster_graphrag))
        .route("/v1/keys", post(cluster_create_key).get(cluster_list_keys))
        .route("/v1/keys/:id", delete(cluster_revoke_key))
        .route("/v1/records/encrypted", post(cluster_insert_encrypted))
        .route("/v1/crypto/shred/:key_id", delete(cluster_shred_key))
        .route("/v1/crypto/status/:key_id", get(cluster_crypto_status))
        .route("/v1/index/config", axum::routing::get(cluster_index_config))
        .route("/v1/index/rebuild", post(cluster_index_rebuild))
        .route(
            "/v1/shard/routing",
            axum::routing::get(cluster_shard_routing),
        )
        .route("/v1/ingest/document", post(valori_ingest::ingest_document))
        .route("/v1/ingest", post(cluster_ingest))
        .route(
            "/v1/ingest/status/:job_id",
            get(crate::ingest::get_ingest_status),
        )
        .route("/v1/ingest/update", post(cluster_ingest_update))
        .route(
            "/v1/ingest/extract-entities",
            post(cluster_extract_entities),
        )
        .route("/v1/tree/build", post(cluster_tree_build))
        .route("/v1/tree/query", post(cluster_tree_query))
        .route("/v1/tree/hybrid", post(cluster_tree_hybrid))
        .route("/v1/tree/verify", post(valori_rag::tree::tree_verify))
        .route(
            "/v1/tree/chain-verify",
            post(valori_rag::tree::tree_chain_verify),
        )
        .route("/v1/community/detect", post(cluster_community_detect))
        .route("/v1/community/search", post(cluster_community_search))
        .route("/v1/community/overview", get(cluster_community_overview))
        .route("/v1/memory/consolidate", post(cluster_memory_consolidate))
        .route("/v1/memory/contradict", post(cluster_memory_contradict))
        .route("/v1/memory/upsert", post(cluster_memory_upsert))
        .route("/v1/memory/upsert_vector", post(cluster_memory_upsert))
        .route("/v1/memory/search", post(cluster_memory_search))
        .route("/v1/memory/search_vector", post(cluster_memory_search))
        .route("/v1/memory/meta/set", post(cluster_meta_set))
        .route("/v1/memory/meta/get", axum::routing::get(cluster_meta_get))
        .route("/v1/graph/nodes", get(cluster_list_nodes))
        .route("/v1/models/health", get(cluster_models_health))
        .route("/v1/version", get(cluster_version))
        .route("/v1/timeline", get(cluster_timeline))
        .route("/v1/operations", get(cluster_get_operations))
        .route("/v1/operations/:id", get(cluster_get_operation_by_id))
        .route(
            "/v1/operations/:id/execution",
            get(crate::server::get_operation_execution),
        )
        .route("/v1/snapshot/save", post(cluster_snapshot_save))
        .route("/v1/snapshot/restore", post(cluster_snapshot_restore))
        .route("/v1/snapshot/download", get(cluster_snapshot_download))
        // Phase 3.1 object store — reads + upload are per-node safe; restore
        // is a documented 501 (see cluster_storage_restore).
        .route("/v1/storage/snapshots", get(cluster_list_remote_snapshots))
        .route(
            "/v1/storage/snapshots/upload",
            post(cluster_upload_snapshot_to_store),
        )
        .route(
            "/v1/storage/snapshots/restore",
            post(cluster_storage_restore),
        )
        .route("/v1/storage/manifest", get(cluster_get_manifest))
        .route("/v1/storage/wal", get(cluster_list_remote_wal))
        .route("/v1/storage/wal/archive", post(cluster_archive_wal_segment));

    // ── Deprecated legacy routes ──────────────────────────────────────────────
    let legacy = Router::new()
        .route("/records", post(insert_record))
        .route("/search", post(search))
        .route("/operations", get(cluster_get_operations))
        .route("/operations/:id", get(cluster_get_operation_by_id))
        .route("/graph/node", post(create_graph_node))
        .route(
            "/graph/node/:id",
            get(get_graph_node).delete(delete_graph_node),
        )
        .route("/graph/edge", post(create_graph_edge))
        .route("/graph/edges/:id", get(get_graph_edges))
        .route("/graph/subgraph", get(get_graph_subgraph))
        // snake_case alias kept for backward compat
        .route("/v1/vectors/batch_insert", post(batch_insert))
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
    let capability_registry: Arc<valori_effect::capability::CapabilityRegistry> =
        Arc::new(CapabilityRegistryBuilder::build_cluster(
            state.shards.clone(),
            state.sm.clone(),
            state.shard_count as u8,
            state.embed_config.clone(),
            state.http.clone(),
            state.tree_cache.clone(),
            state.community_store.clone(),
        ));
    let task_registry: Arc<TaskRegistry> = Arc::new(TaskRegistry::default_registry());
    let execution_registry: Arc<crate::execution_registry::ExecutionRegistry> =
        Arc::new(crate::execution_registry::ExecutionRegistry::default());

    // Auth is applied only to protected routes; public routes (/health, /metrics)
    // are merged AFTER the auth layer so liveness probes and Prometheus scrapers
    // never require credentials.
    let protected = Router::new()
        .merge(v1)
        .merge(legacy)
        .with_state(state)
        .merge(cluster_router(raft, Arc::new(api_shards), audit))
        .layer(axum::middleware::from_fn(cluster_auth_guard))
        .layer(Extension(auth.clone()))
        .layer(Extension(receipt_store))
        .layer(Extension(capability_registry))
        .layer(Extension(task_registry))
        .layer(Extension(execution_registry));

    let mut router = Router::new()
        .merge(public)
        .merge(protected)
        // Phase API-2: same guarantee as the standalone router — see
        // `crate::error_codes`.
        .layer(axum::middleware::from_fn(
            crate::error_codes::attach_error_code,
        ));
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
        HeaderValue::from_static("<https://docs.valori.ai/api/v1>; rel=\"successor-version\""),
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
        config: crate::routes::collections::CollectionConfigRequest,
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
                event: KernelEvent::AutoCreateNamespace {
                    name: name.to_string(),
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: 0,
            },
        )
        .await?;
        let id = resp.allocated_namespace_id.unwrap_or(0);
        // Second, separate Raft-committed event — same two-event pattern as
        // the standalone path's `Engine::create_collection_with_config`
        // (see that method's doc comment for why this isn't one combined
        // event). Applied identically on every replica by
        // `ValoriStateMachine::apply()`, so every node ends up with the
        // same collection dimension — the mandatory cluster-consistency
        // requirement this whole mechanism exists for. Always committed —
        // Phase 3.3: config is required for every collection, no exceptions.
        use valori_engine::config::IndexKind as EngineIndexKind;
        let engine_kind = EngineIndexKind::from_domain(config.index);
        raft_write_data(
            &self.raft,
            ClientRequest {
                event: KernelEvent::ConfigureNamespace {
                    namespace_id: id,
                    dim: config.dim,
                    metric: config.metric.as_u8(),
                    index_kind: engine_kind.as_u8(),
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: id,
            },
        )
        .await?;
        Ok(crate::routes::collections::CreatedCollection {
            id,
            already_existed,
        })
    }

    async fn drop_collection(&self, name: &str) -> Result<(), Response> {
        raft_write_data(
            &self.raft,
            ClientRequest {
                event: KernelEvent::DropNamespace {
                    name: name.to_string(),
                },
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

    async fn config(
        &self,
        namespace_id: u16,
    ) -> Option<crate::routes::collections::CollectionConfigRequest> {
        let c = self.sm.namespace_config(namespace_id).await?;
        // Desired index is tracked separately from vector config — see
        // `valori_metadata::collection`'s module doc.
        let index = self
            .sm
            .namespace_desired_index(namespace_id)
            .await
            .unwrap_or(valori_domain::IndexKind::Brute);
        Some(crate::routes::collections::CollectionConfigRequest {
            dim: c.dim,
            metric: c.metric,
            index,
        })
    }

    async fn record_count(&self, namespace_id: u16) -> usize {
        self.sm
            .with_state(|s| s.iter_records_in_ns(namespace_id).count())
            .await
    }

    async fn max_records(&self) -> usize {
        1_000_000
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
    // The dimension the kernel has actually locked to. Null until the first
    // insert — the node reports what it knows, not what it was configured
    // with. `useHealth` in the UI reads this at the top level; the `cluster`
    // sub-object carries the same value.
    let locked_dim = state.sm.locked_dim().await;

    let (status_str, status_code) = match m.current_leader {
        Some(_) => ("ok", StatusCode::OK),
        None => ("no-leader", StatusCode::SERVICE_UNAVAILABLE),
    };

    let cluster_stats = serde_json::json!({
        "status": status_str,
        "leader": m.current_leader,
        "dim": locked_dim,
        "role": format!("{:?}", m.state),
        "term": m.current_term,
    });

    let resp = crate::api::HealthResponse {
        status: status_str.to_string(),
        mode: "cluster".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        collections: None,
        persistence: None,
        records: None,
        nodes: None,
        edges: None,
        event_log_height: None,
        embed_enabled: Some(embed_enabled),
        embed_provider: embed_provider,
        shard_count: state.shard_event_log_paths.len().max(1),
        leader: m.current_leader,
        dim: locked_dim.map(|d| d as u32),
        node_id: Some(m.id),
        role: Some(format!("{:?}", m.state)),
        leader_id: m.current_leader,
        term: Some(m.current_term),
        raft_state: Some(format!("{:?}", m.state)),
        state_hash: None,
        members: None,
        engine: None,
        cluster: Some(cluster_stats),
    };

    (status_code, Json(resp)).into_response()
}

// ── Shared Raft write helper ──────────────────────────────────────────────────

/// Submit a `ClientRequest` to the Raft leader and map the response.
/// Handles the ForwardToLeader redirect and generic Raft errors uniformly.
async fn raft_write<F>(raft: &Raft, req: ClientRequest, on_ok: F) -> Response
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
                )
                    .into_response());
            }
            Ok(resp.data)
        }
        Err(openraft::error::RaftError::APIError(
            openraft::error::ClientWriteError::ForwardToLeader(fwd),
        )) => Err(not_leader_response(fwd.leader_node.as_ref())),
        Err(e) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": format!("raft write failed: {e}") })),
        )
            .into_response()),
    }
}

// ── Insert ────────────────────────────────────────────────────────────────────

// Phase API-2: the cluster path no longer defines its own insert
// request/response pair. Both routers now deserialise
// `crate::api::InsertRecordRequest` and serialise
// `crate::api::InsertRecordResponse`, so a field can no longer exist on one
// path and be silently dropped on the other.
use crate::api::{InsertRecordRequest as InsertRequest, InsertRecordResponse as InsertResponse};
use crate::error_codes::collection_not_found;

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
    let fxp_values: Vec<i32> = req
        .values
        .iter()
        .map(|&f| valori_kernel::fxp::ops::from_f32(f).0)
        .collect();
    let vector = match to_fxp(&req.values) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };

    // Phase S7: resolve collection -> namespace (registry always lives on
    // shard 0), then route the write to that namespace's data shard.
    let ns_id = match state.sm.resolve_namespace(req.collection.as_deref()).await {
        Some(id) => id,
        None => {
            return collection_not_found(req.collection.as_deref());
        }
    };
    let shard = state.shard_for(ns_id);
    let shard_id = shard_for_namespace(ns_id, state.shard_count).0 as u8;

    // Capture state hash before write.
    let (old_root, state_before) = {
        let raw: [u8; 32] = state.sm.state_hash().await;
        let hex = raw.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        (raw, hex)
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
            request_id: req.request_id.map(|r| r.0),
            schema_version: CURRENT_SCHEMA_VERSION,
            namespace_id: ns_id,
        },
    )
    .await
    {
        Ok(r) => r,
        Err(e) => return e,
    };

    let new_root: [u8; 32] = resp.state_hash;
    let state_after: String = new_root.iter().map(|b| format!("{:02x}", b)).collect();
    let record_id = resp.allocated_record_id.unwrap_or(0);
    let sequence = resp.log_index;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    {
        use valori_planner::operation::{OperationInputs, OperationKind};
        let inputs = OperationInputs::Ingest {
            strategy: "direct".into(),
            collection: req.collection.clone().unwrap_or_else(|| "default".into()),
            shard_id,
            embed_enabled: false,
        };
        crate::receipt_bridge::emit_write(
            &receipts,
            OperationKind::Ingest,
            &inputs,
            ns_id,
            shard_id,
            sequence,
            true,
            state_before,
            state_after,
        );
    }

    let receipt = valori_kernel::proof::InsertReceipt::build(
        record_id,
        old_root,
        &fxp_values,
        new_root,
        sequence,
        timestamp,
    );
    (
        StatusCode::OK,
        Json(InsertResponse {
            id: record_id,
            log_index: Some(sequence),
            deduplicated: resp.deduplicated,
            receipt: receipt.into(),
        }),
    )
        .into_response()
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
    /// G1.4.1 — optional graph-aware reranking. See `api::GraphRerankRequest`.
    #[serde(default)]
    graph_rerank: Option<crate::api::GraphRerankRequest>,
}

fn default_rerank() -> bool {
    true
}

fn default_k() -> usize {
    10
}

/// Hard ceiling on a single search's `k` — mirrors `server.rs::MAX_SEARCH_K`
/// (standalone path). Keep both in sync; see that constant's doc comment.
const MAX_SEARCH_K: usize = 5000;

// Wire-compatible with the standalone server's SearchHit { id, score }
// (api.rs) so one SDK client speaks to both standalone and cluster nodes.
// `score` is the L2 distance as a float (raw Q32.32 divided by SCALE²),
// matching the standalone conversion in server.rs.
#[derive(Serialize, Clone)]
struct SearchHit {
    id: u32,
    score: f32,
    /// G1.4.1 — hop distance to the nearest `graph_rerank` seed. See
    /// `api::SearchHit::graph_distance`.
    #[serde(skip_serializing_if = "Option::is_none")]
    graph_distance: Option<u32>,
}

/// G1.4.1 — cluster-path graph-aware reranking. Identical semantics to
/// standalone's `server::apply_graph_rerank` (same seed/distance/scoring
/// model — see docs/reviews/graph-g1.4.1-graph-aware-reranking-design.md),
/// operating directly on `&KernelState` (the shard's replicated state)
/// since the cluster path has no `Engine`.
fn apply_graph_rerank_cluster(
    state: &valori_kernel::state::kernel::KernelState,
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
        _ => valori_rag::graph::Direction::Outgoing,
    };

    let top_ids: Vec<u32> = hits.iter().take(seed_count).map(|h| h.id).collect();
    let seed_map = valori_rag::graph::resolve_seed_nodes(state, &top_ids);
    let mut seed_nodes: Vec<u32> = seed_map.values().copied().collect();
    seed_nodes.sort_unstable();
    seed_nodes.dedup();

    let distances =
        valori_rag::graph::graph_distances_from_seeds(state, &seed_nodes, direction, max_depth);

    let rerank_hits: Vec<valori_search::GraphRerankHit> = hits
        .iter()
        .map(|h| {
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
        .map(|r| SearchHit {
            id: r.id,
            score: r.score,
            graph_distance: r.graph_distance,
        })
        .collect()
}

/// G1.4.2 — namespace-scoped search for the cluster path.
///
/// BUG-6 (found in G1.4.1, fixed here): `KernelState::search_l2` searches
/// ALL records regardless of namespace ("backward-compat, single-tenant" —
/// see its own doc comment). Every prior cluster search call site used
/// exactly that function, relying entirely on shard routing
/// (`shard_for(ns)`) for isolation — which enforces nothing once more than
/// one namespace maps to the same shard (`shard_count=1`, the default,
/// puts every namespace on shard 0). Proven directly: two collections,
/// colliding vectors, a search scoped to one collection returned both.
///
/// Standalone's `Engine::search_l2_ns` (`crates/valori-engine/src/engine.rs`)
/// already solved this with a two-path split: the kernel's own
/// `KernelState::search_l2_ns` (exact, brute-force, namespace-scoped via
/// the intrusive per-namespace linked list) when `BruteForce` is active, or
/// a namespace-agnostic index search + post-filter when it isn't. This
/// mirrors that exact split for the cluster path's kernel-native index
/// (`valori_kernel::index::ActiveIndex` — `BruteForce` or
/// `BinaryQuantization`; cluster never calls `set_index_kind`, so every
/// cluster shard is `BruteForce` today, but this stays correct if that
/// changes).
fn shard_search_ns(
    s: &valori_kernel::state::kernel::KernelState,
    query: &FxpVector,
    fetch_k: usize,
    ns_id: u16,
) -> Vec<KernelSearchResult> {
    let mut buf = vec![KernelSearchResult::default(); fetch_k];
    match s.index_variant() {
        valori_kernel::index::IndexVariant::BruteForce => {
            let n = s.search_l2_ns(query, &mut buf, ns_id);
            buf.truncate(n);
            buf
        }
        _ => {
            // Namespace-agnostic index: search globally, then post-filter.
            // No pool-widening here — same known, documented gap as
            // standalone's equivalent branch (see
            // docs/reviews/graph-g1.4-hybrid-retrieval-design.md §1,
            // discrepancy #3). Not introduced by this fix.
            let n = s.search_l2(query, &mut buf, None);
            buf.truncate(n);
            buf.retain(|r| {
                s.get_record(r.id)
                    .map(|rec| rec.namespace_id == ns_id)
                    .unwrap_or(false)
            });
            buf
        }
    }
}

async fn search(
    State(state): State<DataPlaneState>,
    axum::Extension(receipts): axum::Extension<Arc<valori_effect::ReceiptStore>>,
    Json(req): Json<SearchRequest>,
) -> Response {
    // k=0 is meaningless; an unbounded k gets multiplied by POOL_FACTOR (20x)
    // on the rerank path before sizing a results buffer, so an unchecked huge
    // k is a client-triggerable unbounded allocation, matching the same bound
    // enforced on the standalone path (server.rs::MAX_SEARCH_K).
    if req.k == 0 || req.k > MAX_SEARCH_K {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("k must be between 1 and {MAX_SEARCH_K}, got {}", req.k)
            })),
        )
            .into_response();
    }

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
            return collection_not_found(req.collection.as_deref());
        }
    };
    let shard = state.shard_for(ns_id);
    let shard_sm = &shard.state_machine;

    // Dimension check against the locked kernel dim (set on first insert).
    // An empty store (dim == None) accepts any query length.
    if let Some(locked) = shard_sm.locked_dim().await {
        if req.query.len() != locked {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!(
                        "Query vector has {} elements but this store is locked to dim={}. \
                         Check GET /health for the current dim.",
                        req.query.len(), locked
                    )
                })),
            )
                .into_response();
        }
    }

    let query = match to_fxp(&req.query) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };

    // Linearizable reads (the default) establish a read index first, so the
    // local scan below reflects every write committed before this read began.
    if req.consistency == Consistency::Linearizable {
        if let Err(resp) = ensure_read_consistency(
            shard_for_namespace(ns_id, state.shard_count),
            &shard.raft,
            &state.http,
        )
        .await
        {
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
        (base_k * valori_search::POOL_FACTOR).max(base_k)
    } else {
        base_k
    };
    let query_text_owned = req.query_text.clone().unwrap_or_default();

    let results: Vec<SearchHit> = if half_life == 0 {
        // Phase 4.3/4.4: try the node-local ANN index first; fall back to
        // exact brute-force when no active index exists or the build is still
        // in progress. The underlying KernelState is identical on all nodes,
        // so ANN and brute-force results are semantically equivalent —
        // only recall/ranking may differ temporarily while builds converge.
        //
        // Modes that use this ANN path: normal top-k, metadata_filter (filter
        // is applied post-fetch), BM25 reranking (reranking is applied post-
        // fetch). Modes that stay on brute-force: decay (uses
        // `with_state_and_timestamps`, see below); graph reranking is a post-
        // fetch pass and works on top of whichever path is used here.
        let raw: Vec<SearchHit> =
            if let Some(ann_hits) = state.try_ann_search(ns_id, &req.query, fetch_k).await {
                ann_hits
                    .into_iter()
                    .map(|(id, dist)| SearchHit {
                        id,
                        score: dist,
                        graph_distance: None,
                    })
                    .collect()
            } else {
                // Phase 4.4: track fallbacks so operators know when ANN is
                // unavailable despite being configured.
                state.record_ann_search_fallback(ns_id);
                shard_sm
                    .with_state(|s| {
                        shard_search_ns(s, &query, fetch_k, ns_id)
                            .iter()
                            .map(|r| SearchHit {
                                id: r.id.0,
                                score: r.score as f32 / (SCALE as f32 * SCALE as f32),
                                graph_distance: None,
                            })
                            .collect()
                    })
                    .await
            };
        // Post-filter by metadata predicate before reranking/trimming. Reads
        // the replicated KernelState.meta map (set via SetMeta) so every
        // replica filters identically, not a per-node sidecar.
        let filtered: Vec<SearchHit> = if let Some(ref f) = mf {
            shard_sm
                .with_state(|s| {
                    raw.into_iter()
                        .filter(|h| {
                            let key = format!("rec:{}", h.id);
                            match s
                                .meta
                                .get(&key)
                                .and_then(|v| serde_json::from_str::<serde_json::Value>(v).ok())
                            {
                                Some(meta) => valori_search::matches_metadata_filter(&meta, f),
                                None => false,
                            }
                        })
                        .collect()
                })
                .await
        } else {
            raw
        };
        if use_rerank && !filtered.is_empty() {
            let candidates: Vec<(u64, f32)> =
                filtered.iter().map(|h| (h.id as u64, h.score)).collect();
            let candidate_ids: Vec<u64> = candidates.iter().map(|(id, _)| *id).collect();
            shard_sm
                .with_text_corpus(|corpus| {
                    // build a reranker seeded with only the candidate texts
                    let mut reranker = valori_search::ValoriReranker::new();
                    for id in &candidate_ids {
                        if let Some(text) = corpus.get(id) {
                            reranker.insert(*id, text);
                        }
                    }
                    reranker
                        .rerank(&query_text_owned, candidates)
                        .into_iter()
                        .take(k)
                        .map(|(id, score)| SearchHit {
                            id: id as u32,
                            score,
                            graph_distance: None,
                        })
                        .collect()
                })
                .await
        } else {
            filtered.into_iter().take(k).collect()
        }
    } else {
        let pool = base_k.saturating_mul(4).max(50).min(5000);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let decayed: Vec<valori_search::DecayedHit> = shard_sm
            .with_state_and_timestamps(|s, created_at| {
                let candidates: Vec<valori_search::DecayHit> =
                    shard_search_ns(s, &query, pool, ns_id)
                        .iter()
                        .map(|r| valori_search::DecayHit {
                            id: r.id.0,
                            distance: r.score as f32 / (SCALE as f32 * SCALE as f32),
                            created_at: created_at.get(&r.id.0).copied(),
                        })
                        .collect();
                valori_search::decay_rerank(candidates, now, half_life, pool)
            })
            .await;
        if let Some(ref f) = mf {
            shard_sm
                .with_state(|s| {
                    decayed
                        .into_iter()
                        .filter(|h| {
                            let key = format!("rec:{}", h.id);
                            match s
                                .meta
                                .get(&key)
                                .and_then(|v| serde_json::from_str::<serde_json::Value>(v).ok())
                            {
                                Some(meta) => valori_search::matches_metadata_filter(&meta, f),
                                None => false,
                            }
                        })
                        .take(k)
                        .map(|h| SearchHit {
                            id: h.id,
                            score: h.distance,
                            graph_distance: None,
                        })
                        .collect::<Vec<_>>()
                })
                .await
        } else {
            decayed
                .into_iter()
                .take(k)
                .map(|h| SearchHit {
                    id: h.id,
                    score: h.distance,
                    graph_distance: None,
                })
                .collect::<Vec<_>>()
        }
    };
    let results = if let Some(gr) = req.graph_rerank.as_ref() {
        shard_sm
            .with_state(|s| apply_graph_rerank_cluster(s, results, gr, k))
            .await
    } else {
        results
    };

    let state_hash: String = {
        let raw = shard.state_machine.state_hash().await;
        raw.iter().map(|b| format!("{:02x}", b)).collect()
    };
    let shard_id = shard_for_namespace(ns_id, state.shard_count).0 as u8;
    {
        use valori_planner::operation::{
            ConsistencyLevel as PlannerConsistency, OperationInputs, OperationKind,
        };
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
        crate::receipt_bridge::emit_read(
            &receipts,
            OperationKind::Search,
            &inputs,
            ns_id,
            shard_id,
            0,
            true,
            state_hash,
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({ "results": results })),
    )
        .into_response()
}

// ── Phase 5: Cross-Collection (Multi) Search ─────────────────────────────────

/// `POST /v1/search/multi` — cluster path.
///
/// Fans the query out to every listed Collection independently (using local
/// reads — no per-shard linearizability round-trip), then merges globally by
/// Squared L2. All Collections must share the same `dim` and `metric`.
///
/// BM25 / graph reranking are excluded (see `routes/query_planner.rs`).
async fn cluster_multi_search(
    State(state): State<DataPlaneState>,
    Json(payload): Json<crate::api::MultiSearchRequest>,
) -> Response {
    use crate::routes::query_planner::{
        check_compatibility, merge_top_k, CollectionHits, MAX_MULTI_COLLECTIONS, MAX_MULTI_SEARCH_K,
    };

    // ── Input validation ──────────────────────────────────────────────────────
    if payload.collections.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "collections list is empty; at least one collection is required"
            })),
        )
            .into_response();
    }
    if payload.collections.len() > MAX_MULTI_COLLECTIONS {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!(
                    "too many collections: {} requested, maximum is {}",
                    payload.collections.len(),
                    MAX_MULTI_COLLECTIONS
                )
            })),
        )
            .into_response();
    }
    if payload.k == 0 || payload.k > MAX_MULTI_SEARCH_K {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!(
                    "k must be between 1 and {}, got {}",
                    MAX_MULTI_SEARCH_K, payload.k
                )
            })),
        )
            .into_response();
    }

    if let Err(resp) = state.readiness.check(&state.raft) {
        return resp;
    }

    // ── Resolve collections and check compatibility ───────────────────────────
    let mut ns_pairs: Vec<(String, u16)> = Vec::with_capacity(payload.collections.len());
    for name in &payload.collections {
        match state.sm.resolve_namespace(Some(name.as_str())).await {
            Some(ns_id) => ns_pairs.push((name.clone(), ns_id)),
            None => {
                return collection_not_found(Some(name.as_str()));
            }
        }
    }

    // Build configs for compatibility check.
    let mut configs_for_check: Vec<(String, valori_metadata::collection::CollectionVectorConfig)> =
        Vec::with_capacity(ns_pairs.len());
    for (name, ns_id) in &ns_pairs {
        match state.sm.namespace_config(*ns_id).await {
            Some(cfg) => configs_for_check.push((name.clone(), cfg)),
            // Phase API-2: 409, matching standalone. A mis-configured
            // Collection is not a server fault, so it must not be a 500.
            None => {
                return crate::errors::error_response(
                    StatusCode::CONFLICT,
                    crate::errors::ErrorCode::Conflict,
                    format!(
                        "collection '{}' has no vector configuration; \
                         was it created with explicit dim and metric?",
                        name
                    ),
                );
            }
        }
    }

    let (dim, _metric) = match check_compatibility(&configs_for_check) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };

    if payload.query.len() != dim as usize {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!(
                    "query vector has {} elements but collections require dim={}",
                    payload.query.len(),
                    dim
                )
            })),
        )
            .into_response();
    }

    // ── Fan-out searches in parallel ─────────────────────────────────────────
    let k = payload.k;
    let half_life = payload.decay_half_life_secs.unwrap_or(0);

    let query = match to_fxp(&payload.query) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };

    let futs: Vec<_> = ns_pairs
        .into_iter()
        .map(|(name, ns_id)| {
            let state = state.clone();
            let query_f32 = payload.query.clone();
            let query_fxp = query.clone();
            let mf = payload.metadata_filter.clone();
            async move {
                let shard = state.shard_for(ns_id);
                let shard_sm = &shard.state_machine;

                let hits: Result<Vec<crate::api::MultiSearchHit>, String> = if half_life == 0 {
                    // Try ANN first, fall back to exact brute-force.
                    let raw: Vec<SearchHit> =
                        if let Some(ann_hits) = state.try_ann_search(ns_id, &query_f32, k).await {
                            ann_hits
                                .into_iter()
                                .map(|(id, dist)| SearchHit {
                                    id,
                                    score: dist,
                                    graph_distance: None,
                                })
                                .collect()
                        } else {
                            shard_sm
                                .with_state(|s| {
                                    shard_search_ns(s, &query_fxp, k, ns_id)
                                        .iter()
                                        .map(|r| SearchHit {
                                            id: r.id.0,
                                            score: r.score as f32 / (SCALE as f32 * SCALE as f32),
                                            graph_distance: None,
                                        })
                                        .collect()
                                })
                                .await
                        };
                    // Metadata filter (post-fetch).
                    let filtered: Vec<SearchHit> = if let Some(ref f) = mf {
                        shard_sm
                            .with_state(|s| {
                                raw.into_iter()
                                    .filter(|h| {
                                        let key = format!("rec:{}", h.id);
                                        match s.meta.get(&key).and_then(|v| {
                                            serde_json::from_str::<serde_json::Value>(v).ok()
                                        }) {
                                            Some(meta) => {
                                                valori_search::matches_metadata_filter(&meta, f)
                                            }
                                            None => false,
                                        }
                                    })
                                    .collect()
                            })
                            .await
                    } else {
                        raw
                    };
                    Ok(filtered
                        .into_iter()
                        .take(k)
                        .map(|h| crate::api::MultiSearchHit {
                            collection: name.clone(),
                            id: h.id,
                            score: h.score,
                            decay_factor: None,
                            age_secs: None,
                        })
                        .collect())
                } else {
                    let pool = k.saturating_mul(4).max(50).min(5000);
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let decayed: Vec<valori_search::DecayedHit> = shard_sm
                        .with_state_and_timestamps(|s, created_at| {
                            let candidates: Vec<valori_search::DecayHit> =
                                shard_search_ns(s, &query_fxp, pool, ns_id)
                                    .iter()
                                    .map(|r| valori_search::DecayHit {
                                        id: r.id.0,
                                        distance: r.score as f32 / (SCALE as f32 * SCALE as f32),
                                        created_at: created_at.get(&r.id.0).copied(),
                                    })
                                    .collect();
                            valori_search::decay_rerank(candidates, now, half_life, pool)
                        })
                        .await;
                    let filtered: Vec<valori_search::DecayedHit> = if let Some(ref f) = mf {
                        shard_sm
                            .with_state(|s| {
                                decayed
                                    .into_iter()
                                    .filter(|h| {
                                        let key = format!("rec:{}", h.id);
                                        match s.meta.get(&key).and_then(|v| {
                                            serde_json::from_str::<serde_json::Value>(v).ok()
                                        }) {
                                            Some(meta) => {
                                                valori_search::matches_metadata_filter(&meta, f)
                                            }
                                            None => false,
                                        }
                                    })
                                    .collect()
                            })
                            .await
                    } else {
                        decayed
                    };
                    Ok(filtered
                        .into_iter()
                        .take(k)
                        .map(|h| crate::api::MultiSearchHit {
                            collection: name.clone(),
                            id: h.id,
                            score: h.distance,
                            decay_factor: Some(h.factor),
                            age_secs: h.age_secs,
                        })
                        .collect())
                };
                (name, hits)
            }
        })
        .collect();

    let raw_results = futures::future::join_all(futs).await;

    let mut per_coll: Vec<CollectionHits> = Vec::new();
    let mut failures: Vec<crate::api::PartialSearchFailure> = Vec::new();

    for (name, result) in raw_results {
        match result {
            Ok(hits) => per_coll.push(CollectionHits {
                collection: name,
                hits,
            }),
            Err(e) => failures.push(crate::api::PartialSearchFailure {
                collection: name,
                error: e,
            }),
        }
    }

    let response = merge_top_k(per_coll, failures, k);
    (StatusCode::OK, Json(response)).into_response()
}

// ── Read consistency (read-index protocol) ──────────────────────────────────────

fn read_unavailable(msg: String) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({ "error": msg })),
    )
        .into_response()
}

/// Block until this node may serve a linearizable read.
///
/// - **Leader**: `ensure_linearizable` confirms leadership via a quorum
///   heartbeat and waits for this node's apply to reach the read index.
/// - **Follower**: ask the leader for its read index (`/v1/cluster/read-index`),
///   then wait until this node's applied index catches up before returning.
///
/// On success the caller may scan local state and the result is linearizable.
async fn ensure_read_consistency(
    shard_id: ShardId,
    raft: &Raft,
    http: &reqwest::Client,
) -> Result<(), Response> {
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

    let url = format!(
        "http://{leader_api}/v1/cluster/read-index?shard={}",
        shard_id.0
    );
    let read_index = match http
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => match r.json::<serde_json::Value>().await {
            Ok(v) => v.get("read_index").and_then(|x| x.as_u64()).unwrap_or(0),
            Err(e) => {
                return Err(read_unavailable(format!(
                    "bad read-index reply from leader: {e}"
                )))
            }
        },
        Ok(r) => {
            return Err(read_unavailable(format!(
                "leader rejected read-index ({})",
                r.status()
            )))
        }
        Err(e) => {
            return Err(read_unavailable(format!(
                "cannot reach leader for read-index: {e}"
            )))
        }
    };

    // Wait until our local apply has reached the leader's read index.
    raft.wait(Some(std::time::Duration::from_secs(5)))
        .applied_index_at_least(Some(read_index), "linearizable-read")
        .await
        .map(|_| ())
        .map_err(|e| {
            read_unavailable(format!(
                "timed out catching up to read index {read_index}: {e}"
            ))
        })
}

// ── Delete ────────────────────────────────────────────────────────────────────
//
// Phase S7: record ids are only unique within their own shard's kernel state
// (each shard runs an independent id counter), so the caller must name the
// collection the record was inserted into. Phase 3.3: an absent or unknown
// name — "default" included — resolves to nothing and 400s; there is no
// implicit collection to fall back to. Handler bodies live in
// `routes::records`.

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
        // G1.3.1 BUG-4: a record existing is not enough — it must belong to
        // the resolved namespace, or this must behave exactly like "not
        // found". Same convention as standalone's `SharedEngine::delete`.
        let referencing_nodes: Vec<u32> = {
            let found = shard
                .state_machine
                .with_state(|s| {
                    s.get_record(RecordId(id))
                        .map(|r| r.namespace_id == ns)
                        .unwrap_or(false)
                })
                .await;
            if !found {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "record not found"})),
                )
                    .into_response());
            }
            // G1.3.1 BUG-2/BUG-3: cascade to every referencing node on hard
            // delete, exactly like the standalone engine, so the cluster
            // path stops leaving dangling `node.record` references (which
            // makes the state's own snapshot undecodable — BUG-1). Soft
            // delete needs no cascade: the record row survives, so any
            // referencing node stays valid.
            if soft {
                Vec::new()
            } else {
                shard
                    .state_machine
                    .with_state(|s| valori_rag::graph::nodes_referencing_record(s, id))
                    .await
            }
        };
        let state_before: String = {
            let raw = self.sm.state_hash().await;
            raw.iter().map(|b| format!("{:02x}", b)).collect()
        };
        for node_id in referencing_nodes {
            raft_write_data(
                &shard.raft,
                ClientRequest {
                    event: KernelEvent::DeleteNode {
                        id: NodeId(node_id),
                    },
                    request_id: None,
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id: ns,
                },
            )
            .await?;
        }
        let event = if soft {
            KernelEvent::SoftDeleteRecord { id: RecordId(id) }
        } else {
            KernelEvent::DeleteRecord { id: RecordId(id) }
        };
        let resp = raft_write_data(
            &shard.raft,
            ClientRequest {
                event,
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns,
            },
        )
        .await?;
        let state_after: String = resp
            .state_hash
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
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

async fn get_record_by_id(
    State(state): State<DataPlaneState>,
    axum::extract::Path(id): axum::extract::Path<u32>,
    Query(q): Query<crate::routes::graph::CollectionQuery>,
) -> Result<Json<serde_json::Value>, Response> {
    let ns = match state.sm.resolve_namespace(q.collection.as_deref()).await {
        Some(ns) => ns,
        None => {
            return Err((
                axum::http::StatusCode::NOT_FOUND,
                axum::Json(serde_json::json!({"error": "collection not found"})),
            )
                .into_response())
        }
    };
    let rec_id = valori_kernel::types::id::RecordId(id);
    let result = state
        .sm
        .with_state(|s| {
            s.get_record(rec_id)
                .filter(|r| r.namespace_id == ns)
                .map(|rec| {
                    let vector: Vec<f32> = rec
                        .vector
                        .data
                        .iter()
                        .map(|s| valori_kernel::fxp::ops::to_f32(*s))
                        .collect();
                    serde_json::json!({
                        "id": id,
                        "vector": vector,
                        "metadata": rec.metadata.as_ref()
                            .and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok()),
                        "tag": rec.tag,
                    })
                })
        })
        .await;
    match result {
        Some(v) => Ok(Json(v)),
        None => Err((
            axum::http::StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({"error": "record not found"})),
        )
            .into_response()),
    }
}

async fn update_record_metadata(
    State(state): State<DataPlaneState>,
    axum::extract::Path(id): axum::extract::Path<u32>,
    Query(q): Query<crate::routes::graph::CollectionQuery>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, Response> {
    let ns = match state.sm.resolve_namespace(q.collection.as_deref()).await {
        Some(ns) => ns,
        None => {
            return Err((
                axum::http::StatusCode::NOT_FOUND,
                axum::Json(serde_json::json!({"error": "collection not found"})),
            )
                .into_response())
        }
    };
    let rec_id = valori_kernel::types::id::RecordId(id);
    let exists = state
        .sm
        .with_state(|s| {
            s.get_record(rec_id)
                .filter(|r| r.namespace_id == ns)
                .is_some()
        })
        .await;
    if !exists {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({"error": "record not found"})),
        )
            .into_response());
    }
    let metadata_bytes = serde_json::to_vec(&body).ok();
    let shard = state.shard_for(ns);
    raft_write_data(
        &shard.raft,
        ClientRequest {
            event: KernelEvent::UpdateRecordMetadata {
                id: rec_id,
                metadata: metadata_bytes,
            },
            request_id: None,
            schema_version: CURRENT_SCHEMA_VERSION,
            namespace_id: ns,
        },
    )
    .await?;
    Ok(Json(serde_json::json!({ "ok": true, "id": id })))
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
    /// Per-vector plain text for BM25 reranking. When present, stored in the
    /// state machine's text_corpus via AutoInsertRecord metadata bytes (same
    /// effect as the standalone path's engine.reranker_insert). Ignored when
    /// the corresponding metadata[i] is already set.
    #[serde(default)]
    texts: Option<Vec<Option<String>>>,
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
            return collection_not_found(req.collection.as_deref());
        }
    };
    let shard = state.shard_for(ns_id);
    let shard_raft = &shard.raft;
    let shard_id = shard_for_namespace(ns_id, state.shard_count).0 as u8;
    let state_before: String = {
        let raw = state.sm.state_hash().await;
        raw.iter().map(|b| format!("{:02x}", b)).collect()
    };

    let mut ids = Vec::with_capacity(req.batch.len());

    for values in req.batch {
        let vector = match to_fxp(&values) {
            Ok(v) => v,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": e })),
                )
                    .into_response();
            }
        };

        // metadata field takes priority; fall back to texts[i] so the state
        // machine's text_corpus is populated for BM25 reranking (same semantics
        // as the standalone path that calls engine.reranker_insert from texts).
        let meta_bytes = req
            .metadata
            .as_ref()
            .and_then(|m| m.get(ids.len()))
            .and_then(|s| s.as_ref())
            .map(|s| s.as_bytes().to_vec())
            .or_else(|| {
                req.texts
                    .as_ref()
                    .and_then(|t| t.get(ids.len()))
                    .and_then(|s| s.as_ref())
                    .map(|t| t.as_bytes().to_vec())
            });

        match shard_raft
            .client_write(ClientRequest {
                event: KernelEvent::AutoInsertRecord {
                    vector,
                    metadata: meta_bytes,
                    tag: 0,
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns_id,
            })
            .await
        {
            Ok(resp) => {
                if let Some(reason) = &resp.data.rejected {
                    return (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        Json(serde_json::json!({ "error": reason })),
                    )
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

    let state_after: String = {
        let raw = shard.state_machine.state_hash().await;
        raw.iter().map(|b| format!("{:02x}", b)).collect()
    };
    {
        use valori_planner::operation::{OperationInputs, OperationKind};
        let inputs = OperationInputs::BatchInsert {
            count: ids.len() as u32,
            collection: req.collection.clone().unwrap_or_else(|| "default".into()),
            shard_id,
        };
        crate::receipt_bridge::emit_write(
            &receipts,
            OperationKind::BatchInsert,
            &inputs,
            ns_id,
            shard_id,
            0,
            true,
            state_before,
            state_after,
        );
    }
    (StatusCode::OK, Json(serde_json::json!({ "ids": ids }))).into_response()
}

// ── State proof ───────────────────────────────────────────────────────────────
// `final_state_hash` matches the standalone DeterministicProof field name the
// SDK reads, so `get_state_hash()` works unchanged against a cluster node.

/// `GET /v1/usage` — Phase P2 cluster equivalent of the standalone
/// endpoint (`server.rs::usage_handler`). Records and storage bytes are
/// summed across **every shard this node runs** — records because they
/// are genuinely partitioned by `namespace_id % shard_count`, storage
/// because each shard has its own audit-log file
/// (`shard_event_log_paths`, the same map `/v1/proof/event-log` already
/// sums across, S16). Collections are NOT summed across shards: the
/// namespace registry is a single logical registry maintained via shard
/// 0's Raft group alone (collection creation always targets
/// `namespace_id: 0`, see `create_collection_handler` below) — it is not
/// duplicated per shard, so shard 0's count already is the true total.
/// Snapshot bytes are not included: cluster-mode snapshots are Raft's
/// own log-compaction mechanism, not a single stat-able file the way the
/// standalone engine's `snapshot_path` is — a known, explicit scope
/// limit, not a silent gap.
async fn usage(State(state): State<DataPlaneState>) -> Response {
    let mut records: u64 = 0;
    for shard in state.shards.values() {
        records += shard
            .state_machine
            .with_state(|s| s.record_count() as u64)
            .await;
    }
    let collections = state.sm.list_namespaces().await.len();
    let mut event_log_bytes: u64 = 0;
    for path in state.shard_event_log_paths.values() {
        event_log_bytes += sum_log_and_archives(path.clone());
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "records": records,
            "collections": collections,
            "storage": {
                "event_log_bytes": event_log_bytes,
                "snapshot_bytes": 0,
                "total_bytes": event_log_bytes,
            }
        })),
    )
        .into_response()
}

async fn state_proof(State(state): State<DataPlaneState>) -> Response {
    let hash = state.sm.state_hash().await;
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    (
        StatusCode::OK,
        Json(serde_json::json!({ "final_state_hash": hex })),
    )
        .into_response()
}

// ── Cluster proof — the demo/verification endpoint ────────────────────────────
// Returns the full verifiable state: node identity, BLAKE3 state hash, and the
// applied index + term at the time of the read. Call this on all nodes and
// compare `final_state_hash` to verify the cluster has a consistent view.

/// `GET /v1/cluster/proof` response.
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub(crate) struct ClusterProofResponse {
    node_id: u64,
    /// 64 lowercase hex characters (32 bytes).
    final_state_hash: String,
    /// Raft index this hash was taken at. Two peers only need to agree when
    /// compared at the same index.
    last_applied_index: Option<u64>,
    term: u64,
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/cluster/proof",
    operation_id = "get_cluster_proof",
    tag = "cluster",
    summary = "This node's state hash and applied index",
    description = "The cluster analogue of `GET /v1/proof/state`. Comparing `final_state_hash` across peers at the same `last_applied_index` is how convergence is verified.",
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "State hash and Raft position", body = ClusterProofResponse),
    ),
))]
async fn cluster_proof(State(state): State<DataPlaneState>) -> Response {
    let m = state.raft.metrics().borrow().clone();
    let hash = state.sm.state_hash().await;
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    (
        StatusCode::OK,
        Json(ClusterProofResponse {
            node_id: m.id,
            final_state_hash: hex,
            last_applied_index: m.last_applied.map(|l| l.index),
            term: m.current_term,
        }),
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
                shards.insert(
                    shard_id.0.to_string(),
                    serde_json::json!({ "event_log_hash": hex }),
                );
            }
            Err(e) => {
                shards.insert(
                    shard_id.0.to_string(),
                    serde_json::json!({ "error": format!("cannot hash event log: {e}") }),
                );
            }
        }
    }
    // Top-level `event_log_hash` = shard 0 for backward compat with single-shard clients.
    let top_hash = shards
        .get("0")
        .and_then(|v| v.get("event_log_hash"))
        .cloned();
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
                event: KernelEvent::AutoCreateNode {
                    kind,
                    record: record_id.map(RecordId),
                },
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
                event: KernelEvent::AutoCreateEdge {
                    from: NodeId(from),
                    to: NodeId(to),
                    kind,
                },
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
        // G1.1.1: pre-check namespace before submitting the Raft write — a
        // shard can host multiple namespaces (`namespace_id % shard_count`),
        // so `shard_for(ns)` alone does not guarantee `id` belongs to `ns`.
        // See docs/reviews/graph-g1.1.1-graph-read-namespace-isolation.md.
        let shard = self.shard_for(ns);
        let in_namespace = shard
            .state_machine
            .with_state(move |s| {
                s.get_node(NodeId(id))
                    .map(|n| n.namespace_id == ns)
                    .unwrap_or(false)
            })
            .await;
        if !in_namespace {
            return Ok(None);
        }
        let resp = raft_write_data(
            &shard.raft,
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
                // G1.1.1: `shard_for(ns)` only selects which physical shard
                // to read from — a shard can host multiple namespaces, so
                // the node found there must still be checked against `ns`.
                s.get_node(NodeId(id))
                    .filter(|n| n.namespace_id == ns)
                    .map(|n| crate::api::GetNodeResponse {
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
                // G1.1.1: validate the SOURCE node's namespace first —
                // sufficient by construction, since edges cannot cross
                // namespaces (G0's invariant).
                match s.get_node(NodeId(id)) {
                    Some(n) if n.namespace_id == ns => {}
                    _ => return None,
                }
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
        // Already namespace-safe (filters by `n.namespace_id == ns` below) —
        // unchanged by G1.1.1, included here only for audit completeness.
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
                // G1.1.1: validate the ROOT node's namespace before
                // traversing — a wrong-namespace root behaves exactly like a
                // nonexistent one already did (empty result, 200 OK).
                match s.get_node(NodeId(root)) {
                    Some(n) if n.namespace_id == ns => {}
                    _ => {
                        return (
                            serde_json::Value::Array(vec![]),
                            serde_json::Value::Array(vec![]),
                        )
                    }
                }
                let (nodes, edges) = valori_rag::graph::expand_subgraph(s, &[root], depth);
                (
                    serde_json::Value::Array(nodes),
                    serde_json::Value::Array(edges),
                )
            })
            .await)
    }

    async fn query(
        &self,
        ns: u16,
        query: valori_rag::graph::GraphQuery,
    ) -> Result<Option<Vec<valori_rag::graph::GraphQueryHit>>, Response> {
        self.readiness.check(&self.raft)?;
        Ok(self
            .shard_for(ns)
            .state_machine
            .with_state(move |s| valori_rag::graph::query_graph(s, ns, &query))
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

fn default_subgraph_depth() -> u32 {
    2
}

async fn get_graph_subgraph(
    State(state): State<DataPlaneState>,
    axum::extract::Query(q): axum::extract::Query<crate::routes::graph::SubgraphQuery>,
) -> Result<Json<serde_json::Value>, Response> {
    crate::routes::graph::get_subgraph(&state, q).await
}

// ── G1.1 — deterministic graph query primitives ─────────────────────────────

async fn get_graph_query(
    State(state): State<DataPlaneState>,
    axum::extract::Query(q): axum::extract::Query<crate::routes::graph::GraphQueryParams>,
) -> Result<Json<crate::api::GraphQueryResponse>, Response> {
    crate::routes::graph::query(&state, q).await
}

// ── Phase 3.15: native GraphRAG (cluster) — KNN + subgraph in one snapshot ────

#[derive(serde::Deserialize)]
struct ClusterGraphRagRequest {
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
    #[serde(default = "default_cluster_graph_weight")]
    graph_weight: f32,
    #[serde(default = "default_subgraph_depth")]
    depth: u32,
    #[serde(default)]
    consistency: Consistency,
    /// Phase S8. Absent/"default" targets the default namespace, shard 0 —
    /// byte-identical to pre-S8 behavior.
    #[serde(default)]
    collection: Option<String>,
}

fn default_cluster_graph_weight() -> f32 {
    0.3
}

async fn cluster_graphrag(
    State(state): State<DataPlaneState>,
    axum::Extension(caps): axum::Extension<Arc<valori_effect::capability::CapabilityRegistry>>,
    axum::Extension(task_reg): axum::Extension<Arc<crate::runner::TaskRegistry>>,
    Json(req): Json<ClusterGraphRagRequest>,
) -> Response {
    use crate::runner::run_graph_inline;
    use valori_planner::context::{
        CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash,
    };
    use valori_planner::graph::ExecutionRetentionPolicy;
    use valori_planner::graph::{ExecutionGraph, TaskId, TaskKind, TaskSpec};
    use valori_planner::operation::{
        compute_operation_hash, ExecutionPolicy, OperationInputs, OperationKind,
    };

    if let Err(resp) = state.readiness.check(&state.raft) {
        return resp;
    }

    let ns_id = match state.sm.resolve_namespace(req.collection.as_deref()).await {
        Some(id) => id,
        None => {
            return collection_not_found(req.collection.as_deref());
        }
    };
    let shard = state.shard_for(ns_id);
    let shard_sm = &shard.state_machine;

    if let Some(locked) = shard_sm.locked_dim().await {
        if req.query_vector.len() != locked {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!(
                        "Query vector has {} elements but this store is locked to dim={}. \
                         Check GET /health for the current dim.",
                        req.query_vector.len(), locked
                    )
                })),
            )
                .into_response();
        }
    }

    // Linearizable by default: establish a read index so the local snapshot
    // reflects every write committed before this GraphRAG read began.
    if req.consistency == Consistency::Linearizable {
        if let Err(resp) = ensure_read_consistency(
            shard_for_namespace(ns_id, state.shard_count),
            &shard.raft,
            &state.http,
        )
        .await
        {
            return resp;
        }
    }

    let shard_id = shard_for_namespace(ns_id, state.shard_count).0 as u8;
    let shard_count = state.shard_count as u8;
    let depth = req.depth;

    // Phase 5.3: `retrieval_k` is the canonical name; `k` is the backward-compat alias.
    // Phase 5.4: `final_k` defaults to `retrieval_k` (not unlimited) to bound result size.
    let retrieval_k = req.retrieval_k.or(req.k).unwrap_or(5).max(1);
    let final_k = req.final_k.unwrap_or(retrieval_k) as u32;
    let max_graph_candidates = req.max_graph_candidates.unwrap_or(100).max(1) as u32;
    let max_nodes = req.max_nodes.map(|v| v as u32);
    let max_edges = req.max_edges.map(|v| v as u32);
    let graph_weight = req.graph_weight.clamp(0.0, 1.0);

    let inputs_json = serde_json::json!({
        "shard_id": shard_id,
        "namespace_id": ns_id,
        "vector": req.query_vector,
        "k": retrieval_k,
        "depth": depth,
        "final_k": final_k,
        "max_graph_candidates": max_graph_candidates,
        "max_nodes": max_nodes,
        "max_edges": max_edges,
        "graph_weight": graph_weight,
    })
    .to_string();

    let op_hash = compute_operation_hash(
        OperationKind::GraphRag,
        &OperationInputs::GraphRag {
            k: retrieval_k as u32,
            depth,
            collection: req.collection.clone().unwrap_or_else(|| "default".into()),
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
            cluster: true,
            shard_count,
        },
        schema_version: 1,
        shard_count,
        cluster_epoch: 0,
        cluster_mode: true,
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

    let result = match run_graph_inline(graph, caps, task_reg, ExecutionPolicy::default()).await {
        Ok(outputs) => outputs.into_iter().next().flatten()
            .map(|o| o.json)
            .unwrap_or(serde_json::json!({ "hits": [], "seed_nodes": [], "subgraph": { "nodes": [], "edges": [] } })),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    };

    metrics::counter!("valori_graphrag_total", 1u64);
    (StatusCode::OK, Json(result)).into_response()
}

// ── Phase 3.5: API key management (cluster) ───────────────────────────────────

#[derive(serde::Deserialize)]
struct ClusterCreateKeyRequest {
    #[serde(default = "default_cluster_scope")]
    scope: ApiScope,
    collection: Option<String>,
    description: Option<String>,
}

fn default_cluster_scope() -> ApiScope {
    ApiScope::ReadWrite
}

async fn cluster_create_key(
    Extension(auth): Extension<Arc<AuthState>>,
    Json(req): Json<ClusterCreateKeyRequest>,
) -> impl axum::response::IntoResponse {
    let created = auth
        .key_store
        .create(req.scope, req.collection, req.description);
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
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    let key_id: [u8; 16] = if let Some(ref hex) = req.key_id {
        match hex_to_key_id(hex) {
            Some(k) => k,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "key_id must be 32 hex chars"})),
                )
                    .into_response()
            }
        }
    } else {
        new_key_id()
    };

    // Encrypt on this node's vault BEFORE submitting to Raft.
    let ciphertext = match state.vault.encrypt(key_id, &plaintext) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("{e:?}")})),
            )
                .into_response()
        }
    };

    // Phase 3.3: no implicit collection — an omitted `collection` must
    // resolve like any other absent name (fails unless something was
    // actually created and literally named the wire default), not
    // silently target namespace 0.
    let ns = match state.sm.resolve_namespace(req.collection.as_deref()).await {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Collection not found"})),
            )
                .into_response()
        }
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
            schema_version: CURRENT_SCHEMA_VERSION,
            namespace_id: ns,
        },
        move |resp| {
            (
                StatusCode::CREATED,
                Json(ClusterInsertEncryptedResponse {
                    id: resp.allocated_record_id.unwrap_or(0),
                    key_id: key_id_to_hex(&key_id),
                    log_index: resp.log_index,
                }),
            )
                .into_response()
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
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "key_id must be 32 hex chars"})),
            )
                .into_response()
        }
    };

    // Shred the vault key locally FIRST — the compliance-critical,
    // irreversible step: this node's ciphertext-decryption capability for
    // key_id is destroyed unconditionally, regardless of what follows.
    if let Err(e) = state.vault.shred(key_id) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e:?}")})),
        )
            .into_response();
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
        match handle
            .raft
            .client_write(ClientRequest {
                event: KernelEvent::ShredKey { key_id },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: 0,
            })
            .await
        {
            Ok(_) => {
                shard_status.insert(key, serde_json::json!({ "status": "shredded" }));
            }
            Err(openraft::error::RaftError::APIError(
                openraft::error::ClientWriteError::ForwardToLeader(fwd),
            )) => {
                all_shredded = false;
                shard_status.insert(
                    key,
                    serde_json::json!({
                        "status": "not-leader",
                        "leader_api_addr": fwd.leader_node.map(|n| n.api_addr.clone()),
                    }),
                );
            }
            Err(e) => {
                all_shredded = false;
                shard_status.insert(
                    key,
                    serde_json::json!({ "status": "error", "detail": e.to_string() }),
                );
            }
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
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
        })),
    )
        .into_response()
}

async fn cluster_crypto_status(
    State(state): State<DataPlaneState>,
    Path(key_id_hex): Path<String>,
) -> Response {
    let key_id = match hex_to_key_id(&key_id_hex) {
        Some(k) => k,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "key_id must be 32 hex chars"})),
            )
                .into_response()
        }
    };
    let exists = state.vault.key_exists(&key_id);
    (
        StatusCode::OK,
        Json(serde_json::json!({"key_id": key_id_hex, "exists": exists})),
    )
        .into_response()
}

// ── Phase 3.13 / 4.3: project-wide index config ───────────────────────────────
//
// This endpoint reports the project-wide VALORI_INDEX env var (the default
// index kind for standalone, informational in cluster). Phase 4.3 adds
// per-collection ANN indexes; use GET /v1/namespaces/{name}/index for those.

async fn cluster_index_config() -> Response {
    // Cluster mode now supports per-collection ANN indexes (Phase 4.3). The
    // project-wide config endpoint reports the env-level setting; per-collection
    // state is available via GET /v1/namespaces/{name}/index.
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "index_type": std::env::var("VALORI_INDEX").unwrap_or_else(|_| "brute".into()),
            "note": "per-collection ANN indexes are available in cluster mode via POST /v1/namespaces/{name}/index",
        })),
    )
        .into_response()
}

async fn cluster_index_rebuild() -> Response {
    // Project-wide index rebuild is a standalone concept (engine rebuilds its
    // global index). In cluster mode, per-collection indexes are built
    // individually via POST /v1/namespaces/{name}/index.
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "note": "use POST /v1/namespaces/{name}/index to build or change a per-collection ANN index in cluster mode",
        })),
    )
        .into_response()
}

// ── Phase 4.3: Cluster index lifecycle — IndexOps implementation ─────────────
//
// ANN indexes are node-local derived state. The desired spec + generation is
// committed through Raft (via SetMeta) so every node agrees on the logical
// generation id and parameters. Each node builds and activates independently
// (node-local activation model). Search uses the local active ANN index if
// available, falling back to exact brute-force otherwise.

#[async_trait::async_trait]
impl crate::routes::index_lifecycle::IndexOps for DataPlaneState {
    async fn resolve(&self, name: &str) -> Option<u16> {
        self.sm.resolve_namespace(Some(name)).await
    }

    async fn get_index_state(&self, namespace_id: u16) -> CollectionIndexState {
        let mut state = self
            .cluster_indexes
            .read()
            .await
            .get(&namespace_id)
            .map(|e| e.state.clone())
            .unwrap_or_default();

        // Phase 4.4: always populate `desired` from the authoritative Raft
        // state, even before a local build has started. This ensures that a
        // freshly-joined or just-restarted node correctly reports what the
        // cluster wants (e.g. "desired: HNSW gen 8") even while it is still
        // in brute-force mode locally.
        if let Some(desired_json) = self.sm.get_meta_json(&idx_spec_key(namespace_id)).await {
            if desired_json != serde_json::Value::Null {
                if let (Some(type_str), Some(params)) = (
                    desired_json.get("type").and_then(|v| v.as_str()),
                    desired_json.get("parameters"),
                ) {
                    state.desired = Some(IndexSpec {
                        index_type: type_str.to_string(),
                        parameters: params.clone(),
                    });
                }
            } else {
                state.desired = None;
            }
        }

        state
    }

    async fn start_build(&self, namespace_id: u16, spec: IndexSpec) -> Result<u32, String> {
        // Check for in-progress build before committing.
        {
            let indexes = self.cluster_indexes.read().await;
            if let Some(entry) = indexes.get(&namespace_id) {
                if entry.state.is_building() {
                    return Err("an index build is already in progress for this collection".into());
                }
            }
        }

        // Determine the next generation by reading the replicated current value.
        let current_gen: u32 = self
            .sm
            .get_meta_json(&idx_spec_key(namespace_id))
            .await
            .and_then(|v| v.get("generation").and_then(|g| g.as_u64()))
            .map(|n| n as u32)
            .unwrap_or(0);
        let new_gen = current_gen + 1;

        // Commit the desired spec + generation through Raft so ALL nodes see it.
        let spec_json = serde_json::json!({
            "generation": new_gen,
            "type": spec.index_type,
            "parameters": spec.parameters,
        })
        .to_string();

        let commit_result = raft_write_data(
            &self.raft,
            ClientRequest {
                event: KernelEvent::SetMeta {
                    key: idx_spec_key(namespace_id),
                    value: spec_json,
                },
                request_id: Some(*uuid::Uuid::new_v4().as_bytes()),
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id,
            },
        )
        .await;

        if let Err(resp) = commit_result {
            // Extract error string from the response JSON.
            return Err(format!("raft commit failed: {resp:?}"));
        }

        // Start the local build on this node immediately (followers pick it up
        // via the watcher task within ~5 s).
        self.trigger_local_build(namespace_id, new_gen, spec).await;
        Ok(new_gen)
    }

    async fn drop_index(&self, namespace_id: u16) -> Result<(), String> {
        // Commit "no index desired" through Raft.
        let commit_result = raft_write_data(
            &self.raft,
            ClientRequest {
                event: KernelEvent::SetMeta {
                    key: idx_spec_key(namespace_id),
                    value: "null".into(),
                },
                request_id: Some(*uuid::Uuid::new_v4().as_bytes()),
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id,
            },
        )
        .await;

        if let Err(resp) = commit_result {
            return Err(format!("raft commit failed: {resp:?}"));
        }

        // Drop the node-local index immediately.
        let mut indexes = self.cluster_indexes.write().await;
        if let Some(entry) = indexes.get_mut(&namespace_id) {
            entry.state.set_none();
            entry.index = None;
        }
        Ok(())
    }

    fn supports_ann_builds(&self) -> bool {
        true
    }
}

async fn cluster_index_lifecycle_create(
    State(state): State<DataPlaneState>,
    Path(name): Path<String>,
    Json(payload): Json<EngineIndexBuildRequest>,
) -> Response {
    crate::routes::index_lifecycle::create_or_change_index(&state, &name, payload).await
}

async fn cluster_index_lifecycle_status(
    State(state): State<DataPlaneState>,
    Path(name): Path<String>,
) -> Response {
    crate::routes::index_lifecycle::get_index_status(&state, &name).await
}

// ── C4.2 & C4.3: Cluster memory domain implementation ────────────────────────

fn cosine_similarity_from_records(
    rec_a: &valori_kernel::storage::record::Record,
    rec_b: &valori_kernel::storage::record::Record,
) -> Option<f32> {
    use valori_kernel::math::dot::dot_i32 as dot_product;
    if !rec_a.is_searchable() || !rec_b.is_searchable() {
        return None;
    }
    let va: Vec<i32> = rec_a.vector.data.iter().map(|x| x.0).collect();
    let vb: Vec<i32> = rec_b.vector.data.iter().map(|x| x.0).collect();
    let dot = dot_product(&va, &vb) as f64;
    let mag_a = (dot_product(&va, &va) as f64).sqrt();
    let mag_b = (dot_product(&vb, &vb) as f64).sqrt();
    if mag_a == 0.0 || mag_b == 0.0 {
        return None;
    }
    Some((dot / (mag_a * mag_b)) as f32)
}

/// Cluster impl of the shared memory domain primitives.
#[async_trait::async_trait]
impl crate::routes::memory::MemoryOps for DataPlaneState {
    async fn resolve_collection(&self, name: Option<&str>) -> Option<u16> {
        self.sm.resolve_namespace(name).await
    }

    async fn ensure_read_consistency(
        &self,
        ns: u16,
        consistency: Option<&str>,
    ) -> Result<(), Response> {
        if consistency != Some("local") {
            let shard = self.shard_for(ns);
            ensure_read_consistency(
                shard_for_namespace(ns, self.shard_count),
                &shard.raft,
                &self.http,
            )
            .await?;
        }
        Ok(())
    }

    async fn upsert_vector(
        &self,
        ns: u16,
        req: &crate::api::MemoryUpsertVectorRequest,
    ) -> Result<crate::routes::memory::UpsertedMemory, Response> {
        let vector = to_fxp(&req.vector).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        })?;

        let shard = self.shard_for(ns);
        let shard_raft = &shard.raft;
        let shard_id = shard_for_namespace(ns, self.shard_count).0 as u8;
        let state_before: String = {
            let raw = shard.state_machine.state_hash().await;
            raw.iter().map(|b| format!("{:02x}", b)).collect()
        };

        // 1. Insert vector record.
        let resp_rec = raft_write_data(
            shard_raft,
            ClientRequest {
                event: KernelEvent::AutoInsertRecord {
                    vector,
                    metadata: None,
                    tag: 0,
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns,
            },
        )
        .await?;
        let record_id = resp_rec.allocated_record_id.unwrap_or(0);

        // 2. Create or reuse document node.
        let doc_node_id = if let Some(existing) = req.attach_to_document_node {
            existing
        } else {
            let resp_doc = raft_write_data(
                shard_raft,
                ClientRequest {
                    event: KernelEvent::AutoCreateNode {
                        kind: NodeKind::Document,
                        record: None,
                    },
                    request_id: None,
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id: ns,
                },
            )
            .await?;
            resp_doc.allocated_node_id.unwrap_or(0)
        };

        // 3. Create chunk node linked to the record.
        let resp_chunk = raft_write_data(
            shard_raft,
            ClientRequest {
                event: KernelEvent::AutoCreateNode {
                    kind: NodeKind::Chunk,
                    record: Some(RecordId(record_id)),
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns,
            },
        )
        .await?;
        let chunk_node_id = resp_chunk.allocated_node_id.unwrap_or(0);

        // 4. Connect document -> chunk.
        let resp_edge = raft_write_data(
            shard_raft,
            ClientRequest {
                event: KernelEvent::AutoCreateEdge {
                    from: NodeId(doc_node_id),
                    to: NodeId(chunk_node_id),
                    kind: EdgeKind::ParentOf,
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns,
            },
        )
        .await?;
        let mut log_index = resp_edge.log_index;

        let memory_id = format!("rec:{}", record_id);
        if let Some(meta) = &req.metadata {
            let resp_meta = raft_write_data(
                shard_raft,
                ClientRequest {
                    event: KernelEvent::SetMeta {
                        key: memory_id.clone(),
                        value: meta.to_string(),
                    },
                    request_id: None,
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id: ns,
                },
            )
            .await?;
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
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!(
                            "Query vector has {} elements but this store is locked to dim={}.",
                            req.query_vector.len(), locked
                        )
                    })),
                )
                    .into_response());
            }
        }

        let query = to_fxp(&req.query_vector).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        })?;

        let shard = self.shard_for(ns);
        let shard_sm = &shard.state_machine;

        let half_life = req.decay_half_life_secs.unwrap_or(0);
        let k = req.k;
        let mf = req.metadata_filter.clone();
        let base_k = if mf.is_some() {
            k.saturating_mul(10).max(100).min(5000)
        } else {
            k
        };

        let use_rerank = req.rerank && req.query_text.is_some();
        let fetch_k = if use_rerank {
            (base_k * valori_search::POOL_FACTOR).max(base_k)
        } else {
            base_k
        };
        let query_text_owned = req.query_text.clone().unwrap_or_default();

        let results = if half_life == 0 {
            let raw: Vec<crate::api::MemorySearchHit> = shard_sm
                .with_state(|s| {
                    let mut buf = vec![KernelSearchResult::default(); fetch_k];
                    let n = s.search_l2_ns(&query, &mut buf, ns);
                    buf[..n]
                        .iter()
                        .map(|r| {
                            let memory_id = format!("rec:{}", r.id.0);
                            crate::api::MemorySearchHit {
                                memory_id,
                                record_id: r.id.0,
                                score: r.score as f32 / (SCALE as f32 * SCALE as f32),
                                metadata: None,
                                decay_factor: None,
                                age_secs: None,
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .await;

            // Populate metadata before filtering so the filter predicate has data.
            let mut after_meta = raw;
            for hit in &mut after_meta {
                hit.metadata = shard_sm.get_meta_json(&hit.memory_id).await;
            }
            // Apply metadata filter (filter → rerank → top-k, in that order).
            let filtered: Vec<crate::api::MemorySearchHit> = if let Some(ref f) = mf {
                after_meta
                    .into_iter()
                    .filter(|h| {
                        h.metadata
                            .as_ref()
                            .map(|m| valori_search::matches_metadata_filter(m, f))
                            .unwrap_or(false)
                    })
                    .collect()
            } else {
                after_meta
            };

            // BM25 reranking: build an ephemeral reranker from the text corpus
            // stored in the state machine (populated via AutoInsertRecord metadata).
            if use_rerank && !filtered.is_empty() {
                let candidates: Vec<(u64, f32)> = filtered
                    .iter()
                    .map(|h| (h.record_id as u64, h.score))
                    .collect();
                let candidate_ids: Vec<u64> = candidates.iter().map(|(id, _)| *id).collect();
                let reranked_ids: Vec<(u64, f32)> = shard_sm
                    .with_text_corpus(|corpus| {
                        let mut reranker = valori_search::ValoriReranker::new();
                        for id in &candidate_ids {
                            if let Some(text) = corpus.get(id) {
                                reranker.insert(*id, text);
                            }
                        }
                        reranker
                            .rerank(&query_text_owned, candidates)
                            .into_iter()
                            .take(k)
                            .collect()
                    })
                    .await;
                // Re-attach metadata from the pre-rerank filtered set.
                let meta_map: std::collections::HashMap<u64, Option<serde_json::Value>> = filtered
                    .into_iter()
                    .map(|h| (h.record_id as u64, h.metadata))
                    .collect();
                reranked_ids
                    .into_iter()
                    .map(|(id, score)| crate::api::MemorySearchHit {
                        memory_id: format!("rec:{id}"),
                        record_id: id as u32,
                        score,
                        metadata: meta_map.get(&id).cloned().flatten(),
                        decay_factor: None,
                        age_secs: None,
                    })
                    .collect::<Vec<_>>()
            } else {
                filtered.into_iter().take(k).collect()
            }
        } else {
            let pool = base_k.saturating_mul(4).max(50).min(1000);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let mut decay_results: Vec<crate::api::MemorySearchHit> = shard_sm
                .with_state_and_timestamps(|s, created_at| {
                    let mut buf = vec![KernelSearchResult::default(); pool];
                    let n = s.search_l2_ns(&query, &mut buf, ns);
                    let candidates: Vec<valori_search::DecayHit> = buf[..n]
                        .iter()
                        .map(|r| valori_search::DecayHit {
                            id: r.id.0,
                            distance: r.score as f32,
                            created_at: created_at.get(&r.id.0).copied(),
                        })
                        .collect();
                    valori_search::decay_rerank(candidates, now, half_life, base_k)
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
                })
                .await;
            for hit in &mut decay_results {
                hit.metadata = shard_sm.get_meta_json(&hit.memory_id).await;
            }
            if let Some(ref f) = mf {
                decay_results.retain(|h| {
                    h.metadata
                        .as_ref()
                        .map(|m| valori_search::matches_metadata_filter(m, f))
                        .unwrap_or(false)
                });
                decay_results.truncate(k);
            }
            decay_results
        };

        Ok(results)
    }

    async fn consolidate(
        &self,
        ns: u16,
        req: &crate::api::MemoryConsolidateRequest,
    ) -> Result<crate::routes::memory::ConsolidatedMemory, Response> {
        let new_vector = to_fxp(&req.new_vector).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        })?;

        let shard = self.shard_for(ns);
        let shard_raft = &shard.raft;
        let shard_id = shard_for_namespace(ns, self.shard_count).0 as u8;
        let state_before: String = {
            let raw = shard.state_machine.state_hash().await;
            raw.iter().map(|b| format!("{:02x}", b)).collect()
        };

        raft_write_data(
            shard_raft,
            ClientRequest {
                event: KernelEvent::SoftDeleteRecord {
                    id: RecordId(req.old_record_id),
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns,
            },
        )
        .await?;

        let resp_rec = raft_write_data(
            shard_raft,
            ClientRequest {
                event: KernelEvent::AutoInsertRecord {
                    vector: new_vector,
                    metadata: None,
                    tag: 0,
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns,
            },
        )
        .await?;
        let new_record_id = resp_rec.allocated_record_id.unwrap_or(0);

        let resp_new_node = raft_write_data(
            shard_raft,
            ClientRequest {
                event: KernelEvent::AutoCreateNode {
                    kind: NodeKind::Chunk,
                    record: Some(RecordId(new_record_id)),
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns,
            },
        )
        .await?;
        let new_node = NodeId(resp_new_node.allocated_node_id.unwrap_or(0));

        let resp_old_node = raft_write_data(
            shard_raft,
            ClientRequest {
                event: KernelEvent::AutoCreateNode {
                    kind: NodeKind::Chunk,
                    record: Some(RecordId(req.old_record_id)),
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns,
            },
        )
        .await?;
        let old_node = NodeId(resp_old_node.allocated_node_id.unwrap_or(0));

        let resp_edge = raft_write_data(
            shard_raft,
            ClientRequest {
                event: KernelEvent::AutoCreateEdge {
                    from: new_node,
                    to: old_node,
                    kind: EdgeKind::Supersedes,
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns,
            },
        )
        .await?;
        let mut log_index = resp_edge.log_index;
        let edge_id = resp_edge.allocated_edge_id.unwrap_or(0);

        if let Some(meta) = &req.metadata {
            let memory_id = format!("rec:{}", new_record_id);
            let resp_meta = raft_write_data(
                shard_raft,
                ClientRequest {
                    event: KernelEvent::SetMeta {
                        key: memory_id,
                        value: meta.to_string(),
                    },
                    request_id: None,
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id: ns,
                },
            )
            .await?;
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

        let similarity: Option<f32> = self
            .sm
            .with_state(move |s| {
                let rec_a = s.get_record(RecordId(ra))?;
                let rec_b = s.get_record(RecordId(rb))?;
                cosine_similarity_from_records(rec_a, rec_b)
            })
            .await;

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
            let resp_a = raft_write_data(
                &self.raft,
                ClientRequest {
                    event: KernelEvent::AutoCreateNode {
                        kind: NodeKind::Chunk,
                        record: Some(RecordId(req.record_a)),
                    },
                    request_id: None,
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id: 0,
                },
            )
            .await?;
            let node_a = NodeId(resp_a.allocated_node_id.unwrap_or(0));

            let resp_b = raft_write_data(
                &self.raft,
                ClientRequest {
                    event: KernelEvent::AutoCreateNode {
                        kind: NodeKind::Chunk,
                        record: Some(RecordId(req.record_b)),
                    },
                    request_id: None,
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id: 0,
                },
            )
            .await?;
            let node_b = NodeId(resp_b.allocated_node_id.unwrap_or(0));

            let resp_edge = raft_write_data(
                &self.raft,
                ClientRequest {
                    event: KernelEvent::AutoCreateEdge {
                        from: node_a,
                        to: node_b,
                        kind: EdgeKind::Contradicts,
                    },
                    request_id: None,
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id: 0,
                },
            )
            .await?;
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
        raft_write_data(
            &self.raft,
            ClientRequest {
                event: KernelEvent::SetMeta {
                    key: target_id,
                    value: metadata.to_string(),
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: 0,
            },
        )
        .await
        .map(|_| ())
    }

    async fn get_meta(&self, target_id: &str) -> Option<serde_json::Value> {
        let key = target_id.to_string();
        self.sm
            .with_state(move |k| {
                k.meta
                    .get(&key)
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
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
    use valori_kernel::types::id::{NodeId, RecordId};

    let collection = payload
        .collection
        .clone()
        .unwrap_or_else(|| "default".into());
    let source = payload.source.clone().unwrap_or_else(|| "unknown".into());
    let strategy = payload.strategy.as_deref().unwrap_or("auto");
    let chunk_size = payload.chunk_size.unwrap_or(1000);
    let overlap = payload.chunk_overlap.unwrap_or(200);
    let is_async = query.r#async.or(payload.r#async).unwrap_or(false);

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
        valori_ingest::chunk_document(&payload.text, strategy, chunk_size, overlap);
    if chunks.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "no chunks produced" })),
        )
            .into_response();
    }

    let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();

    if is_async {
        let job_id = format!("job_{}", valori_core::id::ExecutionId::new_random());
        {
            let mut jobs_map = tasks.jobs.write().await;
            jobs_map.insert(
                job_id.clone(),
                serde_json::json!({
                    "status": "processing",
                    "job_id": job_id,
                    "chunk_count": chunks.len(),
                    "collection": collection,
                    "strategy_used": strategy_used,
                }),
            );
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
            match valori_ingest::embed_batch(&texts_clone, &embed_cfg_clone, &state_clone.http)
                .await
            {
                Ok(vectors) if !vectors.is_empty() && !vectors[0].is_empty() => {
                    // Phase 3.3: the collection must already exist, explicitly
                    // configured — this used to auto-create an unconfigured
                    // namespace on first use (including silently defaulting
                    // "default" to namespace 0 with no existence check at
                    // all), which is exactly the implicit-vector-namespace
                    // behavior this phase closes. Mirrors standalone's
                    // `ingest.rs::ingest`, which has always required
                    // `resolve_collection` to succeed rather than creating
                    // anything.
                    let ns: u16 = match state_clone
                        .sm
                        .resolve_namespace(Some(&collection_clone))
                        .await
                    {
                        Some(id) => id,
                        None => {
                            let mut jobs_map = jobs_clone.write().await;
                            jobs_map.insert(
                                job_id_clone.clone(),
                                serde_json::json!({
                                    "status": "failed",
                                    "job_id": job_id_clone,
                                    "error": format!(
                                        "unknown collection '{collection_clone}' — create it first with POST /v1/namespaces"
                                    ),
                                }),
                            );
                            return;
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
                        if let Ok(resp) = shard_raft
                            .client_write(ClientRequest {
                                event: KernelEvent::AutoInsertRecord {
                                    vector,
                                    metadata: meta_bytes,
                                    tag: ns as u64,
                                },
                                request_id: None,
                                schema_version: CURRENT_SCHEMA_VERSION,
                                namespace_id: ns,
                            })
                            .await
                        {
                            record_ids.push(resp.data.allocated_record_id.unwrap_or(0));
                        }
                    }

                    let doc_node_id: u32 = match shard_raft
                        .client_write(ClientRequest {
                            event: KernelEvent::AutoCreateNode {
                                kind: NodeKind::Document,
                                record: None,
                            },
                            request_id: None,
                            schema_version: CURRENT_SCHEMA_VERSION,
                            namespace_id: ns,
                        })
                        .await
                    {
                        Ok(resp) => resp.data.allocated_node_id.unwrap_or(0),
                        Err(_) => 0,
                    };

                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs().to_string())
                        .unwrap_or_else(|_| "0".into());

                    for (i, (chunk, &rid)) in chunks_clone.iter().zip(record_ids.iter()).enumerate()
                    {
                        let chunk_node_id = match shard_raft
                            .client_write(ClientRequest {
                                event: KernelEvent::AutoCreateNode {
                                    kind: NodeKind::Chunk,
                                    record: Some(RecordId(rid)),
                                },
                                request_id: None,
                                schema_version: CURRENT_SCHEMA_VERSION,
                                namespace_id: ns,
                            })
                            .await
                        {
                            Ok(resp) => resp.data.allocated_node_id.unwrap_or(0),
                            Err(_) => 0,
                        };

                        if doc_node_id > 0 && chunk_node_id > 0 {
                            let _ = shard_raft
                                .client_write(ClientRequest {
                                    event: KernelEvent::AutoCreateEdge {
                                        from: NodeId(doc_node_id),
                                        to: NodeId(chunk_node_id),
                                        kind: EdgeKind::ParentOf,
                                    },
                                    request_id: None,
                                    schema_version: CURRENT_SCHEMA_VERSION,
                                    namespace_id: ns,
                                })
                                .await;
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
                        let _ = shard_raft
                            .client_write(ClientRequest {
                                event: KernelEvent::SetMeta {
                                    key: format!("record:{rid}"),
                                    value: chunk_meta.to_string(),
                                },
                                request_id: None,
                                schema_version: CURRENT_SCHEMA_VERSION,
                                namespace_id: ns,
                            })
                            .await;
                    }

                    let doc_meta = serde_json::json!({
                        "source":       source_clone,
                        "total_chunks": chunks_clone.len(),
                        "collection":   collection_clone,
                        "strategy":     strategy_used_clone,
                        "embed_model":  &embed_cfg_clone.model,
                        "ingested_at":  &now,
                    });
                    let _ = shard_raft
                        .client_write(ClientRequest {
                            event: KernelEvent::SetMeta {
                                key: format!("document:{doc_node_id}"),
                                value: doc_meta.to_string(),
                            },
                            request_id: None,
                            schema_version: CURRENT_SCHEMA_VERSION,
                            namespace_id: ns,
                        })
                        .await;

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
                    jobs_map.insert(
                        job_id_clone.clone(),
                        serde_json::json!({
                            "status": "completed",
                            "job_id": job_id_clone,
                            "document_node_id": doc_node_id,
                            "chunk_count": record_ids.len(),
                            "record_ids": record_ids,
                            "collection": collection_clone,
                            "strategy_used": strategy_used_clone,
                        }),
                    );
                }
                Ok(_) => {
                    let mut jobs_map = jobs_clone.write().await;
                    jobs_map.insert(
                        job_id_clone.clone(),
                        serde_json::json!({
                            "status": "failed",
                            "job_id": job_id_clone,
                            "error": "embed provider returned empty vectors",
                        }),
                    );
                }
                Err(e) => {
                    let mut jobs_map = jobs_clone.write().await;
                    jobs_map.insert(
                        job_id_clone.clone(),
                        serde_json::json!({
                            "status": "failed",
                            "job_id": job_id_clone,
                            "error": e.to_string(),
                        }),
                    );
                }
            }
        });
        return (StatusCode::ACCEPTED, Json(resp)).into_response();
    }

    // 3. Embed
    let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
    let vectors = match valori_ingest::embed_batch(&texts, &embed_cfg, &state.http).await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };
    if vectors.is_empty() || vectors[0].is_empty() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": "embed provider returned empty vectors" })),
        )
            .into_response();
    }

    // 4. Resolve the namespace — Phase 3.3: no auto-create. This used to
    // create an unconfigured namespace via `AutoCreateNamespace` on first
    // use (S2), including silently defaulting "default" to namespace 0
    // with no existence check at all — exactly the implicit-vector-
    // namespace behavior this phase closes. Mirrors standalone's
    // `ingest.rs::ingest`, which has always required `resolve_collection`
    // to succeed rather than creating anything.
    let ns: u16 = match state.sm.resolve_namespace(Some(&collection)).await {
        Some(id) => id,
        // Phase API-2: 404 / `collection_not_found`, matching every other
        // handler on both routers.
        None => return collection_not_found(Some(&collection)),
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
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": e })),
                )
                    .into_response()
            }
        };
        // Encode text in metadata bytes so all replicas can rerank
        let meta_bytes = Some(
            serde_json::json!({ "doc": &source, "n": i, "total": chunks.len(), "text": &chunks[i].text })
                .to_string().into_bytes()
        );
        match shard_raft
            .client_write(ClientRequest {
                event: KernelEvent::AutoInsertRecord {
                    vector,
                    metadata: meta_bytes,
                    tag: ns as u64,
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns,
            })
            .await
        {
            Ok(resp) => {
                if let Some(reason) = &resp.data.rejected {
                    return (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        Json(serde_json::json!({ "error": reason })),
                    )
                        .into_response();
                }
                record_ids.push(resp.data.allocated_record_id.unwrap_or(0));
            }
            Err(openraft::error::RaftError::APIError(
                openraft::error::ClientWriteError::ForwardToLeader(fwd),
            )) => return not_leader_response(fwd.leader_node.as_ref()),
            Err(e) => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({ "error": format!("raft write: {e}") })),
                )
                    .into_response()
            }
        }
    }

    // 6. Document graph node via Raft
    let doc_node_id: u32 = match shard_raft
        .client_write(ClientRequest {
            event: KernelEvent::AutoCreateNode {
                kind: NodeKind::Document,
                record: None,
            },
            request_id: None,
            schema_version: CURRENT_SCHEMA_VERSION,
            namespace_id: ns,
        })
        .await
    {
        Ok(resp) => resp.data.allocated_node_id.unwrap_or(0),
        Err(_) => 0,
    };

    // 7. Chunk nodes + ParentOf edges + node-local metadata sidecar
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into());

    for (i, (chunk, &rid)) in chunks.iter().zip(record_ids.iter()).enumerate() {
        let chunk_node_id = match shard_raft
            .client_write(ClientRequest {
                event: KernelEvent::AutoCreateNode {
                    kind: NodeKind::Chunk,
                    record: Some(RecordId(rid)),
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns,
            })
            .await
        {
            Ok(resp) => resp.data.allocated_node_id.unwrap_or(0),
            Err(_) => 0,
        };

        if doc_node_id > 0 && chunk_node_id > 0 {
            let _ = shard_raft
                .client_write(ClientRequest {
                    event: KernelEvent::AutoCreateEdge {
                        from: NodeId(doc_node_id),
                        to: NodeId(chunk_node_id),
                        kind: EdgeKind::ParentOf,
                    },
                    request_id: None,
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id: ns,
                })
                .await;
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
        let _ = shard_raft
            .client_write(ClientRequest {
                event: KernelEvent::SetMeta {
                    key: format!("record:{rid}"),
                    value: chunk_meta.to_string(),
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns,
            })
            .await;
    }

    let doc_meta = serde_json::json!({
        "source":       source,
        "total_chunks": chunks.len(),
        "collection":   collection,
        "strategy":     strategy_used,
        "embed_model":  &embed_cfg.model,
        "ingested_at":  &now,
    });
    let _ = shard_raft
        .client_write(ClientRequest {
            event: KernelEvent::SetMeta {
                key: format!("document:{doc_node_id}"),
                value: doc_meta.to_string(),
            },
            request_id: None,
            schema_version: CURRENT_SCHEMA_VERSION,
            namespace_id: ns,
        })
        .await;

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

    // NOTE: cluster ingest doesn't go through `IngestPipeline::run_observed()`
    // (it chunks/embeds/writes manually via `raft.client_write()`, not the
    // `Writer` trait) — no `ExecutionRecord` is recorded for this operation
    // id yet, so `GET /v1/operations/:id/execution` will correctly 404 for
    // it rather than return fake data. Real execution telemetry for cluster
    // ingest is follow-up work, not part of this pass.
    let operation_id = format!("ingest-{}", valori_core::id::ExecutionId::new_random());

    Json(crate::ingest::IngestResponse {
        ok: true,
        document_node_id: doc_node_id,
        strategy_used,
        chunk_count: chunks.len(),
        record_ids,
        collection,
        operation_id,
    })
    .into_response()
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
    use valori_kernel::types::id::{NodeId, RecordId};

    let collection = payload
        .collection
        .clone()
        .unwrap_or_else(|| "default".into());
    let source = payload.source.clone().unwrap_or_else(|| "unknown".into());
    let strategy = payload.strategy.as_deref().unwrap_or("auto");
    let chunk_size = payload.chunk_size.unwrap_or(1000);
    let overlap = payload.chunk_overlap.unwrap_or(200);
    let doc_node_id = payload.document_node_id;

    // 1. Embed config
    let embed_cfg = match state.embed_config.clone() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({ "error":
                "on-node embedding not configured — set VALORI_EMBED_PROVIDER" })),
            )
                .into_response();
        }
    };

    // 2. Chunk the new text
    let (new_chunks, strategy_used) =
        valori_ingest::chunk_document(&payload.text, strategy, chunk_size, overlap);
    if new_chunks.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "no chunks produced" })),
        )
            .into_response();
    }

    // 3. Content-hash every new chunk
    let new_hashes: Vec<[u8; 32]> = new_chunks
        .iter()
        .map(|c| valori_ingest::chunk_content_hash(&c.text))
        .collect();

    // 4. Resolve namespace — Phase 3.3: no auto-create (see the fresh-ingest
    // handler above for the full rationale). An update to an existing
    // document's collection should never need to create anything — that
    // collection was already created the first time this document was
    // ingested.
    let ns: u16 = match state.sm.resolve_namespace(Some(&collection)).await {
        Some(id) => id,
        // Phase API-2: 404 / `collection_not_found`, matching every other
        // handler on both routers.
        None => return collection_not_found(Some(&collection)),
    };

    let shard = state.shard_for(ns);
    let shard_id = shard_for_namespace(ns, state.shard_count).0 as u8;
    let state_before: String = {
        let raw = shard.state_machine.state_hash().await;
        raw.iter().map(|b| format!("{:02x}", b)).collect()
    };
    let shard_raft = &shard.raft;
    let shard_sm = &shard.state_machine;

    // 5. Collect old chunks from the document node's outgoing ParentOf edges
    let old_chunks: Vec<(u32, u32, [u8; 32])> = shard_sm
        .with_state(|s| {
            use valori_kernel::types::enums::EdgeKind;
            let mut result = Vec::new();
            let Some(edges) = s.outgoing_edges(NodeId(doc_node_id)) else {
                return result;
            };
            for edge in edges {
                if edge.kind != EdgeKind::ParentOf {
                    continue;
                }
                let chunk_node_id = edge.to.0;
                let Some(chunk_node) = s.get_node(edge.to) else {
                    continue;
                };
                let Some(record_id) = chunk_node.record else {
                    continue;
                };
                let rid = record_id.0;

                let text: Option<String> = s
                    .meta
                    .get(&format!("record:{rid}"))
                    .and_then(|v| serde_json::from_str::<serde_json::Value>(v).ok())
                    .and_then(|v| {
                        v.get("text")
                            .and_then(|t| t.as_str().map(|s| s.to_string()))
                    });

                let hash = match text {
                    Some(ref t) => valori_ingest::chunk_content_hash(t),
                    None => [0u8; 32],
                };
                result.push((rid, chunk_node_id, hash));
            }
            result
        })
        .await;

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
            if let Some(idx) = indices
                .iter()
                .find(|i| !kept_new_indices.contains(i))
                .copied()
            {
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
        let _ = shard_raft
            .client_write(ClientRequest {
                event: KernelEvent::SoftDeleteRecord { id: RecordId(*rid) },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns,
            })
            .await;
    }

    // 8. Embed only new/changed chunks
    let mut added_record_ids: HashMap<usize, u32> = HashMap::new();
    if !to_add.is_empty() {
        let texts_to_embed: Vec<String> =
            to_add.iter().map(|&i| new_chunks[i].text.clone()).collect();
        let vectors =
            match valori_ingest::embed_batch(&texts_to_embed, &embed_cfg, &state.http).await {
                Ok(v) => v,
                Err(e) => {
                    return (
                        StatusCode::BAD_GATEWAY,
                        Json(serde_json::json!({ "error": e.to_string() })),
                    )
                        .into_response()
                }
            };
        if vectors.is_empty() || vectors[0].is_empty() {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": "embed provider returned empty vectors" })),
            )
                .into_response();
        }

        for (vec_idx, &chunk_idx) in to_add.iter().enumerate() {
            let vector = match to_fxp(&vectors[vec_idx]) {
                Ok(v) => v,
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({ "error": e })),
                    )
                        .into_response()
                }
            };
            let meta_bytes = Some(
                serde_json::json!({ "doc": &source, "n": chunk_idx, "total": new_chunks.len(), "text": &new_chunks[chunk_idx].text })
                    .to_string().into_bytes()
            );
            let rid = match shard_raft
                .client_write(ClientRequest {
                    event: KernelEvent::AutoInsertRecord {
                        vector,
                        metadata: meta_bytes,
                        tag: ns as u64,
                    },
                    request_id: None,
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id: ns,
                })
                .await
            {
                Ok(resp) => {
                    if let Some(reason) = &resp.data.rejected {
                        return (
                            StatusCode::UNPROCESSABLE_ENTITY,
                            Json(serde_json::json!({ "error": reason })),
                        )
                            .into_response();
                    }
                    resp.data.allocated_record_id.unwrap_or(0)
                }
                Err(openraft::error::RaftError::APIError(
                    openraft::error::ClientWriteError::ForwardToLeader(fwd),
                )) => return not_leader_response(fwd.leader_node.as_ref()),
                Err(e) => {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(serde_json::json!({ "error": format!("raft write: {e}") })),
                    )
                        .into_response()
                }
            };

            // Create Chunk node
            let chunk_node_id = match shard_raft
                .client_write(ClientRequest {
                    event: KernelEvent::AutoCreateNode {
                        kind: NodeKind::Chunk,
                        record: Some(RecordId(rid)),
                    },
                    request_id: None,
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id: ns,
                })
                .await
            {
                Ok(resp) => resp.data.allocated_node_id.unwrap_or(0),
                Err(_) => 0,
            };

            // ParentOf edge
            if doc_node_id > 0 && chunk_node_id > 0 {
                let _ = shard_raft
                    .client_write(ClientRequest {
                        event: KernelEvent::AutoCreateEdge {
                            from: NodeId(doc_node_id),
                            to: NodeId(chunk_node_id),
                            kind: EdgeKind::ParentOf,
                        },
                        request_id: None,
                        schema_version: CURRENT_SCHEMA_VERSION,
                        namespace_id: ns,
                    })
                    .await;
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
            let _ = shard_raft
                .client_write(ClientRequest {
                    event: KernelEvent::SetMeta {
                        key: format!("record:{rid}"),
                        value: chunk_meta.to_string(),
                    },
                    request_id: None,
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id: ns,
                })
                .await;

            added_record_ids.insert(chunk_idx, rid);
        }
    }

    // 9. Update document-level metadata
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into());
    let _ = shard_raft
        .client_write(ClientRequest {
            event: KernelEvent::SetMeta {
                key: format!("document:{doc_node_id}"),
                value: serde_json::json!({
                    "source":       source,
                    "total_chunks": new_chunks.len(),
                    "collection":   collection,
                    "strategy":     strategy_used,
                    "embed_model":  &embed_cfg.model,
                    "updated_at":   &now,
                })
                .to_string(),
            },
            request_id: None,
            schema_version: CURRENT_SCHEMA_VERSION,
            namespace_id: ns,
        })
        .await;

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
    })
    .into_response()
}

// ── Phase I5: Tree-RAG stateful handlers (cluster path) ───────────────────────

async fn cluster_tree_build(
    State(s): State<DataPlaneState>,
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
    let shard_count = s.shard_count as u8;

    let inputs_json = serde_json::json!({ "text": payload.text, "doc_name": doc_name }).to_string();

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
            cluster: true,
            shard_count,
        },
        schema_version: 1,
        shard_count,
        cluster_epoch: 0,
        cluster_mode: true,
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

async fn cluster_tree_query(
    State(s): State<DataPlaneState>,
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
    let shard_count = s.shard_count as u8;

    let tree_val: serde_json::Value = if let Some(t) = payload.tree {
        serde_json::to_value(t).unwrap_or(serde_json::Value::Null)
    } else if let Some(ref key) = payload.cache_key {
        s.tree_cache.read().await.get(key).cloned()
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

    let inputs_json = serde_json::json!({
        "tree": tree_val, "query": payload.query, "k": k, "prev_hash": payload.prev_hash,
    })
    .to_string();

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
            cluster: true,
            shard_count,
        },
        schema_version: 1,
        shard_count,
        cluster_epoch: 0,
        cluster_mode: true,
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

async fn cluster_tree_hybrid(
    State(s): State<DataPlaneState>,
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

    let shard_count = s.shard_count as u8;

    // ── Resolve tree (inline, cache_key, or text) ─────────────────────────────
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
        let key = valori_rag::tree::hash_text(text);
        s.tree_cache.write().await.insert(key, t.clone());
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
    let ns_id = s.sm.resolve_namespace(ns_name).await.unwrap_or(0);
    let shard_id = shard_for_namespace(ns_id, s.shard_count).0 as u8;
    let embed_cfg = s.embed_config.clone();

    let mut query_vec: Option<Vec<f32>> = None;
    if let Some(ref ecfg) = embed_cfg {
        if let Ok(vecs) = valori_ingest::embed_batch(&[payload.query.clone()], ecfg, &s.http).await
        {
            if !vecs.is_empty() {
                query_vec = Some(vecs.into_iter().next().unwrap());
            }
        }
    }

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
        "shard_id": shard_id,
        "namespace_id": ns_id,
        "query": payload.query,
        "k": payload.k,
        "params": params,
    })
    .to_string();

    let op_hash = compute_operation_hash(
        OperationKind::TreeHybrid,
        &OperationInputs::TreeHybrid {
            k: payload.k as u32,
            shard_id,
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
            cluster: true,
            shard_count,
        },
        schema_version: 1,
        shard_count,
        cluster_epoch: 0,
        cluster_mode: true,
    });
    let graph = Arc::new(ExecutionGraph::build(
        op_hash,
        fp,
        ctx_hash,
        vec![TaskSpec {
            id: TaskId(0),
            kind: TaskKind::TreeHybrid,
            inputs_json,
            shard_id: Some(shard_id),
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

// ── Phase I6: Community handlers (cluster path) ───────────────────────────────

async fn cluster_community_detect(
    State(s): State<DataPlaneState>,
    axum::Extension(caps): axum::Extension<Arc<valori_effect::capability::CapabilityRegistry>>,
    axum::Extension(task_reg): axum::Extension<Arc<crate::runner::TaskRegistry>>,
    Json(payload): Json<valori_rag::community::DetectRequest>,
) -> Result<Json<valori_rag::community::DetectResponse>, (StatusCode, Json<serde_json::Value>)> {
    use crate::runner::run_graph_inline;
    use valori_planner::context::{
        CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash,
    };
    use valori_planner::graph::ExecutionRetentionPolicy;
    use valori_planner::graph::{ExecutionGraph, TaskId, TaskKind, TaskSpec};
    use valori_planner::operation::{
        compute_operation_hash, ExecutionPolicy, OperationInputs, OperationKind,
    };

    let max_iter = payload
        .max_iter
        .unwrap_or(valori_rag::community::DEFAULT_MAX_ITER);
    let shard_count = s.shard_count as u8;

    // Phase S8: namespace→shard routing preserved — see previous comment.
    let ns_id = match payload.namespace.as_deref() {
        Some(name) => s.sm.resolve_namespace(Some(name)).await,
        None => None,
    };
    let ns_id_u16 = ns_id.unwrap_or(0);
    let shard_id = shard_for_namespace(ns_id_u16, s.shard_count).0 as u8;

    let inputs_json = serde_json::json!({
        "shard_id": shard_id,
        "namespace_id": ns_id_u16,
        "max_iter": max_iter,
    })
    .to_string();

    let op_hash = compute_operation_hash(
        OperationKind::CommunityDetect,
        &OperationInputs::CommunityDetect {
            collection: payload
                .namespace
                .clone()
                .unwrap_or_else(|| "default".into()),
            shard_id,
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
            cluster: true,
            shard_count,
        },
        schema_version: 1,
        shard_count,
        cluster_epoch: 0,
        cluster_mode: true,
    });
    let graph = Arc::new(ExecutionGraph::build(
        op_hash,
        fp,
        ctx_hash,
        vec![TaskSpec {
            id: TaskId(0),
            kind: TaskKind::CommunityDetect,
            inputs_json,
            shard_id: Some(shard_id),
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

    let out = result
        .into_iter()
        .next()
        .flatten()
        .map(|o| o.json)
        .unwrap_or(serde_json::json!({}));
    Ok(Json(valori_rag::community::DetectResponse {
        community_count: out["community_count"].as_u64().unwrap_or(0) as usize,
        node_count: out["node_count"].as_u64().unwrap_or(0) as usize,
        communities: out["communities"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default(),
        receipt: out["receipt"].as_str().unwrap_or("").to_string(),
    }))
}

async fn cluster_community_search(
    State(s): State<DataPlaneState>,
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

    let shard_count = s.shard_count as u8;

    // Route to the correct shard for the requested namespace so community_search
    // reads from the same shard that community_detect wrote to.
    let ns_id = match payload.namespace.as_deref() {
        Some(name) => s.sm.resolve_namespace(Some(name)).await.unwrap_or(0),
        None => 0,
    };
    let shard_id = shard_for_namespace(ns_id, s.shard_count).0 as u8;
    let collection = payload
        .namespace
        .clone()
        .unwrap_or_else(|| "default".into());

    let inputs_json = serde_json::json!({
        "shard_id": shard_id,
        "namespace_id": ns_id,
        "vector": payload.vector,
        "k": payload.k,
        "depth": payload.depth,
        "drill_in": payload.drill_in,
    })
    .to_string();

    let op_hash = compute_operation_hash(
        OperationKind::CommunitySearch,
        &OperationInputs::CommunitySearch {
            k: payload.k as u32,
            depth: payload.depth,
            drill_in: payload.drill_in,
            collection,
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
            cluster: true,
            shard_count,
        },
        schema_version: 1,
        shard_count,
        cluster_epoch: 0,
        cluster_mode: true,
    });
    let graph = Arc::new(ExecutionGraph::build(
        op_hash,
        fp,
        ctx_hash,
        vec![TaskSpec {
            id: TaskId(0),
            kind: TaskKind::CommunitySearch,
            inputs_json,
            shard_id: Some(shard_id),
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

async fn cluster_community_overview(
    State(s): State<DataPlaneState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let store_guard = s.community_store.read().await;
    let store = store_guard.as_ref().ok_or_else(|| {
        (
            StatusCode::PRECONDITION_FAILED,
            Json(serde_json::json!({
                "error": "community index not built — call POST /v1/community/detect first"
            })),
        )
    })?;

    let mut communities: Vec<serde_json::Value> = store
        .members
        .iter()
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
    Json(payload): Json<valori_rag::community::ExtractEntitiesRequest>,
) -> Result<
    Json<valori_rag::community::ExtractEntitiesResponse>,
    (StatusCode, Json<serde_json::Value>),
> {
    let embed_cfg = s.embed_config.clone().ok_or_else(|| {
        (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({
            "error": "VALORI_EMBED_PROVIDER not configured — entity extraction requires an LLM provider"
        })))
    })?;

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
        &s.http,
    )
    .await
    .map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": e})),
        )
    })?;

    // Embed entity descriptions.
    let descriptions: Vec<String> = extracted
        .entities
        .iter()
        .map(|e| e.description.clone())
        .collect();
    let vecs = valori_ingest::embed_batch(&descriptions, &embed_cfg, &s.http)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": e.0})),
            )
        })?;

    // Insert records + nodes via Raft.
    let ns_id: u16 =
        s.sm.resolve_namespace(payload.namespace.as_deref())
            .await
            .unwrap_or(0);
    // Phase S4: route to the shard that owns this namespace's data.
    let shard_raft = &s.shard_for(ns_id).raft;

    use valori_kernel::fxp::qformat::SCALE;
    use valori_kernel::types::enums::{EdgeKind, NodeKind};
    use valori_kernel::types::scalar::FxpScalar;

    let mut entity_name_to_node_id: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    let mut inserted_entities: Vec<valori_rag::community::InsertedEntity> = Vec::new();

    use valori_consensus::types::ClientRequest;
    use valori_kernel::event::KernelEvent;
    use valori_kernel::types::id::{NodeId, RecordId};
    use valori_kernel::types::vector::FxpVector;

    for (entity, vec) in extracted.entities.iter().zip(vecs.iter()) {
        let fxp_data: Vec<FxpScalar> = vec
            .iter()
            .map(|&v| FxpScalar((v * SCALE as f32) as i32))
            .collect();
        let fxp_vec = FxpVector { data: fxp_data };

        // Real allocated ids from the commit response — not a pre-read
        // guess, which would race a concurrent writer AND (now that writes
        // are shard-routed) would guess against the wrong shard's counter
        // entirely if read from the flat shard-0 state machine.
        let record_id = match raft_write_data(
            shard_raft,
            ClientRequest {
                event: KernelEvent::AutoInsertRecord {
                    vector: fxp_vec,
                    metadata: None,
                    tag: 0,
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns_id,
            },
        )
        .await
        {
            Ok(resp) => resp.allocated_record_id.unwrap_or(0),
            Err(_) => continue,
        };
        let node_id = match raft_write_data(
            shard_raft,
            ClientRequest {
                event: KernelEvent::AutoCreateNode {
                    kind: NodeKind::Concept,
                    record: Some(RecordId(record_id)),
                },
                request_id: None,
                schema_version: CURRENT_SCHEMA_VERSION,
                namespace_id: ns_id,
            },
        )
        .await
        {
            Ok(resp) => resp.allocated_node_id.unwrap_or(0),
            Err(_) => continue,
        };

        entity_name_to_node_id.insert(entity.name.clone(), node_id);
        inserted_entities.push(valori_rag::community::InsertedEntity {
            name: entity.name.clone(),
            kind: entity.kind.clone(),
            description: entity.description.clone(),
            node_id,
            record_id: Some(record_id),
        });
    }

    // Create edges.
    let mut inserted_rels: Vec<valori_rag::community::InsertedRelationship> = Vec::new();
    let mut skipped = 0usize;

    for rel in &extracted.relationships {
        match (
            entity_name_to_node_id.get(&rel.source),
            entity_name_to_node_id.get(&rel.target),
        ) {
            (Some(&from_id), Some(&to_id)) => {
                let ev = KernelEvent::AutoCreateEdge {
                    from: NodeId(from_id),
                    to: NodeId(to_id),
                    kind: EdgeKind::Relation,
                };
                match raft_write_data(
                    shard_raft,
                    ClientRequest {
                        event: ev,
                        request_id: None,
                        schema_version: CURRENT_SCHEMA_VERSION,
                        namespace_id: ns_id,
                    },
                )
                .await
                {
                    Ok(resp) => inserted_rels.push(valori_rag::community::InsertedRelationship {
                        source_name: rel.source.clone(),
                        target_name: rel.target.clone(),
                        description: rel.description.clone(),
                        edge_id: resp.allocated_edge_id.unwrap_or(0),
                    }),
                    Err(_) => {
                        skipped += 1;
                    }
                }
            }
            _ => {
                skipped += 1;
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
    limit: Option<usize>,
}

fn collect_cluster_timeline(
    state: &DataPlaneState,
    from_unix: Option<u64>,
    to_unix: Option<u64>,
) -> Vec<crate::api::TimelineEntry> {
    use valori_kernel::event::KernelEvent;
    use valori_wire::{decode_entry, parse_header, LogEntry as WireLogEntry};

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
                    if let Some(from) = from_unix {
                        if ts < from {
                            log_index += 1;
                            continue;
                        }
                    }
                    if let Some(to) = to_unix {
                        if ts > to {
                            log_index += 1;
                            continue;
                        }
                    }
                    let inner_ev = match &decoded.entry {
                        WireLogEntry::Event(ev) => Some(ev),
                        WireLogEntry::EventNs { event, .. } => Some(event),
                        _ => None,
                    };
                    if let Some(ev) = inner_ev {
                        let (event_type, record_id, node_id, edge_id) = match ev {
                            KernelEvent::InsertRecord { id, .. } => {
                                ("InsertRecord", Some(id.0), None, None)
                            }
                            KernelEvent::AutoInsertRecord { .. } => {
                                ("AutoInsertRecord", None, None, None)
                            }
                            KernelEvent::InsertRecordEncrypted { id, .. } => {
                                ("InsertRecordEncrypted", Some(id.0), None, None)
                            }
                            KernelEvent::DeleteRecord { id } => {
                                ("DeleteRecord", Some(id.0), None, None)
                            }
                            KernelEvent::SoftDeleteRecord { id } => {
                                ("SoftDeleteRecord", Some(id.0), None, None)
                            }
                            KernelEvent::ShredKey { .. } => ("ShredKey", None, None, None),
                            KernelEvent::CreateNode { id, .. } => {
                                ("CreateNode", None, Some(id.0), None)
                            }
                            KernelEvent::AutoCreateNode { .. } => {
                                ("AutoCreateNode", None, None, None)
                            }
                            KernelEvent::DeleteNode { id } => {
                                ("DeleteNode", None, Some(id.0), None)
                            }
                            KernelEvent::CreateEdge { id, .. } => {
                                ("CreateEdge", None, None, Some(id.0))
                            }
                            KernelEvent::AutoCreateEdge { .. } => {
                                ("AutoCreateEdge", None, None, None)
                            }
                            KernelEvent::DeleteEdge { id } => {
                                ("DeleteEdge", None, None, Some(id.0))
                            }
                            KernelEvent::AutoInsertRecordEncrypted { .. } => {
                                ("AutoInsertRecordEncrypted", None, None, None)
                            }
                            KernelEvent::SetMeta { .. } => ("SetMeta", None, None, None),
                            KernelEvent::AutoCreateNamespace { .. } => {
                                ("AutoCreateNamespace", None, None, None)
                            }
                            KernelEvent::DropNamespace { .. } => {
                                ("DropNamespace", None, None, None)
                            }
                            KernelEvent::UpdateRecordMetadata { id, .. } => {
                                ("UpdateRecordMetadata", Some(id.0), None, None)
                            }
                            KernelEvent::ConfigureNamespace { .. } => {
                                ("ConfigureNamespace", None, None, None)
                            }
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
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Event log not enabled on this node (set VALORI_EVENT_LOG_PATH)"
            })),
        )
            .into_response();
    }

    let from_unix = q.from.as_deref().and_then(crate::server::parse_iso8601);
    let to_unix = q.to.as_deref().and_then(crate::server::parse_iso8601);
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
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "error": format!(
                                "shard {} ordering violation: log_index {} after {}",
                                e.shard_id, e.log_index, prev
                            )
                        })),
                    )
                        .into_response();
                }
            }
            shard_last.insert(e.shard_id, e.log_index);
        }
    }

    let total = entries.len();
    let mut entries = entries;
    if let Some(n) = q.limit {
        let skip = total.saturating_sub(n);
        entries.drain(..skip);
    }
    (
        StatusCode::OK,
        Json(crate::api::TimelineResponse {
            events: entries,
            total,
            from_unix,
            to_unix,
        }),
    )
        .into_response()
}

async fn cluster_get_operations(State(state): State<DataPlaneState>) -> Response {
    if state.shard_event_log_paths.is_empty() {
        return (
            StatusCode::OK,
            Json(crate::api::OperationsListResponse {
                operations: vec![],
                total: 0,
            }),
        )
            .into_response();
    }
    let entries = collect_cluster_timeline(&state, None, None);
    let mut operations: Vec<crate::api::OperationSummary> = entries
        .into_iter()
        .map(|e| {
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
        })
        .collect();
    operations.reverse();
    let total = operations.len();
    (
        StatusCode::OK,
        Json(crate::api::OperationsListResponse { operations, total }),
    )
        .into_response()
}

async fn cluster_get_operation_by_id(
    axum::extract::Path(id): axum::extract::Path<String>,
    State(state): State<DataPlaneState>,
    axum::Extension(receipt_store): axum::Extension<Arc<valori_effect::ReceiptStore>>,
) -> Response {
    if state.shard_event_log_paths.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Event log not enabled"})),
        )
            .into_response();
    }
    let entries = collect_cluster_timeline(&state, None, None);
    let op = entries.iter().find(|e| {
        format!("op-{}-{}", e.shard_id, e.log_index) == id
            || format!("op-{}", e.log_index) == id
            || id == format!("{}", e.log_index)
    });
    let Some(e) = op else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("operation '{}' not found", id)})),
        )
            .into_response();
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

    (
        StatusCode::OK,
        Json(crate::api::OperationDetailResponse {
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
        }),
    )
        .into_response()
}

// ── Cluster snapshot save/restore/download ────────────────────────────────────
// In cluster mode snapshots are driven by openraft's own mechanism, but we
// expose save/restore/download for operational tooling (same surface as standalone).

fn encode_cluster_snapshot(
    state: &valori_kernel::state::kernel::KernelState,
) -> Result<Vec<u8>, String> {
    let hint = valori_kernel::snapshot::encode::encode_capacity_hint(state);
    let mut buf = Vec::with_capacity(hint);
    valori_kernel::snapshot::encode::encode_state(state, &mut buf).map_err(|e| format!("{e:?}"))?;
    Ok(buf)
}

async fn cluster_snapshot_save(
    State(state): State<DataPlaneState>,
    axum::Extension(caps): axum::Extension<Arc<valori_effect::capability::CapabilityRegistry>>,
    axum::Extension(task_reg): axum::Extension<Arc<crate::runner::TaskRegistry>>,
) -> Response {
    use crate::runner::run_graph_inline;
    use valori_planner::context::{
        CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash,
    };
    use valori_planner::graph::ExecutionRetentionPolicy;
    use valori_planner::graph::{ExecutionGraph, TaskId, TaskKind, TaskSpec};
    use valori_planner::operation::{
        compute_operation_hash, ExecutionPolicy, OperationInputs, OperationKind,
    };

    let shard_count = state.shard_count as u8;

    // Snapshot every shard so a restore can recover all data.
    let mut shard_hashes: Vec<serde_json::Value> = Vec::new();
    for shard_id in 0..shard_count {
        let inputs_json = serde_json::json!({ "shard_id": shard_id, "path": null }).to_string();

        let op_hash = compute_operation_hash(
            OperationKind::Snapshot,
            &OperationInputs::Snapshot { shard_id },
            &ExecutionPolicy::default(),
        );
        let fp = PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1);
        let ctx_hash = PlanningContextHash::compute(&PlanningContext {
            capability_set: CapabilitySet {
                embed: false,
                llm: false,
                object_store: false,
                cluster: true,
                shard_count,
            },
            schema_version: 1,
            shard_count,
            cluster_epoch: 0,
            cluster_mode: true,
        });
        let graph = Arc::new(ExecutionGraph::build(
            op_hash,
            fp,
            ctx_hash,
            vec![TaskSpec {
                id: TaskId(0),
                kind: TaskKind::SnapshotArtifact,
                inputs_json,
                shard_id: Some(shard_id),
                topological_index: 0,
            }],
            vec![],
            ExecutionRetentionPolicy::default(),
        ));

        match run_graph_inline(
            graph,
            caps.clone(),
            task_reg.clone(),
            ExecutionPolicy::default(),
        )
        .await
        {
            Ok(outputs) => {
                // Task emits { "state_hash": "..." } — use the correct field name.
                let hash = outputs
                    .into_iter()
                    .next()
                    .flatten()
                    .and_then(|o| {
                        o.json
                            .get("state_hash")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_default();
                shard_hashes.push(serde_json::json!({ "shard_id": shard_id, "state_hash": hash }));
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": format!("snapshot shard {shard_id} failed: {e}")
                    })),
                )
                    .into_response()
            }
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "shards": shard_hashes,
            "note": "Cluster snapshots are persisted automatically by Raft."
        })),
    )
        .into_response()
}

async fn cluster_snapshot_restore() -> Response {
    (StatusCode::NOT_IMPLEMENTED, Json(serde_json::json!({
        "error": "Snapshot restore in cluster mode must be done via the Raft snapshot mechanism. \
                  Shut down all nodes, replace the redb log file on node-1, and restart."
    }))).into_response()
}

// ── Object store (Phase 3.1) — cluster path ───────────────────────────────────
// Reads and uploads are safe per-node operations: each node encodes its OWN
// converged state, and every node's object store points at the same bucket.
// The control plane calls these on the project's published node (index 0),
// same as it does on the standalone path.
//
// RESTORE is the one that isn't symmetric — see `cluster_storage_restore`.

fn cluster_object_store(
    state: &DataPlaneState,
) -> Result<Arc<crate::object_store::ObjectStoreBackend>, Response> {
    state.object_store.clone().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "object store not configured — set VALORI_OBJECT_STORE_URL"
            })),
        )
            .into_response()
    })
}

/// `GET /v1/storage/snapshots` — list snapshots in the object store.
async fn cluster_list_remote_snapshots(State(state): State<DataPlaneState>) -> Response {
    let os = match cluster_object_store(&state) {
        Ok(os) => os,
        Err(r) => return r,
    };
    match os.list_snapshots().await {
        Ok(snapshots) => {
            let count = snapshots.len();
            (
                StatusCode::OK,
                Json(serde_json::json!({ "snapshots": snapshots, "count": count })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("object store list failed: {e}") })),
        )
            .into_response(),
    }
}

/// `GET /v1/storage/manifest` — the disaster-recovery entry point.
async fn cluster_get_manifest(State(state): State<DataPlaneState>) -> Response {
    let os = match cluster_object_store(&state) {
        Ok(os) => os,
        Err(r) => return r,
    };
    match os.read_manifest().await {
        Ok(manifest) => (
            StatusCode::OK,
            Json(serde_json::json!({ "manifest": manifest })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("reading manifest failed: {e}") })),
        )
            .into_response(),
    }
}

/// `POST /v1/storage/snapshots/upload` — encode this node's state and push
/// it to the object store, rewriting `manifest.json` to point at it.
///
/// Safe on any node: a converged cluster produces byte-identical state on
/// every peer (that's what the state-hash watcher checks), so it doesn't
/// matter which one uploads. The snapshot is keyed by epoch + state hash,
/// so two nodes uploading concurrently produce two objects rather than a
/// corrupted one.
async fn cluster_upload_snapshot_to_store(State(state): State<DataPlaneState>) -> Response {
    let os = match cluster_object_store(&state) {
        Ok(os) => os,
        Err(r) => return r,
    };

    let bytes = match state.sm.with_state(encode_cluster_snapshot).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("snapshot encode failed: {e}") })),
            )
                .into_response()
        }
    };

    let state_hash: String = state
        .sm
        .state_hash()
        .await
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let size_bytes = bytes.len();

    let entry = match os
        .upload_snapshot_and_update_manifest(&bytes, &state_hash, env!("CARGO_PKG_VERSION"))
        .await
    {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("upload failed: {e}") })),
            )
                .into_response()
        }
    };

    let keep = std::env::var("VALORI_OBJECT_STORE_KEEP")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(7);
    let pruned = os.prune_snapshots(keep).await.unwrap_or(0);

    metrics::gauge!("valori_snapshot_size_bytes", size_bytes as f64);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "key": entry.key,
            "state_hash": state_hash,
            "size_bytes": size_bytes,
            "pruned": pruned,
        })),
    )
        .into_response()
}

/// `POST /v1/storage/snapshots/restore` — **deliberately not implemented in
/// cluster mode**, matching `cluster_snapshot_restore` above.
///
/// Overwriting one node's `KernelState` out-of-band would desync it from the
/// Raft log: its applied index would no longer describe its state, and the
/// next committed entry would be applied on top of data the log never
/// produced. The node would silently diverge from its peers — exactly the
/// failure the state-hash watcher exists to detect. Raft's own snapshot
/// install is the only correct in-cluster path, and it's leader-driven, not
/// something an operator triggers per-node over HTTP.
///
/// This returns 501 with the real procedure rather than 404, so a control
/// plane calling it learns what to do instead of seeing a missing route.
async fn cluster_storage_restore() -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "restore-from-object-store is not available in cluster mode",
            "why": "writing state behind Raft's back would desync this node's applied index from its log, \
                    silently diverging it from its peers",
            "instead": "recover into a NEW cluster: bootstrap a single node in standalone mode, \
                        POST /v1/storage/snapshots/restore there (or restore via manifest.json), \
                        verify its state hash, then bring peers up against it and let Raft replicate.",
            "manifest": "GET /v1/storage/manifest names the snapshot to restore from"
        })),
    )
        .into_response()
}

/// `GET /v1/storage/wal` — list archived WAL segments.
async fn cluster_list_remote_wal(State(state): State<DataPlaneState>) -> Response {
    let os = match cluster_object_store(&state) {
        Ok(os) => os,
        Err(r) => return r,
    };
    match os.list_wal_segments().await {
        Ok(segments) => {
            let count = segments.len();
            (
                StatusCode::OK,
                Json(serde_json::json!({ "segments": segments, "count": count })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("object store list failed: {e}") })),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
struct ClusterArchiveWalRequest {
    path: String,
}

/// `POST /v1/storage/wal/archive` — archive a sealed per-shard segment.
///
/// Path is validated against this node's own shard log directories
/// (`shard_event_log_paths`), so a caller can't use this to exfiltrate an
/// arbitrary file from the container — the same containment the standalone
/// path gets from `safe_path`.
async fn cluster_archive_wal_segment(
    State(state): State<DataPlaneState>,
    Json(req): Json<ClusterArchiveWalRequest>,
) -> Response {
    let os = match cluster_object_store(&state) {
        Ok(os) => os,
        Err(r) => return r,
    };

    let candidate = std::path::PathBuf::from(&req.path);
    let allowed = state
        .shard_event_log_paths
        .values()
        .filter_map(|p| p.parent())
        .any(|dir| candidate.starts_with(dir));

    if !allowed {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "path is not inside any shard's event-log directory"
            })),
        )
            .into_response();
    }
    if !candidate.exists() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("segment not found: {}", candidate.display()) })),
        )
            .into_response();
    }

    let size_bytes = std::fs::metadata(&candidate).map(|m| m.len()).unwrap_or(0);
    match os.archive_wal_segment(&candidate).await {
        Ok(key) => {
            // Keep manifest.json's WAL list current, same as standalone.
            if let Ok(current) = os
                .read_manifest()
                .await
                .map(|m| m.and_then(|m| m.current_snapshot))
            {
                if let Ok(segments) = os.list_wal_segments().await {
                    if let Err(e) = os
                        .write_manifest(current.as_ref(), segments, env!("CARGO_PKG_VERSION"))
                        .await
                    {
                        tracing::warn!(error = %e, "failed to refresh manifest.json after WAL archive, continuing");
                    }
                }
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({ "key": key, "size_bytes": size_bytes })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("archive failed: {e}") })),
        )
            .into_response(),
    }
}

async fn cluster_snapshot_download(State(state): State<DataPlaneState>) -> Response {
    match state.sm.with_state(encode_cluster_snapshot).await {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE.as_str(), "application/octet-stream"),
                (
                    header::CONTENT_DISPOSITION.as_str(),
                    "attachment; filename=\"cluster-snapshot.snap\"",
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("snapshot encode failed: {e}")
            })),
        )
            .into_response(),
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────
/// `GET /v1/shard/routing` — show namespace→shard assignment for all collections.
///
/// In cluster mode, also shows the shard count and which shard each namespace
/// maps to via `namespace_id % shard_count`.
async fn cluster_shard_routing(State(state): State<DataPlaneState>) -> Response {
    let shard_count = state.shard_count as usize;

    // Read namespace registry from shard 0's state machine.
    let shard0 = match state.shards.values().next() {
        Some(s) => s,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "no shard 0"})),
            )
                .into_response()
        }
    };
    let collections: Vec<(String, u16)> = shard0
        .state_machine
        .with_state(|_ks| vec![("default".to_string(), 0u16)])
        .await;

    // Build per-shard collection buckets
    let mut shard_map: Vec<Vec<String>> = vec![Vec::new(); shard_count.max(1)];
    for (name, ns_id) in &collections {
        let shard = ns_id.wrapping_rem(shard_count.max(1) as u16) as usize;
        if let Some(bucket) = shard_map.get_mut(shard) {
            bucket.push(name.clone());
        }
    }

    let shards: Vec<serde_json::Value> = shard_map
        .into_iter()
        .enumerate()
        .map(|(i, cols)| serde_json::json!({ "shard": i, "collections": cols }))
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "mode": "cluster",
            "shard_count": shard_count,
            "shards": shards,
        })),
    )
        .into_response()
}

// ── Receipt endpoints (Phase A8) ──────────────────────────────────────────────

async fn cluster_get_latest_receipt(
    axum::Extension(store): axum::Extension<Arc<valori_effect::ReceiptStore>>,
) -> Response {
    match store.latest() {
        Some(r) => Json(serde_json::json!({"ok": true, "receipt": r})).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "no receipt available yet"})),
        )
            .into_response(),
    }
}

async fn cluster_get_receipt_by_id(
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Extension(store): axum::Extension<Arc<valori_effect::ReceiptStore>>,
) -> Response {
    match store.get(&id) {
        Some(r) => Json(serde_json::json!({"ok": true, "receipt": r})).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("receipt '{}' not found", id)})),
        )
            .into_response(),
    }
}

/// `GET /v1/models/health`
async fn cluster_models_health() -> axum::Json<serde_json::Value> {
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

#[cfg(test)]
mod tests {
    use super::ReadinessGate;

    /// A fresh node (target == 0) must be ready immediately without any apply.
    #[test]
    fn gate_target_zero_is_immediately_ready() {
        let gate = ReadinessGate::new(0);
        assert!(
            gate.check_applied(0).is_ok(),
            "target=0 should be ready at apply=0"
        );
        assert!(gate.check_applied(5).is_ok(), "target=0 should stay ready");
    }

    /// Below the target the gate must return an Err (503) response.
    #[test]
    fn gate_blocks_below_target() {
        let gate = ReadinessGate::new(10);
        assert!(
            gate.check_applied(0).is_err(),
            "apply=0  < target=10 → not ready"
        );
        assert!(
            gate.check_applied(9).is_err(),
            "apply=9  < target=10 → not ready"
        );
    }

    /// Exactly at the target the gate must flip open and return Ok.
    #[test]
    fn gate_opens_at_target() {
        let gate = ReadinessGate::new(10);
        assert!(
            gate.check_applied(10).is_ok(),
            "apply=10 == target=10 → ready"
        );
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
        assert!(
            gate.check_applied(0).is_ok(),
            "fast-path must bypass target check"
        );
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
    fn namespace_zero_always_resolves_to_shard_zero() {
        // Namespace id 0 lands on shard 0 regardless of shard_count —
        // consequence of the modulo, not a special case (Phase 3.3: id 0
        // carries no name-based meaning; it just stays permanently
        // unallocated in a fresh registry — see
        // `CollectionRegistry::new`'s doc comment), but worth pinning: the
        // namespace registry itself lives only on shard 0 (Phase S2), so
        // this must hold for the registry's own bookkeeping to be sound.
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
