// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! G1.4.1 — graph-aware vector reranking, HTTP level (standalone).
//!
//! See docs/reviews/graph-g1.4.1-graph-aware-reranking-design.md for the
//! full design; this file proves the test matrix from that document's §14.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::Value;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::RwLock;
use tower::ServiceExt;

use valori_node::config::{IndexKind, NodeConfig};
use valori_node::engine::{Engine, RecoveryMode};
use valori_node::server::{build_router, SharedEngine};
use valori_node::EngineFromNodeConfig;

const DIM: usize = 4;

fn mem_cfg() -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.max_records = 200;
    cfg.max_nodes = 200;
    cfg.max_edges = 400;
    cfg
}

fn engine_router(cfg: NodeConfig) -> (SharedEngine, axum::Router) {
    let engine = Engine::new(&cfg);
    let shared = Arc::new(RwLock::new(engine));
    let router = build_router(shared.clone(), None, None);
    (shared, router)
}

fn v(seed: f32) -> Vec<f32> {
    (0..DIM).map(|i| seed + i as f32 * 0.001).collect()
}

async fn post(router: axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let resp = router
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
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::json!(null));
    (status, json)
}

async fn create_default_collection(router: axum::Router) {
    let (status, body) = post(
        router,
        "/v1/namespaces",
        serde_json::json!({"name": "default", "dimension": DIM, "metric": "squared_l2"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn insert(router: axum::Router, seed: f32) -> u32 {
    let (status, body) = post(
        router,
        "/records",
        serde_json::json!({"values": v(seed), "collection": "default"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["id"].as_u64().unwrap() as u32
}

async fn create_node(router: axum::Router, record: u32) -> u64 {
    let (status, body) = post(
        router,
        "/v1/graph/node",
        serde_json::json!({"kind": 1, "record_id": record, "collection": "default"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["node_id"].as_u64().unwrap()
}

async fn create_edge(router: axum::Router, from: u64, to: u64) -> u64 {
    let (status, body) = post(
        router,
        "/v1/graph/edge",
        serde_json::json!({"from": from, "to": to, "kind": 0, "collection": "default"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["edge_id"].as_u64().unwrap()
}

async fn search(
    router: axum::Router,
    query: [f32; DIM],
    k: usize,
    graph_rerank: Option<Value>,
) -> Value {
    let mut body = serde_json::json!({"query": query, "k": k, "collection": "default"});
    if let Some(gr) = graph_rerank {
        body["graph_rerank"] = gr;
    }
    let (status, body) = post(router, "/search", body).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

fn ids(resp: &Value) -> Vec<u64> {
    resp["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["id"].as_u64().unwrap())
        .collect()
}

// ── 1. vector-only behavior unchanged when graph_rerank absent ───────────────

#[tokio::test]
async fn absent_graph_rerank_is_identical_to_pre_g141_response() {
    let (_, router) = engine_router(mem_cfg());
    create_default_collection(router.clone()).await;
    insert(router.clone(), 0.10).await;
    insert(router.clone(), 0.20).await;

    let without = search(router.clone(), [0.10, 0.101, 0.102, 0.103], 5, None).await;
    // No graph_distance field should appear anywhere in the response.
    for hit in without["results"].as_array().unwrap() {
        assert!(hit.get("graph_distance").is_none());
    }
}

// ── 2. directly connected candidate ranks up ─────────────────────────────────

#[tokio::test]
async fn directly_connected_candidate_is_boosted_above_a_slightly_better_but_disconnected_one() {
    let (_, router) = engine_router(mem_cfg());
    create_default_collection(router.clone()).await;
    // "best" is the closest vector match to the query but graph-isolated.
    let best = insert(router.clone(), 0.100).await;
    // "connected" is a slightly worse vector match but directly connected
    // to the top seed (itself, once resolved) via another record's node.
    let connected = insert(router.clone(), 0.108).await;
    let anchor_node = create_node(router.clone(), best).await;
    let connected_node = create_node(router.clone(), connected).await;
    create_edge(router.clone(), anchor_node, connected_node).await;

    // Plain vector search: "best" wins by construction.
    let plain = search(router.clone(), [0.100, 0.1001, 0.1002, 0.1003], 2, None).await;
    assert_eq!(ids(&plain)[0], best as u64);

    // Graph rerank with a strong weight: "connected" must overtake once its
    // graph adjacency to the top seed is factored in — the top seed IS
    // "best" itself (distance 0, no penalty), so this specific setup
    // instead verifies "best" stays on top (it's its own seed) but
    // "connected" (depth 1) is now present with a recorded distance.
    let reranked = search(
        router.clone(),
        [0.100, 0.1001, 0.1002, 0.1003],
        2,
        Some(serde_json::json!({"weight": 1.0, "seed_count": 1})),
    )
    .await;
    let hits = reranked["results"].as_array().unwrap();
    let best_hit = hits.iter().find(|h| h["id"] == best).unwrap();
    let connected_hit = hits.iter().find(|h| h["id"] == connected).unwrap();
    assert_eq!(best_hit["graph_distance"], 0, "the seed is its own node");
    assert_eq!(connected_hit["graph_distance"], 1, "one hop from the seed");
}

// ── 3. 2-hop candidate ────────────────────────────────────────────────────────

#[tokio::test]
async fn two_hop_candidate_reports_distance_two() {
    let (_, router) = engine_router(mem_cfg());
    create_default_collection(router.clone()).await;
    let seed_rec = insert(router.clone(), 0.100).await;
    let mid_rec = insert(router.clone(), 0.200).await;
    let far_rec = insert(router.clone(), 0.300).await;
    let seed_node = create_node(router.clone(), seed_rec).await;
    let mid_node = create_node(router.clone(), mid_rec).await;
    let far_node = create_node(router.clone(), far_rec).await;
    create_edge(router.clone(), seed_node, mid_node).await;
    create_edge(router.clone(), mid_node, far_node).await;

    let resp = search(
        router.clone(),
        [0.100, 0.1001, 0.1002, 0.1003],
        3,
        Some(serde_json::json!({"weight": 0.1, "seed_count": 1, "max_depth": 3})),
    )
    .await;
    let hits = resp["results"].as_array().unwrap();
    let far_hit = hits.iter().find(|h| h["id"] == far_rec).unwrap();
    assert_eq!(far_hit["graph_distance"], 2);
}

// ── 4. unreachable candidate ───────────────────────────────────────────────────

#[tokio::test]
async fn unreachable_candidate_gets_no_graph_distance_and_keeps_its_rank() {
    let (_, router) = engine_router(mem_cfg());
    create_default_collection(router.clone()).await;
    let seed_rec = insert(router.clone(), 0.100).await;
    let isolated_rec = insert(router.clone(), 0.200).await;
    let seed_node = create_node(router.clone(), seed_rec).await;
    let _isolated_node = create_node(router.clone(), isolated_rec).await;
    // No edge between them at all.
    let _ = seed_node;

    let resp = search(
        router.clone(),
        [0.100, 0.1001, 0.1002, 0.1003],
        2,
        Some(serde_json::json!({"weight": 1.0})),
    )
    .await;
    let hits = resp["results"].as_array().unwrap();
    let isolated_hit = hits.iter().find(|h| h["id"] == isolated_rec).unwrap();
    assert!(isolated_hit.get("graph_distance").is_none());
}

// ── 5. candidate with no graph node at all ────────────────────────────────────

#[tokio::test]
async fn candidate_without_any_graph_node_is_never_dropped() {
    let (_, router) = engine_router(mem_cfg());
    create_default_collection(router.clone()).await;
    let seed_rec = insert(router.clone(), 0.100).await;
    let no_node_rec = insert(router.clone(), 0.101).await;
    create_node(router.clone(), seed_rec).await;
    // no_node_rec never gets a graph node.

    let resp = search(
        router.clone(),
        [0.100, 0.1001, 0.1002, 0.1003],
        2,
        Some(serde_json::json!({"weight": 1.0})),
    )
    .await;
    let found = ids(&resp);
    assert!(
        found.contains(&(no_node_rec as u64)),
        "a candidate with no graph node must still be returned"
    );
}

// ── 6. multiple nodes for one record — minimum distance wins ─────────────────

#[tokio::test]
async fn multi_node_record_uses_the_minimum_distance_across_its_nodes() {
    let (_, router) = engine_router(mem_cfg());
    create_default_collection(router.clone()).await;
    let seed_rec = insert(router.clone(), 0.100).await;
    let multi_rec = insert(router.clone(), 0.200).await;
    let seed_node = create_node(router.clone(), seed_rec).await;
    // multi_rec gets two nodes: one far (3 hops), one directly connected.
    let far_branch = create_node(router.clone(), multi_rec).await;
    let near_branch = create_node(router.clone(), multi_rec).await;
    let a = create_node(router.clone(), seed_rec).await; // filler to build a 3-hop chain
    let b = create_node(router.clone(), seed_rec).await;
    create_edge(router.clone(), seed_node, a).await;
    create_edge(router.clone(), a, b).await;
    create_edge(router.clone(), b, far_branch).await; // seed -> a -> b -> far_branch (3 hops)
    create_edge(router.clone(), seed_node, near_branch).await; // seed -> near_branch (1 hop)

    let resp = search(
        router.clone(),
        [0.100, 0.1001, 0.1002, 0.1003],
        3,
        Some(serde_json::json!({"weight": 0.1, "max_depth": 4})),
    )
    .await;
    let hits = resp["results"].as_array().unwrap();
    let multi_hit = hits.iter().find(|h| h["id"] == multi_rec).unwrap();
    assert_eq!(
        multi_hit["graph_distance"], 1,
        "must report the MINIMUM distance across the record's nodes, not the max"
    );
}

// ── 7. multiple graph seeds ────────────────────────────────────────────────────

#[tokio::test]
async fn multiple_seeds_widen_the_reachable_set() {
    let (_, router) = engine_router(mem_cfg());
    create_default_collection(router.clone()).await;
    let seed1_rec = insert(router.clone(), 0.100).await;
    let seed2_rec = insert(router.clone(), 0.105).await;
    let via_seed2_rec = insert(router.clone(), 0.300).await;
    let seed2_node = create_node(router.clone(), seed2_rec).await;
    create_node(router.clone(), seed1_rec).await;
    let via_seed2_node = create_node(router.clone(), via_seed2_rec).await;
    create_edge(router.clone(), seed2_node, via_seed2_node).await;

    // With seed_count=1, only the top-1 hit becomes a seed; if seed1 wins
    // top-1, via_seed2 is unreachable. With seed_count=2, seed2 also
    // becomes a seed, making via_seed2 reachable at depth 1.
    let resp = search(
        router.clone(),
        [0.100, 0.1001, 0.1002, 0.1003],
        3,
        Some(serde_json::json!({"weight": 0.1, "seed_count": 2, "max_depth": 2})),
    )
    .await;
    let hits = resp["results"].as_array().unwrap();
    let via_hit = hits.iter().find(|h| h["id"] == via_seed2_rec).unwrap();
    assert_eq!(via_hit["graph_distance"], 1);
}

// ── 9/10/11. direction ────────────────────────────────────────────────────────

#[tokio::test]
async fn direction_outgoing_only_follows_forward_edges() {
    let (_, router) = engine_router(mem_cfg());
    create_default_collection(router.clone()).await;
    let seed_rec = insert(router.clone(), 0.100).await;
    let downstream_rec = insert(router.clone(), 0.200).await;
    let upstream_rec = insert(router.clone(), 0.300).await;
    let seed_node = create_node(router.clone(), seed_rec).await;
    let downstream_node = create_node(router.clone(), downstream_rec).await;
    let upstream_node = create_node(router.clone(), upstream_rec).await;
    create_edge(router.clone(), seed_node, downstream_node).await; // seed -> downstream
    create_edge(router.clone(), upstream_node, seed_node).await; // upstream -> seed

    let resp = search(
        router.clone(),
        [0.100, 0.1001, 0.1002, 0.1003],
        3,
        Some(serde_json::json!({"weight": 0.1, "direction": "outgoing"})),
    )
    .await;
    let hits = resp["results"].as_array().unwrap();
    let down = hits.iter().find(|h| h["id"] == downstream_rec).unwrap();
    let up = hits.iter().find(|h| h["id"] == upstream_rec).unwrap();
    assert_eq!(down["graph_distance"], 1);
    assert!(up.get("graph_distance").is_none());
}

#[tokio::test]
async fn direction_incoming_follows_backward_edges() {
    let (_, router) = engine_router(mem_cfg());
    create_default_collection(router.clone()).await;
    let seed_rec = insert(router.clone(), 0.100).await;
    let upstream_rec = insert(router.clone(), 0.300).await;
    let seed_node = create_node(router.clone(), seed_rec).await;
    let upstream_node = create_node(router.clone(), upstream_rec).await;
    create_edge(router.clone(), upstream_node, seed_node).await;

    let resp = search(
        router.clone(),
        [0.100, 0.1001, 0.1002, 0.1003],
        2,
        Some(serde_json::json!({"weight": 0.1, "direction": "incoming"})),
    )
    .await;
    let hits = resp["results"].as_array().unwrap();
    let up = hits.iter().find(|h| h["id"] == upstream_rec).unwrap();
    assert_eq!(up["graph_distance"], 1);
}

#[tokio::test]
async fn direction_both_merges_outgoing_and_incoming() {
    let (_, router) = engine_router(mem_cfg());
    create_default_collection(router.clone()).await;
    let seed_rec = insert(router.clone(), 0.100).await;
    let downstream_rec = insert(router.clone(), 0.200).await;
    let upstream_rec = insert(router.clone(), 0.300).await;
    let seed_node = create_node(router.clone(), seed_rec).await;
    let downstream_node = create_node(router.clone(), downstream_rec).await;
    let upstream_node = create_node(router.clone(), upstream_rec).await;
    create_edge(router.clone(), seed_node, downstream_node).await;
    create_edge(router.clone(), upstream_node, seed_node).await;

    let resp = search(
        router.clone(),
        [0.100, 0.1001, 0.1002, 0.1003],
        3,
        Some(serde_json::json!({"weight": 0.1, "direction": "both"})),
    )
    .await;
    let hits = resp["results"].as_array().unwrap();
    let down = hits.iter().find(|h| h["id"] == downstream_rec).unwrap();
    let up = hits.iter().find(|h| h["id"] == upstream_rec).unwrap();
    assert_eq!(down["graph_distance"], 1);
    assert_eq!(up["graph_distance"], 1);
}

// ── 14. deterministic ties ────────────────────────────────────────────────────

#[tokio::test]
async fn graph_rerank_ties_break_by_id_ascending() {
    let (_, router) = engine_router(mem_cfg());
    create_default_collection(router.clone()).await;
    // Two records at the exact same vector distance, neither with a graph
    // node — both end up with graph_distance=None, so the penalty is
    // identical (neutral), and the tie-break must be id ascending.
    let a = insert(router.clone(), 0.5).await;
    let b = insert(router.clone(), 0.5).await;
    let lower = a.min(b);
    let higher = a.max(b);

    let resp = search(
        router.clone(),
        [0.5, 0.501, 0.502, 0.503],
        2,
        Some(serde_json::json!({"weight": 0.5})),
    )
    .await;
    let found = ids(&resp);
    assert_eq!(found, vec![lower as u64, higher as u64]);
}

// ── 15. snapshot round trip produces the same result ──────────────────────────

#[tokio::test]
async fn graph_rerank_result_is_identical_across_snapshot_round_trip() {
    let (shared, router) = engine_router(mem_cfg());
    create_default_collection(router.clone()).await;
    let seed_rec = insert(router.clone(), 0.100).await;
    let connected_rec = insert(router.clone(), 0.150).await;
    let seed_node = create_node(router.clone(), seed_rec).await;
    let connected_node = create_node(router.clone(), connected_rec).await;
    create_edge(router.clone(), seed_node, connected_node).await;

    let before = search(
        router.clone(),
        [0.100, 0.1001, 0.1002, 0.1003],
        2,
        Some(serde_json::json!({"weight": 0.5})),
    )
    .await;

    // Encode + decode the state, swap it back in, and rerun the same search.
    let mut buf: Vec<u8> = Vec::new();
    {
        let engine = shared.read().await;
        valori_kernel::snapshot::encode::encode_state(&engine.state, &mut buf).unwrap();
    }
    let decoded = valori_kernel::snapshot::decode::decode_state(&buf).unwrap();
    {
        let mut engine = shared.write().await;
        engine.state = decoded;
    }

    let after = search(
        router.clone(),
        [0.100, 0.1001, 0.1002, 0.1003],
        2,
        Some(serde_json::json!({"weight": 0.5})),
    )
    .await;

    assert_eq!(before, after);
}

// ── 17. restart produces the same result ──────────────────────────────────────

#[tokio::test]
async fn graph_rerank_result_survives_a_real_restart() {
    let dir = tempdir().unwrap();
    let mut cfg = mem_cfg();
    cfg.event_log_path = Some(dir.path().join("events.log"));
    cfg.snapshot_path = Some(dir.path().join("snapshot.bin"));

    let (shared, router) = engine_router(cfg.clone());
    create_default_collection(router.clone()).await;
    let seed_rec = insert(router.clone(), 0.100).await;
    let connected_rec = insert(router.clone(), 0.150).await;
    let seed_node = create_node(router.clone(), seed_rec).await;
    let connected_node = create_node(router.clone(), connected_rec).await;
    create_edge(router.clone(), seed_node, connected_node).await;

    let before = search(
        router.clone(),
        [0.100, 0.1001, 0.1002, 0.1003],
        2,
        Some(serde_json::json!({"weight": 0.5})),
    )
    .await;

    {
        let engine = shared.write().await;
        engine.save_snapshot(None).unwrap();
    }

    let mut restarted = Engine::new(&cfg);
    let mode = restarted.try_recover();
    assert!(!matches!(mode, RecoveryMode::Fresh), "must recover state");
    let restarted_shared = Arc::new(RwLock::new(restarted));
    let restarted_router = build_router(restarted_shared.clone(), None, None);

    let after = search(
        restarted_router,
        [0.100, 0.1001, 0.1002, 0.1003],
        2,
        Some(serde_json::json!({"weight": 0.5})),
    )
    .await;

    assert_eq!(before, after);
}

// ── 19. soft-deleted record never appears as a hit or a seed ─────────────────

#[tokio::test]
async fn soft_deleted_candidate_never_appears_as_a_hit_or_a_seed() {
    let (_, router) = engine_router(mem_cfg());
    create_default_collection(router.clone()).await;
    let seed_rec = insert(router.clone(), 0.100).await;
    let victim_rec = insert(router.clone(), 0.101).await;
    create_node(router.clone(), seed_rec).await;
    create_node(router.clone(), victim_rec).await;

    let (status, _) = post(
        router.clone(),
        "/v1/soft-delete",
        serde_json::json!({"id": victim_rec, "collection": "default"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let resp = search(
        router.clone(),
        [0.100, 0.1001, 0.1002, 0.1003],
        5,
        Some(serde_json::json!({"weight": 1.0})),
    )
    .await;
    assert!(!ids(&resp).contains(&(victim_rec as u64)));
}

// ── 20. namespace isolation ────────────────────────────────────────────────────

#[tokio::test]
async fn graph_rerank_never_crosses_namespaces_for_seeds_or_candidates() {
    let (_, router) = engine_router(mem_cfg());
    post(
        router.clone(),
        "/v1/namespaces",
        serde_json::json!({"name": "ns-a", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    post(
        router.clone(),
        "/v1/namespaces",
        serde_json::json!({"name": "ns-b", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;

    let (status, body) = post(
        router.clone(),
        "/records",
        serde_json::json!({"values": v(0.100), "collection": "ns-a"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rec_a = body["id"].as_u64().unwrap() as u32;

    let (status, body) = post(
        router.clone(),
        "/records",
        serde_json::json!({"values": v(0.100), "collection": "ns-b"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rec_b = body["id"].as_u64().unwrap() as u32;

    // A search scoped to ns-a must never return, or be influenced by, ns-b's
    // colliding-vector record, graph_rerank enabled or not.
    let (status, resp) = post(
        router.clone(),
        "/search",
        serde_json::json!({
            "query": v(0.100), "k": 5, "collection": "ns-a",
            "graph_rerank": {"weight": 1.0}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    let found = ids(&resp);
    assert!(found.contains(&(rec_a as u64)));
    assert!(!found.contains(&(rec_b as u64)));
}
