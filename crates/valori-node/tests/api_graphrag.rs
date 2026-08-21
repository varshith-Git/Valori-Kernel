// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! GraphRAG endpoint tests — Phase 3.15 through Phase 5.4.
//!
//! Phases 3.15 / 5.2 / 5.3: one-call composition, provenance, deduplication,
//!   retrieval_k/final_k split, min-distance HashMap, score fields.
//! Phase 5.4: graph-aware reranking, max_nodes/max_edges BFS budgets,
//!   graph_score field, final_score for all hits, final_k defaults to retrieval_k.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;
use valori_node::config::{IndexKind, NodeConfig};
use valori_node::engine::Engine;
use valori_node::server::build_router;
use valori_node::EngineFromNodeConfig;

const DIM: usize = 4;

fn make_shared() -> Arc<RwLock<Engine>> {
    let mut cfg = NodeConfig::default();
    cfg.max_records = 100;
    cfg.max_nodes = 64;
    cfg.max_edges = 64;
    cfg.event_log_path = None;
    cfg.wal_path = None;
    Arc::new(RwLock::new(Engine::new(&cfg)))
}

async fn post(
    shared: &Arc<RwLock<Engine>>,
    path: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let app = build_router(shared.clone(), None, None);
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

fn vec_n(seed: f32) -> Vec<f32> {
    (0..DIM).map(|i| seed + i as f32 * 0.01).collect()
}

async fn create_default_collection(shared: &Arc<RwLock<Engine>>) {
    let (st, body) = post(
        shared,
        "/v1/namespaces",
        serde_json::json!({"name": "default", "dimension": DIM, "metric": "squared_l2"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn graphrag_returns_hits_and_connected_subgraph() {
    let shared = make_shared();
    create_default_collection(&shared).await;

    // Write a memory → creates a Document node, a Chunk node, and a doc→chunk edge.
    let (st, w) = post(
        &shared,
        "/v1/memory/upsert_vector",
        serde_json::json!({ "vector": vec_n(0.10), "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let chunk = w["chunk_node_id"].as_u64().unwrap();
    let doc = w["document_node_id"].as_u64().unwrap();

    // Add an edge OUT of the chunk so the subgraph around the hit is non-trivial.
    let (st, _) = post(
        &shared,
        "/graph/edge",
        serde_json::json!({ "from": chunk, "to": doc, "kind": 0, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // Two more memories so KNN has alternatives.
    post(
        &shared,
        "/v1/memory/upsert_vector",
        serde_json::json!({ "vector": vec_n(0.5), "collection": "default" }),
    )
    .await;
    post(
        &shared,
        "/v1/memory/upsert_vector",
        serde_json::json!({ "vector": vec_n(0.9), "collection": "default" }),
    )
    .await;

    // One GraphRAG call.
    let (st, out) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({ "query_vector": vec_n(0.10), "k": 3, "depth": 2, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // Hits came back, the nearest being our seed memory.
    let hits = out["hits"].as_array().unwrap();
    assert!(!hits.is_empty(), "expected hits");
    assert_eq!(
        hits[0]["node_id"].as_u64(),
        Some(chunk),
        "nearest hit maps to its chunk node"
    );

    // The subgraph expanded from the seed and includes the chunk→doc edge.
    let seeds = out["seed_nodes"].as_array().unwrap();
    assert!(
        seeds.iter().any(|s| s.as_u64() == Some(chunk)),
        "seed nodes should include the chunk"
    );
    let nodes = out["subgraph"]["nodes"].as_array().unwrap();
    let edges = out["subgraph"]["edges"].as_array().unwrap();
    assert!(nodes.iter().any(|n| n["id"].as_u64() == Some(chunk)));
    assert!(
        nodes.iter().any(|n| n["id"].as_u64() == Some(doc)),
        "expanded to the doc node one hop out"
    );
    assert!(
        edges
            .iter()
            .any(|e| e["from"].as_u64() == Some(chunk) && e["to"].as_u64() == Some(doc)),
        "the chunk→doc edge must be present"
    );
}

#[tokio::test]
async fn graphrag_on_empty_store_is_empty_not_error() {
    let shared = make_shared();
    create_default_collection(&shared).await;
    let (st, out) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({ "query_vector": vec_n(0.1), "k": 5, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(out["hits"].as_array().unwrap().len(), 0);
    assert_eq!(out["subgraph"]["nodes"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn graphrag_depth_zero_returns_seeds_without_edges() {
    let shared = make_shared();
    create_default_collection(&shared).await;
    let (_, w) = post(
        &shared,
        "/v1/memory/upsert_vector",
        serde_json::json!({ "vector": vec_n(0.2), "collection": "default" }),
    )
    .await;
    let chunk = w["chunk_node_id"].as_u64().unwrap();
    let doc = w["document_node_id"].as_u64().unwrap();
    post(
        &shared,
        "/graph/edge",
        serde_json::json!({ "from": chunk, "to": doc, "kind": 0, "collection": "default" }),
    )
    .await;

    let (st, out) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({ "query_vector": vec_n(0.2), "k": 1, "depth": 0, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    // depth 0 → the seed node itself, no edge traversal.
    assert!(out["subgraph"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["id"].as_u64() == Some(chunk)));
    assert_eq!(out["subgraph"]["edges"].as_array().unwrap().len(), 0);
}

// ── G1.3: seed resolution derives from canonical state, not a stale cache ────

/// A record may legitimately have several graph nodes (`CreateNode` imposes
/// no uniqueness on `record`). The standalone path used to resolve this via
/// an engine-local last-write-wins `record_to_node` cache while the cluster
/// path used `resolve_seed_nodes` (first-in-pool-order wins) — so identical
/// canonical state produced different `node_id`/seeds on the two paths.
/// Both now share `resolve_seed_nodes`; this pins the standalone side at the
/// HTTP boundary. See docs/reviews/graph-g1.3-vector-graph-retrieval.md.
#[tokio::test]
async fn graphrag_seed_for_a_multi_node_record_is_the_lowest_node_id() {
    let shared = make_shared();
    create_default_collection(&shared).await;

    let (st, w) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(0.10), "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let record_id = w["id"].as_u64().expect("insert returns a record id");

    // Two nodes on the same record; the SECOND one is what the old cache
    // would have reported.
    let (st, a) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 6, "record_id": record_id, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let first_node = a["node_id"].as_u64().unwrap();

    let (st, b) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 5, "record_id": record_id, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let second_node = b["node_id"].as_u64().unwrap();
    assert!(second_node > first_node, "sanity: node ids ascend");

    let (st, out) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({ "query_vector": vec_n(0.10), "k": 1, "depth": 1, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    assert_eq!(
        out["hits"][0]["node_id"].as_u64(),
        Some(first_node),
        "standalone must report the canonical first-in-pool-order node, \
         matching the cluster path"
    );
    assert_eq!(
        out["seed_nodes"].as_array().unwrap(),
        &vec![serde_json::json!(first_node)],
        "and must seed expansion from that same node"
    );
}

/// Deleting one of several nodes on a record must leave the survivor
/// resolvable — the old cache dropped the mapping outright, so expansion
/// silently stopped until a restart rebuilt the map.
#[tokio::test]
async fn graphrag_still_seeds_from_the_surviving_node_after_a_sibling_delete() {
    let shared = make_shared();
    create_default_collection(&shared).await;

    let (_, w) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(0.10), "collection": "default" }),
    )
    .await;
    let record_id = w["id"].as_u64().unwrap();

    let (_, a) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 6, "record_id": record_id, "collection": "default" }),
    )
    .await;
    let first_node = a["node_id"].as_u64().unwrap();
    let (_, b) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 5, "record_id": record_id, "collection": "default" }),
    )
    .await;
    let second_node = b["node_id"].as_u64().unwrap();

    // Remove the lower-id node; the higher-id one still points at the record.
    let app = build_router(shared.clone(), None, None);
    let resp = app
        .oneshot(
            axum::http::Request::builder()
                .method("DELETE")
                .uri(format!("/v1/graph/node/{first_node}?collection=default"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let (st, out) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({ "query_vector": vec_n(0.10), "k": 1, "depth": 1, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        out["hits"][0]["node_id"].as_u64(),
        Some(second_node),
        "the surviving node must still seed expansion"
    );
}

// ── Phase 5.2: provenance, graph-only candidates, deduplication ───────────────

/// A record with no graph node must survive as a vector candidate in hits
/// (source="vector", node_id=null, graph_distance=null).  This is the
/// "pure vector hit" path: graph absence must not discard the record.
#[tokio::test]
async fn graphrag_record_without_graph_node_remains_in_hits() {
    let shared = make_shared();
    create_default_collection(&shared).await;

    // Insert a record directly — no memory_upsert, so NO graph node is created.
    let (st, w) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(0.10), "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let record_id = w["id"].as_u64().unwrap();

    let (st, out) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({ "query_vector": vec_n(0.10), "k": 1, "depth": 2, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let hits = out["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1, "the record must appear as a hit");
    assert_eq!(hits[0]["record_id"].as_u64(), Some(record_id));
    assert!(
        hits[0]["node_id"].is_null(),
        "no graph node → node_id must be null"
    );
    assert_eq!(
        hits[0]["source"].as_str(),
        Some("vector"),
        "provenance must be 'vector'"
    );
    assert!(
        hits[0]["graph_distance"].is_null(),
        "no graph node → graph_distance must be null"
    );
}

/// A record that is NOT among the top vector hits but IS reachable via graph
/// expansion from a seed must appear in hits with source="graph".
///
/// Setup:
///   record A  (near query vector) → graph node N_A (seed)
///   record B  (far from query, so NOT a top-k vector hit)
///   edge  N_A → N_B  (creates the graph path)
///
/// GraphRAG must return both A (vector hit, source=vector_and_graph) and
/// B (graph-only candidate, source=graph, score=null, graph_distance=1).
#[tokio::test]
async fn graphrag_graph_only_candidate_appears_in_hits() {
    let shared = make_shared();
    create_default_collection(&shared).await;

    // Record A: very close to the query vector → will be top-1 vector hit.
    let (_, wa) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(0.10), "collection": "default" }),
    )
    .await;
    let record_a = wa["id"].as_u64().unwrap();

    // Record B: far from the query → will NOT appear in a k=1 vector search.
    let (_, wb) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(100.0), "collection": "default" }),
    )
    .await;
    let record_b = wb["id"].as_u64().unwrap();

    // Create graph node for A (N_A) and B (N_B).
    let (_, na) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": record_a, "collection": "default" }),
    )
    .await;
    let node_a = na["node_id"].as_u64().unwrap();
    let (_, nb) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": record_b, "collection": "default" }),
    )
    .await;
    let node_b = nb["node_id"].as_u64().unwrap();

    // Edge N_A → N_B (so B is reachable from seed A at depth 1).
    let (st, _) = post(
        &shared,
        "/v1/graph/edge",
        serde_json::json!({ "from": node_a, "to": node_b, "kind": 0, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // k=1 so only A is a vector hit; depth=2 so B is reachable from A.
    // Phase 5.4: final_k defaults to retrieval_k=1, so explicitly pass final_k=10
    // to allow both A (vector) and B (graph-only) to be returned.
    let (st, out) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({ "query_vector": vec_n(0.10), "k": 1, "depth": 2, "final_k": 10, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let hits = out["hits"].as_array().unwrap();
    // Both A and B must appear in hits.
    assert_eq!(
        hits.len(),
        2,
        "A (vector) and B (graph-only) must both be candidates"
    );

    let hit_a = hits
        .iter()
        .find(|h| h["record_id"].as_u64() == Some(record_a))
        .expect("record A must be in hits");
    let hit_b = hits
        .iter()
        .find(|h| h["record_id"].as_u64() == Some(record_b))
        .expect("record B must be in hits");

    assert_eq!(
        hit_a["source"].as_str(),
        Some("vector_and_graph"),
        "A is a vector hit with a graph node"
    );
    assert_eq!(
        hit_a["graph_distance"].as_u64(),
        Some(0),
        "seed A is at distance 0"
    );
    assert!(!hit_a["score"].is_null(), "A has a vector score");

    assert_eq!(
        hit_b["source"].as_str(),
        Some("graph"),
        "B is a graph-only candidate"
    );
    assert_eq!(
        hit_b["graph_distance"].as_u64(),
        Some(1),
        "B is one hop from seed A"
    );
    assert!(hit_b["score"].is_null(), "B has no vector score");
    assert_eq!(hit_b["node_id"].as_u64(), Some(node_b));
}

// ── Phase 5.3: retrieval_k/final_k, min-distance, scores, ordering ───────────

/// `vector_score` and `final_score` must be present on vector hits;
/// graph-only hits must have null for both.
#[tokio::test]
async fn graphrag_vector_score_and_final_score_fields() {
    let shared = make_shared();
    create_default_collection(&shared).await;

    // Record A near query, with a graph node so we get a graph-only B too.
    let (_, wa) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(0.10), "collection": "default" }),
    )
    .await;
    let record_a = wa["id"].as_u64().unwrap();

    let (_, wb) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(100.0), "collection": "default" }),
    )
    .await;
    let record_b = wb["id"].as_u64().unwrap();

    let (_, na) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": record_a, "collection": "default" }),
    )
    .await;
    let node_a = na["node_id"].as_u64().unwrap();
    let (_, nb) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": record_b, "collection": "default" }),
    )
    .await;
    let node_b = nb["node_id"].as_u64().unwrap();
    post(
        &shared,
        "/v1/graph/edge",
        serde_json::json!({ "from": node_a, "to": node_b, "kind": 0, "collection": "default" }),
    )
    .await;

    // Phase 5.4: final_k defaults to retrieval_k=1; pass final_k=10 to get both hits.
    let (st, out) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({ "query_vector": vec_n(0.10), "k": 1, "depth": 2, "final_k": 10, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let hits = out["hits"].as_array().unwrap();

    let hit_a = hits
        .iter()
        .find(|h| h["record_id"].as_u64() == Some(record_a))
        .expect("A must be in hits");
    let hit_b = hits
        .iter()
        .find(|h| h["record_id"].as_u64() == Some(record_b))
        .expect("B must be in hits");

    // Vector hit (A): vector_score must be a number; final_score is combined score.
    assert!(
        hit_a["vector_score"].as_f64().is_some(),
        "vector hit must have numeric vector_score"
    );
    assert!(
        hit_a["final_score"].as_f64().is_some(),
        "vector hit must have numeric final_score"
    );
    assert_eq!(
        hit_a["vector_score"], hit_a["score"],
        "vector_score must equal backward-compat score"
    );
    // Phase 5.4: graph_score is always present (0.0 for vector-only, 1.0 for seeds).
    assert!(
        hit_a["graph_score"].as_f64().is_some(),
        "graph_score must be a number on vector hits"
    );

    // Graph-only hit (B): vector_score and score must be null; final_score and
    // graph_score are numeric (Phase 5.4 — graph-only hits now have a combined score).
    assert!(
        hit_b["vector_score"].is_null(),
        "graph-only must have null vector_score"
    );
    assert!(
        hit_b["score"].is_null(),
        "graph-only must have null score (backward compat)"
    );
    // Phase 5.4: final_score is computed from graph_relevance and is always a number.
    assert!(
        hit_b["final_score"].as_f64().is_some(),
        "graph-only must have numeric final_score in Phase 5.4"
    );
    assert!(
        hit_b["graph_score"].as_f64().is_some(),
        "graph-only must have numeric graph_score"
    );
    // graph_score for a hop-1 candidate = 1/(1+1) = 0.5
    let g_score = hit_b["graph_score"].as_f64().unwrap();
    assert!(
        (g_score - 0.5).abs() < 1e-6,
        "hop-1 graph_score should be 0.5, got {g_score}"
    );
}

/// `final_k` must bound the number of returned hits even when the candidate
/// pool is larger.  retrieval_k controls vector seed count independently.
#[tokio::test]
async fn graphrag_final_k_bounds_result_count() {
    let shared = make_shared();
    create_default_collection(&shared).await;

    // Insert 5 records spread out — all will be vector hits with k=5.
    let mut node_ids: Vec<u64> = Vec::new();
    let mut record_ids: Vec<u64> = Vec::new();
    for i in 0..5u32 {
        let (_, w) = post(
            &shared,
            "/v1/records",
            serde_json::json!({ "values": vec_n(i as f32 * 0.1), "collection": "default" }),
        )
        .await;
        let rid = w["id"].as_u64().unwrap();
        record_ids.push(rid);
        let (_, n) = post(
            &shared,
            "/v1/graph/node",
            serde_json::json!({ "kind": 1, "record_id": rid, "collection": "default" }),
        )
        .await;
        node_ids.push(n["node_id"].as_u64().unwrap());
    }

    // Chain: 0→1→2→3→4 so graph expansion from top-1 can discover extra records.
    for i in 0..4 {
        post(
            &shared,
            "/v1/graph/edge",
            serde_json::json!({
                "from": node_ids[i],
                "to": node_ids[i + 1],
                "kind": 0,
                "collection": "default"
            }),
        )
        .await;
    }

    // retrieval_k=5 (all vector candidates) but final_k=3 → at most 3 hits.
    let (st, out) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({
            "query_vector": vec_n(0.0),
            "retrieval_k": 5,
            "final_k": 3,
            "depth": 4,
            "collection": "default"
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let hits = out["hits"].as_array().unwrap();
    assert!(
        hits.len() <= 3,
        "final_k=3 must cap results; got {}",
        hits.len()
    );
    let _ = record_ids; // suppress unused
}

/// `retrieval_k` is the canonical field name.  When only `k` is sent (legacy)
/// it must be interpreted as `retrieval_k` with no other change in behaviour.
#[tokio::test]
async fn graphrag_k_alias_backward_compat() {
    let shared = make_shared();
    create_default_collection(&shared).await;

    for i in 0..3u32 {
        post(
            &shared,
            "/v1/records",
            serde_json::json!({ "values": vec_n(i as f32 * 0.1), "collection": "default" }),
        )
        .await;
    }

    // Legacy request: only `k`, no `retrieval_k`.
    let (st, out_legacy) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({ "query_vector": vec_n(0.0), "k": 2, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // Explicit request: `retrieval_k=2`, no `k`.
    let (st2, out_new) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({ "query_vector": vec_n(0.0), "retrieval_k": 2, "collection": "default" }),
    )
    .await;
    assert_eq!(st2, StatusCode::OK);

    // Both must return the same number of vector hits.
    assert_eq!(
        out_legacy["hits"].as_array().unwrap().len(),
        out_new["hits"].as_array().unwrap().len(),
        "k and retrieval_k must produce identical result counts"
    );
}

/// When a record is reachable from two different graph paths with different
/// hop distances, graph_distance must be the MINIMUM (shortest path).
///
/// Diamond: A→B→D  and  A→C→D  (D is at distance 2 via both paths).
/// With k=1 only A is a vector hit; B, C, D are graph-only.
/// D must appear once with graph_distance=2 (not a longer or arbitrary distance).
#[tokio::test]
async fn graphrag_minimum_graph_distance_diamond() {
    let shared = make_shared();
    create_default_collection(&shared).await;

    // Insert four records: A near query, B/C/D far.
    async fn insert_rec(shared: &Arc<RwLock<Engine>>, seed: f32) -> (u64, u64) {
        let (_, w) = post(
            shared,
            "/v1/records",
            serde_json::json!({ "values": vec_n(seed), "collection": "default" }),
        )
        .await;
        let rid = w["id"].as_u64().unwrap();
        let (_, n) = post(
            shared,
            "/v1/graph/node",
            serde_json::json!({ "kind": 1, "record_id": rid, "collection": "default" }),
        )
        .await;
        let nid = n["node_id"].as_u64().unwrap();
        (rid, nid)
    }

    let (record_a, node_a) = insert_rec(&shared, 0.10).await;
    let (record_b, node_b) = insert_rec(&shared, 100.0).await;
    let (record_c, node_c) = insert_rec(&shared, 200.0).await;
    let (record_d, node_d) = insert_rec(&shared, 300.0).await;

    // Edges: A→B→D and A→C→D (diamond — D reachable via two paths, both length 2).
    for (from, to) in [
        (node_a, node_b),
        (node_b, node_d),
        (node_a, node_c),
        (node_c, node_d),
    ] {
        let (st, _) = post(
            &shared,
            "/v1/graph/edge",
            serde_json::json!({ "from": from, "to": to, "kind": 0, "collection": "default" }),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
    }

    // k=1 so only A is a vector hit; depth=3 so B, C, D are reachable.
    // Phase 5.4: final_k defaults to retrieval_k=1; use final_k=10 to get all hits.
    let (st, out) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({
            "query_vector": vec_n(0.10),
            "k": 1,
            "depth": 3,
            "final_k": 10,
            "collection": "default"
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let hits = out["hits"].as_array().unwrap();

    let hit_d = hits
        .iter()
        .find(|h| h["record_id"].as_u64() == Some(record_d))
        .expect("D must be in hits as graph-only candidate");

    assert_eq!(
        hit_d["graph_distance"].as_u64(),
        Some(2),
        "D is at distance 2 via both paths; minimum must be reported"
    );
    assert_eq!(hit_d["source"].as_str(), Some("graph"));
    assert!(hit_d["vector_score"].is_null());

    // Also verify B and C appear at distance 1.
    let hit_b = hits
        .iter()
        .find(|h| h["record_id"].as_u64() == Some(record_b))
        .expect("B must be in hits");
    let hit_c = hits
        .iter()
        .find(|h| h["record_id"].as_u64() == Some(record_c))
        .expect("C must be in hits");
    assert_eq!(hit_b["graph_distance"].as_u64(), Some(1));
    assert_eq!(hit_c["graph_distance"].as_u64(), Some(1));

    let _ = (record_b, record_c, node_d); // suppress unused
}

/// `max_graph_candidates` limits the number of graph-only hits included
/// BEFORE final_k truncation.  Closer candidates (lower graph_distance)
/// must be preferred when truncating.
#[tokio::test]
async fn graphrag_max_graph_candidates_budget() {
    let shared = make_shared();
    create_default_collection(&shared).await;

    // A is the seed; B (dist 1) and C (dist 2) are graph-only.
    let (_, wa) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(0.1), "collection": "default" }),
    )
    .await;
    let record_a = wa["id"].as_u64().unwrap();
    let (_, wb) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(100.0), "collection": "default" }),
    )
    .await;
    let record_b = wb["id"].as_u64().unwrap();
    let (_, wc) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(200.0), "collection": "default" }),
    )
    .await;
    let record_c = wc["id"].as_u64().unwrap();

    let (_, na) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": record_a, "collection": "default" }),
    )
    .await;
    let node_a = na["node_id"].as_u64().unwrap();
    let (_, nb) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": record_b, "collection": "default" }),
    )
    .await;
    let node_b = nb["node_id"].as_u64().unwrap();
    let (_, nc) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": record_c, "collection": "default" }),
    )
    .await;
    let node_c = nc["node_id"].as_u64().unwrap();

    // Chain: A→B→C  (B at dist 1, C at dist 2).
    post(
        &shared,
        "/v1/graph/edge",
        serde_json::json!({ "from": node_a, "to": node_b, "kind": 0, "collection": "default" }),
    )
    .await;
    post(
        &shared,
        "/v1/graph/edge",
        serde_json::json!({ "from": node_b, "to": node_c, "kind": 0, "collection": "default" }),
    )
    .await;

    // max_graph_candidates=1 → only the closest graph-only candidate (B at dist 1).
    // Phase 5.4: final_k defaults to retrieval_k=1; pass final_k=10 to allow A+B.
    let (st, out) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({
            "query_vector": vec_n(0.1),
            "k": 1,
            "depth": 3,
            "max_graph_candidates": 1,
            "final_k": 10,
            "collection": "default"
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let hits = out["hits"].as_array().unwrap();

    let record_ids_returned: Vec<u64> = hits
        .iter()
        .filter_map(|h| h["record_id"].as_u64())
        .collect();

    assert!(
        record_ids_returned.contains(&record_a),
        "A (vector hit) must be present"
    );
    assert!(
        record_ids_returned.contains(&record_b),
        "B (closest graph-only, dist 1) must be included"
    );
    assert!(
        !record_ids_returned.contains(&record_c),
        "C (farther graph-only, dist 2) must be excluded by max_graph_candidates=1"
    );
    assert_eq!(hits.len(), 2, "exactly A (vector) + B (graph) = 2 hits");

    let _ = (record_c, node_c); // suppress unused
}

/// Determinism: calling GraphRAG twice with identical state and query must
/// produce hits in the same order with the same record_ids.
#[tokio::test]
async fn graphrag_deterministic_ordering() {
    let shared = make_shared();
    create_default_collection(&shared).await;

    for i in 0..4u32 {
        let (_, w) = post(
            &shared,
            "/v1/records",
            serde_json::json!({ "values": vec_n(i as f32 * 0.1), "collection": "default" }),
        )
        .await;
        let rid = w["id"].as_u64().unwrap();
        let (_, n) = post(
            &shared,
            "/v1/graph/node",
            serde_json::json!({ "kind": 1, "record_id": rid, "collection": "default" }),
        )
        .await;
        let nid = n["node_id"].as_u64().unwrap();
        if i > 0 {
            // chain: 0→1→2→3
            post(
                &shared,
                "/v1/graph/edge",
                serde_json::json!({ "from": nid - 1, "to": nid, "kind": 0, "collection": "default" }),
            )
            .await;
        }
    }

    let body = serde_json::json!({
        "query_vector": vec_n(0.0),
        "k": 1,
        "depth": 3,
        "collection": "default"
    });

    let (_, out1) = post(&shared, "/v1/graphrag", body.clone()).await;
    let (_, out2) = post(&shared, "/v1/graphrag", body).await;

    let ids1: Vec<u64> = out1["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|h| h["record_id"].as_u64())
        .collect();
    let ids2: Vec<u64> = out2["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|h| h["record_id"].as_u64())
        .collect();

    assert_eq!(
        ids1, ids2,
        "GraphRAG must produce identical hit ordering on repeated calls"
    );
}

/// When a record appears as BOTH a vector hit AND a graph neighbor of another
/// seed, it must appear exactly once in hits (not duplicated).  Its provenance
/// must be "vector_and_graph" if it has a graph node.
#[tokio::test]
async fn graphrag_duplicate_candidate_appears_once() {
    let shared = make_shared();
    create_default_collection(&shared).await;

    // Two records, both near the query → both top-2 vector hits.
    let (_, wa) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(0.10), "collection": "default" }),
    )
    .await;
    let record_a = wa["id"].as_u64().unwrap();
    let (_, wb) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(0.11), "collection": "default" }),
    )
    .await;
    let record_b = wb["id"].as_u64().unwrap();

    // Create nodes for both.
    let (_, na) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": record_a, "collection": "default" }),
    )
    .await;
    let node_a = na["node_id"].as_u64().unwrap();
    let (_, nb) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": record_b, "collection": "default" }),
    )
    .await;
    let node_b = nb["node_id"].as_u64().unwrap();

    // Edge A → B so B is also reachable as a graph neighbor of A.
    let (st, _) = post(
        &shared,
        "/v1/graph/edge",
        serde_json::json!({ "from": node_a, "to": node_b, "kind": 0, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // k=2: both A and B are vector hits.  B is also a graph neighbor of A.
    let (st, out) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({ "query_vector": vec_n(0.10), "k": 2, "depth": 2, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let hits = out["hits"].as_array().unwrap();
    // B must appear exactly once — not once as a vector hit and once as a graph candidate.
    assert_eq!(hits.len(), 2, "A and B each appear exactly once");

    let hit_b = hits
        .iter()
        .find(|h| h["record_id"].as_u64() == Some(record_b))
        .expect("record B must be in hits");
    // B was a vector hit with a graph node → source="vector_and_graph", not "graph".
    assert_eq!(
        hit_b["source"].as_str(),
        Some("vector_and_graph"),
        "B's provenance is vector_and_graph (it was a vector hit, not a graph-only candidate)"
    );
    let _ = node_b; // suppress unused warning
}

// ── Phase 5.4: reranking, BFS budgets, final_k default ───────────────────────

/// `final_k` must default to `retrieval_k` when absent (Phase 5.4).
/// Requesting k=2 with no explicit final_k must return ≤2 hits even when
/// graph expansion discovers additional candidates.
#[tokio::test]
async fn graphrag_final_k_defaults_to_retrieval_k() {
    let shared = make_shared();
    create_default_collection(&shared).await;

    // Insert 3 records and chain them: 0→1→2.
    let mut node_ids: Vec<u64> = Vec::new();
    for i in 0..3u32 {
        let (_, w) = post(
            &shared,
            "/v1/records",
            serde_json::json!({ "values": vec_n(i as f32 * 0.1), "collection": "default" }),
        )
        .await;
        let rid = w["id"].as_u64().unwrap();
        let (_, n) = post(
            &shared,
            "/v1/graph/node",
            serde_json::json!({ "kind": 1, "record_id": rid, "collection": "default" }),
        )
        .await;
        node_ids.push(n["node_id"].as_u64().unwrap());
    }
    for i in 0..2 {
        post(
            &shared,
            "/v1/graph/edge",
            serde_json::json!({ "from": node_ids[i], "to": node_ids[i + 1], "kind": 0, "collection": "default" }),
        )
        .await;
    }

    // retrieval_k=2, no final_k → final_k defaults to 2.
    // Without the default, graph expansion at depth=3 would surface record 2 as
    // a graph-only candidate, returning 3 hits.
    let (st, out) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({
            "query_vector": vec_n(0.0),
            "retrieval_k": 2,
            "depth": 3,
            "collection": "default"
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let hits = out["hits"].as_array().unwrap();
    assert!(
        hits.len() <= 2,
        "absent final_k must default to retrieval_k=2; got {} hits",
        hits.len()
    );
}

/// `graph_score` must be present on all hits as a number in [0, 1].
/// - Seeds (dist=0):       graph_score = 1.0
/// - Graph-only (dist=1):  graph_score = 0.5
/// - Vector-only (no node): graph_score = 0.0
#[tokio::test]
async fn graphrag_graph_score_field_on_all_hit_types() {
    let shared = make_shared();
    create_default_collection(&shared).await;

    // Record A: near query + graph node (becomes seed at dist=0).
    let (_, wa) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(0.10), "collection": "default" }),
    )
    .await;
    let record_a = wa["id"].as_u64().unwrap();

    // Record B: far + graph node (graph-only at dist=1 from A).
    let (_, wb) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(100.0), "collection": "default" }),
    )
    .await;
    let record_b = wb["id"].as_u64().unwrap();

    // Record C: slightly far + NO graph node (pure vector hit).
    let (_, wc) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(0.5), "collection": "default" }),
    )
    .await;
    let record_c = wc["id"].as_u64().unwrap();

    let (_, na) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": record_a, "collection": "default" }),
    )
    .await;
    let node_a = na["node_id"].as_u64().unwrap();
    let (_, nb) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": record_b, "collection": "default" }),
    )
    .await;
    post(
        &shared,
        "/v1/graph/edge",
        serde_json::json!({ "from": node_a, "to": nb["node_id"].as_u64().unwrap(), "kind": 0, "collection": "default" }),
    )
    .await;

    // retrieval_k=2 gets A and C; B is graph-only. final_k=10 returns all.
    let (st, out) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({
            "query_vector": vec_n(0.10),
            "retrieval_k": 2,
            "depth": 2,
            "final_k": 10,
            "collection": "default"
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let hits = out["hits"].as_array().unwrap();

    for h in hits {
        let gs = h["graph_score"]
            .as_f64()
            .expect("graph_score must be numeric on every hit");
        assert!(
            (0.0..=1.0).contains(&gs),
            "graph_score must be in [0,1], got {gs}"
        );
        let fs = h["final_score"]
            .as_f64()
            .expect("final_score must be numeric on every hit");
        assert!(
            (0.0..=1.0).contains(&fs),
            "final_score must be in [0,1], got {fs}"
        );
    }

    // Seed A (dist=0): graph_score must be 1.0
    let hit_a = hits
        .iter()
        .find(|h| h["record_id"].as_u64() == Some(record_a))
        .unwrap();
    let gs_a = hit_a["graph_score"].as_f64().unwrap();
    assert!(
        (gs_a - 1.0).abs() < 1e-6,
        "seed graph_score should be 1.0, got {gs_a}"
    );

    // Vector-only C (no graph node): graph_score must be 0.0
    let hit_c = hits
        .iter()
        .find(|h| h["record_id"].as_u64() == Some(record_c))
        .unwrap();
    let gs_c = hit_c["graph_score"].as_f64().unwrap();
    assert!(
        (gs_c - 0.0).abs() < 1e-6,
        "no-graph vector hit graph_score should be 0.0, got {gs_c}"
    );

    // Graph-only B (dist=1): graph_score must be 0.5
    let hit_b = hits
        .iter()
        .find(|h| h["record_id"].as_u64() == Some(record_b));
    if let Some(h) = hit_b {
        let gs_b = h["graph_score"].as_f64().unwrap();
        assert!(
            (gs_b - 0.5).abs() < 1e-6,
            "hop-1 graph_score should be 0.5, got {gs_b}"
        );
    }

    let _ = (record_b, node_a); // suppress unused
}

/// With `graph_weight=1.0` (pure graph signal), a graph-only candidate at hop 1
/// must outrank a pure vector hit that has no graph node, because the graph-only
/// hit has graph_relevance=0.5 while the no-graph vector hit has graph_relevance=0.0.
#[tokio::test]
async fn graphrag_graph_only_outranks_no_graph_vector_with_high_graph_weight() {
    let shared = make_shared();
    create_default_collection(&shared).await;

    // Record A: near query + graph node (seed, dist=0).
    let (_, wa) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(0.10), "collection": "default" }),
    )
    .await;
    let record_a = wa["id"].as_u64().unwrap();

    // Record B: far + graph node (graph-only at dist=1 via A→B).
    let (_, wb) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(100.0), "collection": "default" }),
    )
    .await;
    let record_b = wb["id"].as_u64().unwrap();

    // Record C: mid-range + NO graph node (pure vector hit in retrieval_k=2).
    let (_, wc) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(0.5), "collection": "default" }),
    )
    .await;
    let record_c = wc["id"].as_u64().unwrap();

    let (_, na) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": record_a, "collection": "default" }),
    )
    .await;
    let node_a = na["node_id"].as_u64().unwrap();
    let (_, nb) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": record_b, "collection": "default" }),
    )
    .await;
    post(
        &shared,
        "/v1/graph/edge",
        serde_json::json!({ "from": node_a, "to": nb["node_id"].as_u64().unwrap(), "kind": 0, "collection": "default" }),
    )
    .await;

    // graph_weight=1.0 → β=1, α=0 → only graph signal matters.
    //   A (seed, dist=0):       final_score = 1.0 × 1.0 = 1.0  → first
    //   B (graph-only, dist=1): final_score = 1.0 × 0.5 = 0.5  → second
    //   C (no graph node):      final_score = 1.0 × 0.0 = 0.0  → last
    let (st, out) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({
            "query_vector": vec_n(0.10),
            "retrieval_k": 2,
            "depth": 2,
            "final_k": 10,
            "graph_weight": 1.0,
            "collection": "default"
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let hits = out["hits"].as_array().unwrap();

    let pos_b = hits
        .iter()
        .position(|h| h["record_id"].as_u64() == Some(record_b));
    let pos_c = hits
        .iter()
        .position(|h| h["record_id"].as_u64() == Some(record_c));

    if let (Some(pb), Some(pc)) = (pos_b, pos_c) {
        assert!(
            pb < pc,
            "with graph_weight=1.0, graph-only B (dist=1, final_score=0.5) \
             must outrank no-graph vector C (final_score=0.0); \
             B at pos {pb}, C at pos {pc}"
        );
    }
    let _ = (record_a, record_c, node_a); // suppress unused
}

/// `max_nodes` must halt BFS before visiting more nodes than the budget.
/// Chain A→B→C with max_nodes=1: the subgraph can contain at most 1 node (A the seed).
/// No graph-only candidates should be returned since B and C are not expanded.
#[tokio::test]
async fn graphrag_max_nodes_limits_bfs_expansion() {
    let shared = make_shared();
    create_default_collection(&shared).await;

    // Record A: near query (seed), B and C far (would be graph-only without budget).
    let (_, wa) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(0.10), "collection": "default" }),
    )
    .await;
    let record_a = wa["id"].as_u64().unwrap();
    let (_, wb) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(100.0), "collection": "default" }),
    )
    .await;
    let record_b = wb["id"].as_u64().unwrap();
    let (_, wc) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(200.0), "collection": "default" }),
    )
    .await;

    let (_, na) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": record_a, "collection": "default" }),
    )
    .await;
    let node_a = na["node_id"].as_u64().unwrap();
    let (_, nb) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": record_b, "collection": "default" }),
    )
    .await;
    let node_b = nb["node_id"].as_u64().unwrap();
    let (_, nc) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": wc["id"].as_u64().unwrap(), "collection": "default" }),
    )
    .await;
    let node_c = nc["node_id"].as_u64().unwrap();

    // Chain A→B→C
    post(
        &shared,
        "/v1/graph/edge",
        serde_json::json!({ "from": node_a, "to": node_b, "kind": 0, "collection": "default" }),
    )
    .await;
    post(
        &shared,
        "/v1/graph/edge",
        serde_json::json!({ "from": node_b, "to": node_c, "kind": 0, "collection": "default" }),
    )
    .await;

    // max_nodes=1: BFS halts before visiting B or C — only A (the seed) is in subgraph.
    let (st, out) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({
            "query_vector": vec_n(0.10),
            "k": 1,
            "depth": 3,
            "final_k": 10,
            "max_nodes": 1,
            "collection": "default"
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let subgraph_nodes = out["subgraph"]["nodes"].as_array().unwrap();
    assert!(
        subgraph_nodes.len() <= 1,
        "max_nodes=1 must limit subgraph to at most 1 node; got {}",
        subgraph_nodes.len()
    );

    let graph_only_count = out["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|h| h["source"].as_str() == Some("graph"))
        .count();
    assert_eq!(
        graph_only_count, 0,
        "no graph-only candidates when BFS is capped to 1 node (the seed itself)"
    );
    let _ = (record_a, node_a); // suppress unused
}

/// `max_edges` must stop edge emission once the budget is reached.
/// Chain A→B→C with max_edges=1: at most 1 edge in the subgraph.
/// C must not appear as a graph-only candidate since its path requires B→C.
#[tokio::test]
async fn graphrag_max_edges_limits_bfs_expansion() {
    let shared = make_shared();
    create_default_collection(&shared).await;

    let (_, wa) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(0.10), "collection": "default" }),
    )
    .await;
    let record_a = wa["id"].as_u64().unwrap();
    let (_, wb) = post(
        &shared,
        "/v1/records",
        serde_json::json!({ "values": vec_n(100.0), "collection": "default" }),
    )
    .await;
    let record_c_far = {
        let (_, wc) = post(
            &shared,
            "/v1/records",
            serde_json::json!({ "values": vec_n(200.0), "collection": "default" }),
        )
        .await;
        wc["id"].as_u64().unwrap()
    };

    let (_, na) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": record_a, "collection": "default" }),
    )
    .await;
    let node_a = na["node_id"].as_u64().unwrap();
    let (_, nb) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": wb["id"].as_u64().unwrap(), "collection": "default" }),
    )
    .await;
    let node_b = nb["node_id"].as_u64().unwrap();
    let (_, nc) = post(
        &shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "record_id": record_c_far, "collection": "default" }),
    )
    .await;
    let node_c = nc["node_id"].as_u64().unwrap();

    // Chain A→B→C
    post(
        &shared,
        "/v1/graph/edge",
        serde_json::json!({ "from": node_a, "to": node_b, "kind": 0, "collection": "default" }),
    )
    .await;
    post(
        &shared,
        "/v1/graph/edge",
        serde_json::json!({ "from": node_b, "to": node_c, "kind": 0, "collection": "default" }),
    )
    .await;

    // max_edges=1: only the A→B edge is emitted; B→C is cut.
    // C must not appear as a graph-only candidate.
    let (st, out) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({
            "query_vector": vec_n(0.10),
            "k": 1,
            "depth": 3,
            "final_k": 10,
            "max_edges": 1,
            "collection": "default"
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let edges = out["subgraph"]["edges"].as_array().unwrap();
    assert!(
        edges.len() <= 1,
        "max_edges=1 must limit subgraph edges; got {}",
        edges.len()
    );

    let c_in_hits = out["hits"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["record_id"].as_u64() == Some(record_c_far));
    assert!(
        !c_in_hits,
        "C must not appear in hits when max_edges=1 prevents B→C traversal"
    );
    let _ = (record_a, node_a, node_b, node_c); // suppress unused
}
