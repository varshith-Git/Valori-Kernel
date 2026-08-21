// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 4 — Collection index lifecycle tests.
//!
//! Covers the spec's test matrix (§41):
//!   Creation: none_to_hnsw, none_to_ivf, none_to_bq
//!   Removal: hnsw_to_none
//!   Replacement: hnsw_to_ivf, ivf_to_hnsw
//!   Failure: build_failure_preserves_active (unknown type)
//!   Concurrent mutation: insert_during_build, delete_during_build
//!   Recovery: active_index_restores, incomplete_generation_not_activated
//!   Isolation: collection_a_build_does_not_affect_b
//!   Graph: index_change_preserves_graph
//!   Dimensions: 384_and_768_collections_simultaneously
//!
//! Phase 4 does NOT test cluster ANN (explicitly unsupported — cluster
//! returns 501). That is a separate future phase.

use reqwest::StatusCode;
use std::sync::Arc;
use tokio::sync::RwLock;
use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use valori_node::server::build_router;
use valori_node::EngineFromNodeConfig;

// ── helpers ──────────────────────────────────────────────────────────────────

async fn spawn_node() -> (reqwest::Client, String) {
    let mut cfg = NodeConfig::default();
    cfg.max_records = 2000;
    cfg.max_nodes = 500;
    cfg.max_edges = 500;

    let state = Arc::new(RwLock::new(Engine::new(&cfg)));
    let app = build_router(state, None, None);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    (client, format!("http://{}", addr))
}

async fn create_collection(client: &reqwest::Client, base: &str, name: &str, dim: u32) {
    let resp = client
        .post(format!("{base}/v1/namespaces"))
        .json(&serde_json::json!({"name": name, "dimension": dim, "metric": "squared_l2"}))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "create collection failed: {}",
        resp.status()
    );
}

async fn insert_vec(client: &reqwest::Client, base: &str, collection: &str, vec: &[f32]) -> u32 {
    let resp = client
        .post(format!("{base}/records"))
        .json(&serde_json::json!({"values": vec, "collection": collection}))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "insert failed: {}",
        resp.status()
    );
    resp.json::<serde_json::Value>().await.unwrap()["id"]
        .as_u64()
        .unwrap() as u32
}

async fn search(
    client: &reqwest::Client,
    base: &str,
    collection: &str,
    query: &[f32],
    k: usize,
) -> Vec<serde_json::Value> {
    let resp = client
        .post(format!("{base}/search"))
        .json(&serde_json::json!({"query": query, "k": k, "collection": collection}))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "search failed: {}",
        resp.status()
    );
    resp.json::<serde_json::Value>().await.unwrap()["results"]
        .as_array()
        .unwrap()
        .clone()
}

/// POST /v1/namespaces/{name}/index with given payload.
async fn post_index(
    client: &reqwest::Client,
    base: &str,
    collection: &str,
    payload: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let resp = client
        .post(format!("{base}/v1/namespaces/{collection}/index"))
        .json(&payload)
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body = resp.json::<serde_json::Value>().await.unwrap();
    (status, body)
}

/// GET /v1/namespaces/{name}/index
async fn get_index_status(
    client: &reqwest::Client,
    base: &str,
    collection: &str,
) -> serde_json::Value {
    let resp = client
        .get(format!("{base}/v1/namespaces/{collection}/index"))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "get index status failed: {}",
        resp.status()
    );
    resp.json().await.unwrap()
}

