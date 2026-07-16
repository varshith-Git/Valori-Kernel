// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Behavioral parity tests: metadata_filter, rerank, and query_text on
//! POST /v1/memory/search_vector must produce identical filtering semantics
//! in both the standalone and cluster execution paths.
//!
//! Route parity (same URL registered) is already enforced by route_parity.rs.
//! This file enforces *behavioral* parity: the same request body must produce
//! the same filtered/ranked result set regardless of which router handles it.

use std::time::Duration;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;
use valori_consensus::types::ValoriNode;
use valori_node::cluster::{bootstrap_cluster, ClusterConfig};
use valori_node::EngineFromNodeConfig;
use valori_node::cluster_server::build_cluster_router;
use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use valori_node::server::build_router;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Standalone router: in-memory engine, no WAL, no auth.
fn standalone_router() -> axum::Router {
    let mut cfg = NodeConfig::default();
    cfg.dim = 4;
    cfg.max_records = 500;
    cfg.max_nodes = 200;
    cfg.max_edges = 200;
    let engine = Engine::new(&cfg);
    let state = std::sync::Arc::new(tokio::sync::RwLock::new(engine));
    build_router(state, None, None)
}

/// Single-node cluster router bootstrapped as sole leader.
async fn cluster_router() -> axum::Router {
    let cfg = ClusterConfig {
        node_id: 1,
        raft_bind: "127.0.0.1:0".into(),
        members: [(1, ValoriNode {
            api_addr: "127.0.0.1:0".into(),
            raft_addr: String::new(),
        })].into_iter().collect(),
        init: true,
        raft_log_path: None,
        tls: None,
        shard_count: 1,
    };
    let handle = bootstrap_cluster(&cfg, None, None, 0).await.unwrap();
    handle.raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader == Some(1), "self-elected")
        .await
        .unwrap();
    let router = build_cluster_router(&handle, None);
    // Keep handle alive for the duration of the test via a leak.
    // Tests are short-lived processes so this is acceptable.
    std::mem::forget(handle);
    router
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
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(json!(null));
    (status, json)
}

/// Insert a record with metadata via /v1/memory/upsert_vector and return its memory_id.
/// Note: does NOT populate the BM25 reranker — use insert_with_text for rerank tests.
async fn upsert(router: axum::Router, vec: [f32; 4], metadata: Option<Value>) -> Value {
    let mut body = json!({ "vector": vec });
    if let Some(m) = metadata { body["metadata"] = m; }
    let (status, resp) = post(router, "/v1/memory/upsert_vector", body).await;
    assert_eq!(status, StatusCode::OK, "upsert failed: {resp}");
    resp
}

/// Insert via /v1/vectors/batch-insert with text so the BM25 reranker
/// (standalone) and the cluster text_corpus (cluster) are both populated.
/// Returns the record_id.
async fn insert_with_text(router: axum::Router, vec: [f32; 4], text: &str) -> u64 {
    let body = json!({ "batch": [vec], "texts": [text] });
    let (status, resp) = post(router, "/v1/vectors/batch-insert", body).await;
    assert_eq!(status, StatusCode::OK, "insert_with_text failed: {resp}");
    resp["ids"].as_array().unwrap()[0].as_u64().unwrap()
}

async fn memory_search(router: axum::Router, query: [f32; 4], extra: Value) -> Value {
    let mut body = json!({ "query_vector": query, "k": 10 });
    if let Value::Object(map) = extra {
        for (k, v) in map { body[k] = v; }
    }
    let (status, resp) = post(router, "/v1/memory/search_vector", body).await;
    assert_eq!(status, StatusCode::OK, "search failed: {resp}");
    resp
}

// ── Parity assertion ──────────────────────────────────────────────────────────

/// Extract record_ids from search results for easy comparison.
fn record_ids(resp: &Value) -> Vec<u64> {
    resp["results"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|r| r["record_id"].as_u64().unwrap_or(0))
        .collect()
}

// ── Test: metadata_filter excludes non-matching records identically ───────────

