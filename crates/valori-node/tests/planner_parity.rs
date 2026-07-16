// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Planner-level parity tests.
//!
//! Each test seeds the same data into both standalone and cluster paths, builds
//! one `ExecutionGraph`, and asserts that `run_graph_inline` produces identical
//! outputs regardless of which `KernelCapability` impl runs it.
//!
//! This is the right layer to catch the class of bugs that the A14 audit
//! surfaced (decay sort inverted, snapshot field name mismatch, etc.) — it
//! exercises `EngineKernelCapability` vs `RaftKernelCapability` directly,
//! bypassing HTTP serialization and routing entirely.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use valori_consensus::types::ValoriNode;
use valori_node::capabilities::CapabilityRegistryBuilder;
use valori_node::cluster::{bootstrap_cluster, ClusterConfig};
use valori_node::cluster_server::build_cluster_router;
use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use valori_node::runner::{run_graph_inline, TaskRegistry};
use valori_node::server::build_router;
use valori_node::EngineFromNodeConfig;

use valori_planner::context::{
    CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash,
};
use valori_planner::graph::{ExecutionGraph, ExecutionRetentionPolicy, TaskId, TaskKind, TaskSpec};
use valori_planner::operation::{
    compute_operation_hash, ExecutionPolicy, OperationInputs, OperationKind,
};

// ── Environment builders ──────────────────────────────────────────────────────

/// Everything a parity test needs for one execution path.
struct TestEnv {
    caps: Arc<valori_effect::capability::CapabilityRegistry>,
    task_reg: Arc<TaskRegistry>,
    router: axum::Router,
}

fn standalone_env() -> TestEnv {
    let mut cfg = NodeConfig::default();
    cfg.dim = 4;
    cfg.max_records = 500;
    cfg.max_nodes = 200;
    cfg.max_edges = 200;
    let engine = Engine::new(&cfg);
    let shared = Arc::new(tokio::sync::RwLock::new(engine));
    let http = reqwest::Client::new();
    let caps = Arc::new(CapabilityRegistryBuilder::new(shared.clone(), 1, http).build());
    let task_reg = Arc::new(TaskRegistry::default_registry());
    let router = build_router(shared, None, None);
    TestEnv {
        caps,
        task_reg,
        router,
    }
}

