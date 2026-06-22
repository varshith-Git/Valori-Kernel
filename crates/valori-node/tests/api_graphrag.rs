// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 3.15 — native GraphRAG endpoint (`POST /v1/graphrag`).
//!
//! Proves the one-call composition: vector KNN → record→node resolution →
//! subgraph BFS, all from a single read against one consistent kernel snapshot.

use valori_node::config::{IndexKind, NodeConfig};
use valori_node::engine::Engine;
use valori_node::server::build_router;
use axum::{body::Body, http::{Request, StatusCode}};
use tower::ServiceExt;
use std::sync::Arc;
use tokio::sync::RwLock;

const DIM: usize = 4;

fn make_shared() -> Arc<RwLock<Engine>> {
    let mut cfg = NodeConfig::default();
    cfg.dim = DIM;
    cfg.max_records = 100;
    cfg.max_nodes = 64;
    cfg.max_edges = 64;
    cfg.index_kind = IndexKind::BruteForce;
    cfg.event_log_path = None;
    cfg.wal_path = None;
    Arc::new(RwLock::new(Engine::new(&cfg)))
}

async fn post(shared: &Arc<RwLock<Engine>>, path: &str, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
    let app = build_router(shared.clone(), None, None);
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

fn vec_n(seed: f32) -> Vec<f32> {
    (0..DIM).map(|i| seed + i as f32 * 0.01).collect()
}

#[tokio::test]
async fn graphrag_returns_hits_and_connected_subgraph() {
    let shared = make_shared();

    // Write a memory → creates a Document node, a Chunk node, and a doc→chunk edge.
    let (st, w) = post(&shared, "/v1/memory/upsert_vector",
        serde_json::json!({ "vector": vec_n(0.10) })).await;
    assert_eq!(st, StatusCode::OK);
    let chunk = w["chunk_node_id"].as_u64().unwrap();
    let doc = w["document_node_id"].as_u64().unwrap();

    // Add an edge OUT of the chunk so the subgraph around the hit is non-trivial.
    let (st, _) = post(&shared, "/graph/edge",
        serde_json::json!({ "from": chunk, "to": doc, "kind": 0 })).await;
    assert_eq!(st, StatusCode::OK);

    // Two more memories so KNN has alternatives.
    post(&shared, "/v1/memory/upsert_vector", serde_json::json!({ "vector": vec_n(0.5) })).await;
    post(&shared, "/v1/memory/upsert_vector", serde_json::json!({ "vector": vec_n(0.9) })).await;

    // One GraphRAG call.
    let (st, out) = post(&shared, "/v1/graphrag",
        serde_json::json!({ "query_vector": vec_n(0.10), "k": 3, "depth": 2 })).await;
    assert_eq!(st, StatusCode::OK);

    // Hits came back, the nearest being our seed memory.
    let hits = out["hits"].as_array().unwrap();
    assert!(!hits.is_empty(), "expected hits");
    assert_eq!(hits[0]["node_id"].as_u64(), Some(chunk), "nearest hit maps to its chunk node");

    // The subgraph expanded from the seed and includes the chunk→doc edge.
    let seeds = out["seed_nodes"].as_array().unwrap();
    assert!(seeds.iter().any(|s| s.as_u64() == Some(chunk)), "seed nodes should include the chunk");
    let nodes = out["subgraph"]["nodes"].as_array().unwrap();
    let edges = out["subgraph"]["edges"].as_array().unwrap();
    assert!(nodes.iter().any(|n| n["id"].as_u64() == Some(chunk)));
    assert!(nodes.iter().any(|n| n["id"].as_u64() == Some(doc)), "expanded to the doc node one hop out");
    assert!(edges.iter().any(|e|
        e["from"].as_u64() == Some(chunk) && e["to"].as_u64() == Some(doc)),
        "the chunk→doc edge must be present");
}

#[tokio::test]
async fn graphrag_on_empty_store_is_empty_not_error() {
    let shared = make_shared();
    let (st, out) = post(&shared, "/v1/graphrag",
        serde_json::json!({ "query_vector": vec_n(0.1), "k": 5 })).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(out["hits"].as_array().unwrap().len(), 0);
    assert_eq!(out["subgraph"]["nodes"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn graphrag_depth_zero_returns_seeds_without_edges() {
    let shared = make_shared();
    let (_, w) = post(&shared, "/v1/memory/upsert_vector",
        serde_json::json!({ "vector": vec_n(0.2) })).await;
    let chunk = w["chunk_node_id"].as_u64().unwrap();
    let doc = w["document_node_id"].as_u64().unwrap();
    post(&shared, "/graph/edge", serde_json::json!({ "from": chunk, "to": doc, "kind": 0 })).await;

    let (st, out) = post(&shared, "/v1/graphrag",
        serde_json::json!({ "query_vector": vec_n(0.2), "k": 1, "depth": 0 })).await;
    assert_eq!(st, StatusCode::OK);
    // depth 0 → the seed node itself, no edge traversal.
    assert!(out["subgraph"]["nodes"].as_array().unwrap().iter()
        .any(|n| n["id"].as_u64() == Some(chunk)));
    assert_eq!(out["subgraph"]["edges"].as_array().unwrap().len(), 0);
}