async fn seed_metadata_scenario(router: axum::Router) -> (u64, u64) {
    // Two records: one tagged author=Alice, one author=Bob. Both near [1,0,0,0].
    let a = upsert(router.clone(), [1.0, 0.1, 0.0, 0.0],
        Some(json!({"author": "Alice"}))).await;
    let b = upsert(router.clone(), [1.0, 0.2, 0.0, 0.0],
        Some(json!({"author": "Bob"}))).await;
    (a["record_id"].as_u64().unwrap(), b["record_id"].as_u64().unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_metadata_filter_excludes_non_matching() {
    let router = standalone_router();
    let (alice_id, _bob_id) = seed_metadata_scenario(router.clone()).await;

    let resp = memory_search(router, [1.0, 0.0, 0.0, 0.0],
        json!({"metadata_filter": {"author": "Alice"}})).await;
    let ids = record_ids(&resp);
    assert!(!ids.is_empty(), "expected at least one result");
    assert!(ids.contains(&alice_id), "Alice should appear");
    assert!(ids.iter().all(|&id| id == alice_id), "only Alice should appear, got {ids:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cluster_metadata_filter_excludes_non_matching() {
    let router = cluster_router().await;
    let (alice_id, _bob_id) = seed_metadata_scenario(router.clone()).await;

    let resp = memory_search(router, [1.0, 0.0, 0.0, 0.0],
        json!({"metadata_filter": {"author": "Alice"}})).await;
    let ids = record_ids(&resp);
    assert!(!ids.is_empty(), "expected at least one result");
    assert!(ids.contains(&alice_id), "Alice should appear");
    assert!(ids.iter().all(|&id| id == alice_id), "only Alice should appear, got {ids:?}");
}

// ── Test: empty metadata_filter returns all records ───────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_no_filter_returns_all() {
    let router = standalone_router();
    let (alice_id, bob_id) = seed_metadata_scenario(router.clone()).await;

    let resp = memory_search(router, [1.0, 0.0, 0.0, 0.0], json!({})).await;
    let ids = record_ids(&resp);
    assert!(ids.contains(&alice_id) && ids.contains(&bob_id),
        "both records should appear without filter, got {ids:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cluster_no_filter_returns_all() {
    let router = cluster_router().await;
    let (alice_id, bob_id) = seed_metadata_scenario(router.clone()).await;

    let resp = memory_search(router, [1.0, 0.0, 0.0, 0.0], json!({})).await;
    let ids = record_ids(&resp);
    assert!(ids.contains(&alice_id) && ids.contains(&bob_id),
        "both records should appear without filter, got {ids:?}");
}

// ── Test: rerank with query_text changes ordering toward lexical match ─────────

async fn seed_rerank_scenario(router: axum::Router) -> (u64, u64) {
    // Both records are inserted at the EXACT same vector as the query [1,0,0,0].
    // When all L2 distances are equal, the reranker's normalise() returns all-zeros
    // for the vector component (range = 0), which after the 1-x flip becomes
    // all-ones — so every candidate gets equal vector weight and BM25 alone
    // determines the winner. This makes the ordering deterministic and stable.
    let quantum = insert_with_text(router.clone(), [1.0, 0.0, 0.0, 0.0], "quantum mechanics theory").await;
    let fruit   = insert_with_text(router.clone(), [1.0, 0.0, 0.0, 0.0], "apple fruit salad").await;
    (fruit, quantum)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_rerank_promotes_lexical_match() {
    let router = standalone_router();
    let (fruit_id, quantum_id) = seed_rerank_scenario(router.clone()).await;

    // Without rerank: quantum is geometrically closer → should rank first
    let resp_raw = memory_search(router.clone(), [1.0, 0.0, 0.0, 0.0],
        json!({"rerank": false})).await;
    let ids_raw = record_ids(&resp_raw);
    assert_eq!(ids_raw.first(), Some(&quantum_id),
        "without rerank, geometric winner should be first");

    // With rerank + query_text="fruit": fruit record should be promoted
    let resp_reranked = memory_search(router, [1.0, 0.0, 0.0, 0.0],
        json!({"rerank": true, "query_text": "fruit"})).await;
    let ids_reranked = record_ids(&resp_reranked);
    assert_eq!(ids_reranked.first(), Some(&fruit_id),
        "rerank should promote the fruit record for query 'fruit', got {ids_reranked:?}");
    let _ = quantum_id; // still present, just ranked lower
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cluster_rerank_promotes_lexical_match() {
    let router = cluster_router().await;
    let (fruit_id, quantum_id) = seed_rerank_scenario(router.clone()).await;

    // Without rerank: quantum is geometrically closer
    let resp_raw = memory_search(router.clone(), [1.0, 0.0, 0.0, 0.0],
        json!({"rerank": false})).await;
    let ids_raw = record_ids(&resp_raw);
    assert_eq!(ids_raw.first(), Some(&quantum_id),
        "without rerank, geometric winner should be first");

    // With rerank + query_text="fruit": fruit should be promoted
    let resp_reranked = memory_search(router, [1.0, 0.0, 0.0, 0.0],
        json!({"rerank": true, "query_text": "fruit"})).await;
    let ids_reranked = record_ids(&resp_reranked);
    assert_eq!(ids_reranked.first(), Some(&fruit_id),
        "cluster rerank should promote the fruit record for query 'fruit', got {ids_reranked:?}");
}

// ── Test: rerank=false with query_text present → query_text ignored ───────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_rerank_false_ignores_query_text() {
    let router = standalone_router();
    let (fruit_id, quantum_id) = seed_rerank_scenario(router.clone()).await;

    let resp = memory_search(router, [1.0, 0.0, 0.0, 0.0],
        json!({"rerank": false, "query_text": "fruit"})).await;
    let ids = record_ids(&resp);
    // rerank=false → pure L2 → geometric winner (quantum) should be first
    assert_eq!(ids.first(), Some(&quantum_id),
        "rerank=false must ignore query_text, got {ids:?}");
    let _ = fruit_id;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cluster_rerank_false_ignores_query_text() {
    let router = cluster_router().await;
    let (fruit_id, quantum_id) = seed_rerank_scenario(router.clone()).await;

    let resp = memory_search(router, [1.0, 0.0, 0.0, 0.0],
        json!({"rerank": false, "query_text": "fruit"})).await;
    let ids = record_ids(&resp);
    assert_eq!(ids.first(), Some(&quantum_id),
        "cluster: rerank=false must ignore query_text, got {ids:?}");
    let _ = fruit_id;
}

// ── Test: metadata_filter + rerank are not mutually exclusive ─────────────────
// Previously both paths had `if use_rerank && mf.is_none()`, which silently
// skipped reranking whenever a filter was active. This verifies the fix.

async fn seed_filter_and_rerank(router: axum::Router) -> (u64, u64) {
    // Two Alice records at the same vector: one matches "fruit", one does not.
    // Both have metadata author=Alice so the filter passes both.
    // Reranking should still promote the fruit record.
    let quantum = insert_with_text_and_meta(router.clone(), [1.0, 0.0, 0.0, 0.0],
        "quantum mechanics theory", json!({"author": "Alice"})).await;
    let fruit   = insert_with_text_and_meta(router.clone(), [1.0, 0.0, 0.0, 0.0],
        "apple fruit salad", json!({"author": "Alice"})).await;
    (fruit, quantum)
}

/// Insert via upsert_vector with text (for BM25) and metadata (for filter).
/// Since upsert_vector doesn't populate the reranker directly, we combine
/// batch-insert (for text corpus) + set_meta (for metadata).
async fn insert_with_text_and_meta(router: axum::Router, vec: [f32; 4], text: &str, meta: Value) -> u64 {
    // batch-insert registers the text in the reranker/corpus
    let record_id = insert_with_text(router.clone(), vec, text).await;
    // set_meta attaches the metadata so the filter can see it
    let body = json!({ "target_id": format!("rec:{record_id}"), "metadata": meta });
    let (status, _) = post(router, "/v1/memory/meta/set", body).await;
    assert_eq!(status, StatusCode::OK, "meta set failed for rec:{record_id}");
    record_id
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_filter_plus_rerank_both_apply() {
    let router = standalone_router();
    // Bob record at same vector — should be excluded by filter
    insert_with_text_and_meta(router.clone(), [1.0, 0.0, 0.0, 0.0],
        "apple fruit dessert", json!({"author": "Bob"})).await;
    let (fruit_id, quantum_id) = seed_filter_and_rerank(router.clone()).await;

    let resp = memory_search(router, [1.0, 0.0, 0.0, 0.0],
        json!({"metadata_filter": {"author": "Alice"}, "rerank": true, "query_text": "fruit"})).await;
    let ids = record_ids(&resp);

    // Filter: only Alice records appear (Bob excluded)
    assert!(ids.iter().all(|id| *id == fruit_id || *id == quantum_id),
        "Bob should be filtered out, got {ids:?}");
    // Rerank: fruit ranks above quantum
    assert_eq!(ids.first(), Some(&fruit_id),
        "rerank should promote fruit even when filter is active, got {ids:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cluster_filter_plus_rerank_both_apply() {
    let router = cluster_router().await;
    insert_with_text_and_meta(router.clone(), [1.0, 0.0, 0.0, 0.0],
        "apple fruit dessert", json!({"author": "Bob"})).await;
    let (fruit_id, quantum_id) = seed_filter_and_rerank(router.clone()).await;

    let resp = memory_search(router, [1.0, 0.0, 0.0, 0.0],
        json!({"metadata_filter": {"author": "Alice"}, "rerank": true, "query_text": "fruit"})).await;
    let ids = record_ids(&resp);

    assert!(ids.iter().all(|id| *id == fruit_id || *id == quantum_id),
        "Bob should be filtered out, got {ids:?}");
    assert_eq!(ids.first(), Some(&fruit_id),
        "cluster: rerank should promote fruit even when filter is active, got {ids:?}");
}

// ── Test: k is respected after filtering ─────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_filter_respects_k() {
    let router = standalone_router();
    // Insert 5 Alice records and 5 Bob records.
    for _ in 0..5 {
        upsert(router.clone(), [1.0, 0.0, 0.0, 0.0], Some(json!({"author": "Alice"}))).await;
        upsert(router.clone(), [1.0, 0.1, 0.0, 0.0], Some(json!({"author": "Bob"}))).await;
    }
    let mut body = json!({ "query_vector": [1.0, 0.0, 0.0, 0.0], "k": 3,
                            "metadata_filter": {"author": "Alice"} });
    let (status, resp) = post(router, "/v1/memory/search_vector", body.take()).await;
    assert_eq!(status, StatusCode::OK);
    let ids = record_ids(&resp);
    assert!(ids.len() <= 3, "k=3 must cap results, got {} items", ids.len());
    assert!(!ids.is_empty(), "should have at least one Alice result");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cluster_filter_respects_k() {
    let router = cluster_router().await;
    for _ in 0..5 {
        upsert(router.clone(), [1.0, 0.0, 0.0, 0.0], Some(json!({"author": "Alice"}))).await;
        upsert(router.clone(), [1.0, 0.1, 0.0, 0.0], Some(json!({"author": "Bob"}))).await;
    }
    let mut body = json!({ "query_vector": [1.0, 0.0, 0.0, 0.0], "k": 3,
                            "metadata_filter": {"author": "Alice"} });
    let (status, resp) = post(router, "/v1/memory/search_vector", body.take()).await;
    assert_eq!(status, StatusCode::OK);
    let ids = record_ids(&resp);
    assert!(ids.len() <= 3, "cluster k=3 must cap results, got {} items", ids.len());
    assert!(!ids.is_empty(), "should have at least one Alice result");
}