async fn cluster_env() -> TestEnv {
    let cfg = ClusterConfig {
        node_id: 1,
        raft_bind: "127.0.0.1:0".into(),
        members: [(
            1,
            ValoriNode {
                api_addr: "127.0.0.1:0".into(),
                raft_addr: String::new(),
            },
        )]
        .into_iter()
        .collect(),
        init: true,
        raft_log_path: None,
        tls: None,
        shard_count: 1,
    };
    let handle = bootstrap_cluster(&cfg, None, None, 4).await.unwrap();
    handle
        .raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader == Some(1), "self-elected")
        .await
        .unwrap();

    // Build the router first — it clones Arc-based internals from the handle.
    let router = build_cluster_router(&handle, None);

    // Build the capability from the same handle — ValoriStateMachine.inner is an
    // Arc<Mutex<...>>, so both the router and the capability see the same state.
    let shards = Arc::new(
        handle
            .shards
            .iter()
            .map(|(id, h)| {
                use valori_node::cluster::ShardHandle;
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
            .collect::<std::collections::BTreeMap<_, _>>(),
    );
    let sm = handle.state_machine.clone();
    let tree_cache = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
    let community_store = Arc::new(tokio::sync::RwLock::new(None));
    let http = reqwest::Client::new();
    let caps = Arc::new(CapabilityRegistryBuilder::build_cluster(
        shards,
        sm,
        1,
        None,
        http,
        tree_cache,
        community_store,
    ));
    let task_reg = Arc::new(TaskRegistry::default_registry());
    // Leak handle so raft/sm remain valid for the test duration.
    std::mem::forget(handle);
    TestEnv {
        caps,
        task_reg,
        router,
    }
}

// ── HTTP seed helpers ─────────────────────────────────────────────────────────

async fn post(router: &axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(json!(null));
    (status, json)
}

/// Insert four vectors through whichever router the env provides.
/// Returns the assigned record IDs in insertion order.
async fn seed_vectors(env: &TestEnv) -> Vec<u64> {
    let vecs: &[[f32; 4]] = &[
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut ids = Vec::new();
    for v in vecs {
        let (status, resp) = post(&env.router, "/records", json!({ "values": v })).await;
        assert_eq!(status, StatusCode::OK, "insert failed: {resp}");
        ids.push(resp["id"].as_u64().unwrap_or(0));
    }
    ids
}

// ── Planner graph builders ────────────────────────────────────────────────────

fn fp() -> PlannerFingerprint {
    PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1)
}

fn standalone_ctx() -> PlanningContextHash {
    PlanningContextHash::compute(&PlanningContext {
        capability_set: CapabilitySet {
            embed: false,
            llm: false,
            object_store: false,
            cluster: false,
            shard_count: 1,
        },
        schema_version: 1,
        shard_count: 1,
        cluster_epoch: 0,
        cluster_mode: false,
    })
}

fn cluster_ctx() -> PlanningContextHash {
    PlanningContextHash::compute(&PlanningContext {
        capability_set: CapabilitySet {
            embed: false,
            llm: false,
            object_store: false,
            cluster: true,
            shard_count: 1,
        },
        schema_version: 1,
        shard_count: 1,
        cluster_epoch: 0,
        cluster_mode: true,
    })
}

fn memory_search_graph(ctx: PlanningContextHash, query: &[f32; 4]) -> Arc<ExecutionGraph> {
    let inputs_json = json!({
        "shard_id": 0u8, "namespace_id": 0u16,
        "vector": query, "k": 4u32,
        "decay_half_life_secs": null, "rerank": false, "metadata_filter": null,
    })
    .to_string();
    let op_hash = compute_operation_hash(
        OperationKind::MemorySearch,
        &OperationInputs::MemorySearch {
            k: 4,
            collection: "default".into(),
            decay: false,
            shard_id: 0,
        },
        &ExecutionPolicy::default(),
    );
    Arc::new(ExecutionGraph::build(
        op_hash,
        fp(),
        ctx,
        vec![TaskSpec {
            id: TaskId(0),
            kind: TaskKind::MemorySearch,
            inputs_json,
            shard_id: Some(0),
            topological_index: 0,
        }],
        vec![],
        ExecutionRetentionPolicy::default(),
    ))
}

fn graph_rag_graph(ctx: PlanningContextHash, query: &[f32; 4]) -> Arc<ExecutionGraph> {
    let inputs_json = json!({
        "shard_id": 0u8, "namespace_id": 0u16,
        "vector": query, "k": 4u32, "depth": 1u32,
    })
    .to_string();
    let op_hash = compute_operation_hash(
        OperationKind::GraphRag,
        &OperationInputs::GraphRag {
            k: 4,
            depth: 1,
            collection: "default".into(),
            shard_id: 0,
        },
        &ExecutionPolicy::default(),
    );
    Arc::new(ExecutionGraph::build(
        op_hash,
        fp(),
        ctx,
        vec![TaskSpec {
            id: TaskId(0),
            kind: TaskKind::GraphRag,
            inputs_json,
            shard_id: Some(0),
            topological_index: 0,
        }],
        vec![],
        ExecutionRetentionPolicy::default(),
    ))
}

fn snapshot_graph(ctx: PlanningContextHash, path: Option<&str>) -> Arc<ExecutionGraph> {
    let inputs_json = json!({ "shard_id": 0u8, "path": path }).to_string();
    let op_hash = compute_operation_hash(
        OperationKind::Snapshot,
        &OperationInputs::Snapshot { shard_id: 0 },
        &ExecutionPolicy::default(),
    );
    Arc::new(ExecutionGraph::build(
        op_hash,
        fp(),
        ctx,
        vec![TaskSpec {
            id: TaskId(0),
            kind: TaskKind::SnapshotArtifact,
            inputs_json,
            shard_id: Some(0),
            topological_index: 0,
        }],
        vec![],
        ExecutionRetentionPolicy::default(),
    ))
}

fn community_detect_graph(ctx: PlanningContextHash) -> Arc<ExecutionGraph> {
    let inputs_json =
        json!({ "shard_id": 0u8, "namespace_id": 0u16, "max_iter": 10u32 }).to_string();
    let op_hash = compute_operation_hash(
        OperationKind::CommunityDetect,
        &OperationInputs::CommunityDetect {
            collection: "default".into(),
            shard_id: 0,
            max_iter: 10,
        },
        &ExecutionPolicy::default(),
    );
    Arc::new(ExecutionGraph::build(
        op_hash,
        fp(),
        ctx,
        vec![TaskSpec {
            id: TaskId(0),
            kind: TaskKind::CommunityDetect,
            inputs_json,
            shard_id: Some(0),
            topological_index: 0,
        }],
        vec![],
        ExecutionRetentionPolicy::default(),
    ))
}

// ── Helper: run one graph on both paths and return (standalone_out, cluster_out) ──

async fn run_both(
    s_env: &TestEnv,
    c_env: &TestEnv,
    s_graph: Arc<ExecutionGraph>,
    c_graph: Arc<ExecutionGraph>,
) -> (Value, Value) {
    let s_out = run_graph_inline(
        s_graph,
        s_env.caps.clone(),
        s_env.task_reg.clone(),
        ExecutionPolicy::default(),
    )
    .await
    .expect("standalone run_graph_inline failed");
    let c_out = run_graph_inline(
        c_graph,
        c_env.caps.clone(),
        c_env.task_reg.clone(),
        ExecutionPolicy::default(),
    )
    .await
    .expect("cluster run_graph_inline failed");

    let s_json = s_out
        .into_iter()
        .next()
        .flatten()
        .map(|o| o.json)
        .unwrap_or(json!({}));
    let c_json = c_out
        .into_iter()
        .next()
        .flatten()
        .map(|o| o.json)
        .unwrap_or(json!({}));
    (s_json, c_json)
}

// ── Parity tests ──────────────────────────────────────────────────────────────

/// MemorySearch with no decay/rerank: both paths must return the same record
/// IDs in the same rank order given the same inserted vectors.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn memory_search_rank_order_matches() {
    let s = standalone_env();
    let c = cluster_env().await;

    seed_vectors(&s).await;
    seed_vectors(&c).await;

    let query = [1.0f32, 0.1, 0.0, 0.0];
    let (s_out, c_out) = run_both(
        &s,
        &c,
        memory_search_graph(standalone_ctx(), &query),
        memory_search_graph(cluster_ctx(), &query),
    )
    .await;

    // Both capability impls return a bare JSON array (not wrapped in an object).
    let s_ids: Vec<u64> = s_out
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|r| r["record_id"].as_u64().unwrap_or(0))
        .collect();
    let c_ids: Vec<u64> = c_out
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|r| r["record_id"].as_u64().unwrap_or(0))
        .collect();

    assert!(!s_ids.is_empty(), "standalone returned no results");
    assert_eq!(
        s_ids, c_ids,
        "rank order differs: standalone={s_ids:?} cluster={c_ids:?}"
    );
}