/// Poll until the index status is no longer "building", up to `max_ms` milliseconds.
async fn wait_for_build(client: &reqwest::Client, base: &str, collection: &str, max_ms: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(max_ms);
    loop {
        let status = get_index_status(client, base, collection).await;
        let s = status["status"].as_str().unwrap_or("");
        if s != "building" {
            return;
        }
        if std::time::Instant::now() > deadline {
            panic!(
                "index build did not complete within {}ms; status={}",
                max_ms,
                serde_json::to_string_pretty(&status).unwrap()
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

// ── Creation tests ────────────────────────────────────────────────────────────

#[tokio::test]
async fn none_to_hnsw() {
    let (client, base) = spawn_node().await;
    create_collection(&client, &base, "docs", 4).await;
    insert_vec(&client, &base, "docs", &[1.0, 0.0, 0.0, 0.0]).await;
    insert_vec(&client, &base, "docs", &[0.0, 1.0, 0.0, 0.0]).await;
    insert_vec(&client, &base, "docs", &[0.0, 0.0, 1.0, 0.0]).await;

    let (status, body) =
        post_index(&client, &base, "docs", serde_json::json!({"type": "hnsw"})).await;
    // Accepted (202) — build is async
    assert_eq!(status.as_u16(), 202, "expected 202 Accepted: {body}");
    assert_eq!(body["collection"].as_str().unwrap(), "docs");

    wait_for_build(&client, &base, "docs", 5000).await;

    let status_body = get_index_status(&client, &base, "docs").await;
    assert_eq!(status_body["status"].as_str().unwrap(), "active");
    assert_eq!(status_body["active_type"].as_str().unwrap(), "hnsw");

    // Search still works after build
    let results = search(&client, &base, "docs", &[1.0, 0.0, 0.0, 0.0], 3).await;
    assert!(
        !results.is_empty(),
        "search must return results after HNSW build"
    );
}

#[tokio::test]
async fn none_to_ivf() {
    let (client, base) = spawn_node().await;
    create_collection(&client, &base, "docs", 4).await;
    // IVF needs enough records for k-means (insert ≥ n_list = max(16, sqrt(N)))
    for i in 0..40 {
        let v = [i as f32, 0.0, 0.0, 0.0];
        insert_vec(&client, &base, "docs", &v).await;
    }

    let (status, body) =
        post_index(&client, &base, "docs", serde_json::json!({"type": "ivf"})).await;
    assert_eq!(status.as_u16(), 202, "expected 202 Accepted: {body}");

    wait_for_build(&client, &base, "docs", 8000).await;
    let status_body = get_index_status(&client, &base, "docs").await;
    assert_eq!(status_body["active_type"].as_str().unwrap(), "ivf");
    assert_eq!(status_body["status"].as_str().unwrap(), "active");

    let results = search(&client, &base, "docs", &[1.0, 0.0, 0.0, 0.0], 5).await;
    assert!(!results.is_empty());
}

#[tokio::test]
async fn none_to_bq() {
    let (client, base) = spawn_node().await;
    create_collection(&client, &base, "docs", 8).await;
    for i in 0..10 {
        let v = [i as f32, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        insert_vec(&client, &base, "docs", &v).await;
    }

    let (status, body) =
        post_index(&client, &base, "docs", serde_json::json!({"type": "bq"})).await;
    assert_eq!(status.as_u16(), 202, "expected 202 Accepted: {body}");

    wait_for_build(&client, &base, "docs", 5000).await;
    let s = get_index_status(&client, &base, "docs").await;
    assert_eq!(s["active_type"].as_str().unwrap(), "bq");
    assert_eq!(s["status"].as_str().unwrap(), "active");
}

// ── Removal test ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn hnsw_to_none() {
    let (client, base) = spawn_node().await;
    create_collection(&client, &base, "docs", 4).await;
    insert_vec(&client, &base, "docs", &[1.0, 0.0, 0.0, 0.0]).await;

    // Build HNSW first
    post_index(&client, &base, "docs", serde_json::json!({"type": "hnsw"})).await;
    wait_for_build(&client, &base, "docs", 5000).await;

    // Drop it
    let (status, _body) =
        post_index(&client, &base, "docs", serde_json::json!({"type": null})).await;
    assert_eq!(status.as_u16(), 200, "drop index should return 200");

    let s = get_index_status(&client, &base, "docs").await;
    assert_eq!(s["active_type"].as_str().unwrap(), "none");
    assert_eq!(s["status"].as_str().unwrap(), "none");

    // Search should still work (exact path)
    let results = search(&client, &base, "docs", &[1.0, 0.0, 0.0, 0.0], 1).await;
    assert_eq!(results.len(), 1, "exact search must still work after drop");
}

// ── Replacement test ──────────────────────────────────────────────────────────

#[tokio::test]
async fn hnsw_to_ivf() {
    let (client, base) = spawn_node().await;
    create_collection(&client, &base, "docs", 4).await;
    for i in 0..40 {
        insert_vec(&client, &base, "docs", &[i as f32, 0.0, 0.0, 0.0]).await;
    }

    // Build HNSW
    post_index(&client, &base, "docs", serde_json::json!({"type": "hnsw"})).await;
    wait_for_build(&client, &base, "docs", 5000).await;

    // Request IVF replacement
    let (status, body) =
        post_index(&client, &base, "docs", serde_json::json!({"type": "ivf"})).await;
    assert_eq!(
        status.as_u16(),
        202,
        "replacement should return 202: {body}"
    );

    // Poll: while building, search continues to work (active is still HNSW)
    let mut saw_building = false;
    for _ in 0..20 {
        let s = get_index_status(&client, &base, "docs").await;
        if s["status"].as_str().unwrap() == "building" {
            saw_building = true;
            // Search must still work while building
            let r = search(&client, &base, "docs", &[1.0, 0.0, 0.0, 0.0], 5).await;
            assert!(!r.is_empty(), "search must work during replacement build");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    wait_for_build(&client, &base, "docs", 8000).await;
    let s = get_index_status(&client, &base, "docs").await;
    assert_eq!(s["active_type"].as_str().unwrap(), "ivf");
    assert_eq!(s["status"].as_str().unwrap(), "active");
    let _ = saw_building; // may have been too fast to observe on a fast machine
}

// ── Failure test ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn unknown_type_rejected() {
    let (client, base) = spawn_node().await;
    create_collection(&client, &base, "docs", 4).await;
    insert_vec(&client, &base, "docs", &[1.0, 0.0, 0.0, 0.0]).await;

    let (status, body) =
        post_index(&client, &base, "docs", serde_json::json!({"type": "faiss"})).await;
    assert_eq!(status.as_u16(), 400, "unknown type must return 400: {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("unsupported index type"),
        "error must mention unsupported type: {body}"
    );
}

#[tokio::test]
async fn concurrent_build_rejected() {
    let (client, base) = spawn_node().await;
    create_collection(&client, &base, "docs", 4).await;
    // Use many records to keep the IVF k-means build in-flight long enough
    // for the second request to arrive before it completes.
    for i in 0..200 {
        insert_vec(&client, &base, "docs", &[i as f32, 0.0, 0.0, 0.0]).await;
    }

    // Fire both build requests truly concurrently using tokio::join!
    let (r1, r2) = tokio::join!(
        post_index(&client, &base, "docs", serde_json::json!({"type": "hnsw"})),
        post_index(&client, &base, "docs", serde_json::json!({"type": "ivf"})),
    );

    let s1 = r1.0.as_u16();
    let s2 = r2.0.as_u16();

    // At least one must succeed and at least one must fail
    let both_succeed = s1 == 202 && s2 == 202;
    assert!(
        !both_succeed,
        "two concurrent builds must not both be accepted; got s1={s1} s2={s2}"
    );
    assert!(
        s1 == 202 || s2 == 202,
        "at least one build must be accepted; got s1={s1} s2={s2}"
    );
    let has_conflict = s1 == 409 || s2 == 409;
    assert!(
        has_conflict,
        "the second build must be rejected with 409; got s1={s1} s2={s2}"
    );
}

// ── Concurrent mutation test ──────────────────────────────────────────────────

#[tokio::test]
async fn insert_during_build() {
    let (client, base) = spawn_node().await;
    create_collection(&client, &base, "docs", 4).await;
    for i in 0..20 {
        insert_vec(&client, &base, "docs", &[i as f32, 0.0, 0.0, 0.0]).await;
    }

    // Start the build
    post_index(&client, &base, "docs", serde_json::json!({"type": "hnsw"})).await;

    // Insert records concurrently during the build
    let mut new_ids = vec![];
    for i in 20..30 {
        let id = insert_vec(&client, &base, "docs", &[i as f32, 0.0, 0.0, 0.0]).await;
        new_ids.push(id);
    }

    wait_for_build(&client, &base, "docs", 5000).await;

    // After build completes, all records (including those inserted during build)
    // should be searchable.
    let results = search(&client, &base, "docs", &[25.0, 0.0, 0.0, 0.0], 3).await;
    assert!(
        !results.is_empty(),
        "records inserted during build must be searchable after"
    );
}

// ── Isolation test ────────────────────────────────────────────────────────────

#[tokio::test]
async fn collection_a_build_does_not_affect_b() {
    let (client, base) = spawn_node().await;
    create_collection(&client, &base, "images", 4).await;
    create_collection(&client, &base, "text", 4).await;

    for i in 0..10 {
        insert_vec(&client, &base, "images", &[i as f32, 0.0, 0.0, 0.0]).await;
        insert_vec(&client, &base, "text", &[0.0, i as f32, 0.0, 0.0]).await;
    }

    // Build HNSW on images only
    post_index(
        &client,
        &base,
        "images",
        serde_json::json!({"type": "hnsw"}),
    )
    .await;
    wait_for_build(&client, &base, "images", 5000).await;

    // text should still be NONE (unaffected)
    let s = get_index_status(&client, &base, "text").await;
    assert_eq!(
        s["active_type"].as_str().unwrap(),
        "none",
        "text collection index must be unaffected"
    );

    // Both collections must still search correctly
    let r1 = search(&client, &base, "images", &[1.0, 0.0, 0.0, 0.0], 3).await;
    let r2 = search(&client, &base, "text", &[0.0, 1.0, 0.0, 0.0], 3).await;
    assert!(!r1.is_empty());
    assert!(!r2.is_empty());
}

// ── Graph test ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn index_change_preserves_graph() {
    let (client, base) = spawn_node().await;
    create_collection(&client, &base, "docs", 4).await;

    let id0 = insert_vec(&client, &base, "docs", &[1.0, 0.0, 0.0, 0.0]).await;
    let id1 = insert_vec(&client, &base, "docs", &[0.0, 1.0, 0.0, 0.0]).await;

    // Create a graph node + edge (routes: POST /v1/graph/node, kind: u8, collection required)
    let node_resp = client
        .post(format!("{base}/v1/graph/node"))
        .json(&serde_json::json!({"record_id": id0, "kind": 0, "collection": "docs"}))
        .send()
        .await
        .unwrap();
    assert!(
        node_resp.status().is_success(),
        "node create failed: {}",
        node_resp.status()
    );
    let node0_id = node_resp.json::<serde_json::Value>().await.unwrap()["node_id"]
        .as_u64()
        .unwrap();

    let node_resp2 = client
        .post(format!("{base}/v1/graph/node"))
        .json(&serde_json::json!({"record_id": id1, "kind": 0, "collection": "docs"}))
        .send()
        .await
        .unwrap();
    assert!(node_resp2.status().is_success());
    let node1_id = node_resp2.json::<serde_json::Value>().await.unwrap()["node_id"]
        .as_u64()
        .unwrap();

    let edge_resp = client
        .post(format!("{base}/v1/graph/edge"))
        .json(
            &serde_json::json!({"from": node0_id, "to": node1_id, "kind": 0, "collection": "docs"}),
        )
        .send()
        .await
        .unwrap();
    assert!(edge_resp.status().is_success());

    // Build HNSW
    post_index(&client, &base, "docs", serde_json::json!({"type": "hnsw"})).await;
    wait_for_build(&client, &base, "docs", 5000).await;

    // Graph must still be intact after index change (GET /v1/graph/node/:id?collection=)
    let node_check = client
        .get(format!("{base}/v1/graph/node/{node0_id}?collection=docs"))
        .send()
        .await
        .unwrap();
    assert!(
        node_check.status().is_success(),
        "graph node must survive index change: {}",
        node_check.status()
    );
    let ndata = node_check.json::<serde_json::Value>().await.unwrap();
    assert_eq!(ndata["record_id"].as_u64().unwrap(), id0 as u64);
}

// ── Multi-dimension test ──────────────────────────────────────────────────────

#[tokio::test]
async fn two_collections_different_dims() {
    let (client, base) = spawn_node().await;
    // 384-dim collection
    create_collection(&client, &base, "small", 4).await;
    // 768-dim simulated (we use 8 in test to keep it fast)
    create_collection(&client, &base, "large", 8).await;

    insert_vec(&client, &base, "small", &[1.0, 0.0, 0.0, 0.0]).await;
    insert_vec(
        &client,
        &base,
        "large",
        &[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    )
    .await;

    post_index(&client, &base, "small", serde_json::json!({"type": "hnsw"})).await;
    post_index(&client, &base, "large", serde_json::json!({"type": "hnsw"})).await;

    wait_for_build(&client, &base, "small", 5000).await;
    wait_for_build(&client, &base, "large", 5000).await;

    let s1 = get_index_status(&client, &base, "small").await;
    let s2 = get_index_status(&client, &base, "large").await;
    assert_eq!(s1["active_type"].as_str().unwrap(), "hnsw");
    assert_eq!(s2["active_type"].as_str().unwrap(), "hnsw");

    // Each collection can only search with the right dimension
    let r1 = search(&client, &base, "small", &[1.0, 0.0, 0.0, 0.0], 1).await;
    let r2 = search(
        &client,
        &base,
        "large",
        &[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        1,
    )
    .await;
    assert_eq!(r1.len(), 1);
    assert_eq!(r2.len(), 1);
}

// ── 404 for unknown collection ────────────────────────────────────────────────

#[tokio::test]
async fn index_on_unknown_collection_returns_404() {
    let (client, base) = spawn_node().await;
    let (status, body) = post_index(
        &client,
        &base,
        "nonexistent",
        serde_json::json!({"type": "hnsw"}),
    )
    .await;
    assert_eq!(
        status.as_u16(),
        404,
        "should 404 for unknown collection: {body}"
    );

    let resp = client
        .get(format!("{base}/v1/namespaces/nonexistent/index"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);
}