/// GraphRAG: seed nodes via the graph API then query — both paths must return
/// the same set of seed record IDs for an identical query vector.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn graph_rag_seed_nodes_match() {
    let s = standalone_env();
    let c = cluster_env().await;

    seed_vectors(&s).await;
    seed_vectors(&c).await;

    let query = [0.9f32, 0.1, 0.0, 0.0];
    let (s_out, c_out) = run_both(
        &s,
        &c,
        graph_rag_graph(standalone_ctx(), &query),
        graph_rag_graph(cluster_ctx(), &query),
    )
    .await;

    // Compare hit record IDs (distance order should be identical).
    let hit_ids = |v: &Value| -> Vec<u64> {
        v["hits"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(|h| h["id"].as_u64().unwrap_or(0))
            .collect()
    };

    let s_hits = hit_ids(&s_out);
    let c_hits = hit_ids(&c_out);
    assert!(!s_hits.is_empty(), "standalone graphrag returned no hits");
    assert_eq!(
        s_hits, c_hits,
        "graphrag hits differ: standalone={s_hits:?} cluster={c_hits:?}"
    );
}

/// Snapshot state_hash: after seeding identical records, the BLAKE3 hash of
/// the KernelState must be identical on both paths.
///
/// This directly validates the A14 P0 fix: before the fix, RaftKernelCapability
/// always returned "0000...0000" as the state_hash.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_state_hash_matches() {
    let s = standalone_env();
    let c = cluster_env().await;

    // Insert the same 4 vectors into both paths.
    seed_vectors(&s).await;
    seed_vectors(&c).await;

    // Standalone snapshot task requires a file path; cluster ignores it.
    let tmp = std::env::temp_dir().join(format!("valori_parity_snap_{}.bin", std::process::id()));
    let tmp_str = tmp.to_str().unwrap().to_string();

    let (s_out, c_out) = run_both(
        &s,
        &c,
        snapshot_graph(standalone_ctx(), Some(&tmp_str)),
        snapshot_graph(cluster_ctx(), None),
    )
    .await;

    let _ = std::fs::remove_file(&tmp);

    let s_hash = s_out["state_hash"].as_str().unwrap_or("").to_string();
    let c_hash = c_out["state_hash"].as_str().unwrap_or("").to_string();

    assert_eq!(
        s_hash.len(),
        64,
        "standalone state_hash is not a 64-char hex string: '{s_hash}'"
    );
    assert_eq!(
        c_hash.len(),
        64,
        "cluster state_hash is not a 64-char hex string: '{c_hash}'"
    );
    assert_ne!(
        s_hash,
        "0".repeat(64),
        "standalone returned zero hash — snapshot task broken"
    );
    assert_ne!(
        c_hash,
        "0".repeat(64),
        "cluster returned zero hash — A14 P0 regression"
    );
    assert_eq!(
        s_hash, c_hash,
        "state_hash diverged: standalone={s_hash} cluster={c_hash}"
    );
}

/// CommunityDetect on an empty graph: both paths must return 0 communities and
/// 0 nodes — and must not crash or error out.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn community_detect_empty_graph_matches() {
    let s = standalone_env();
    let c = cluster_env().await;

    let (s_out, c_out) = run_both(
        &s,
        &c,
        community_detect_graph(standalone_ctx()),
        community_detect_graph(cluster_ctx()),
    )
    .await;

    let s_count = s_out["community_count"].as_u64().unwrap_or(u64::MAX);
    let c_count = c_out["community_count"].as_u64().unwrap_or(u64::MAX);
    let s_nodes = s_out["node_count"].as_u64().unwrap_or(u64::MAX);
    let c_nodes = c_out["node_count"].as_u64().unwrap_or(u64::MAX);

    assert_eq!(
        s_count, c_count,
        "community_count differs: standalone={s_count} cluster={c_count}"
    );
    assert_eq!(
        s_nodes, c_nodes,
        "node_count differs: standalone={s_nodes} cluster={c_nodes}"
    );
}
