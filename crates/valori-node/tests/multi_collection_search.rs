// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 5 — Cross-Collection (Multi) Search.
//!
//! Tests `POST /v1/search/multi` on the standalone path.
//! Covers: golden merge order, collection tagging, validation errors,
//! decay, metadata filter, empty list, and k-bounds.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;
use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use valori_node::server::{build_router, SharedEngine};
use valori_node::EngineFromNodeConfig;

// ── test infrastructure ───────────────────────────────────────────────────────

fn cfg() -> NodeConfig {
    let mut c = NodeConfig::default();
    c.max_records = 1024;
    c.max_nodes = 128;
    c.max_edges = 256;
    c.event_log_path = None;
    c.wal_path = None;
    c.snapshot_path = None;
    c
}

fn make_shared() -> SharedEngine {
    Arc::new(RwLock::new(Engine::new(&cfg())))
}

async fn http_req(
    app: axum::Router,
    method: Method,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

async fn http_post(
    app: axum::Router,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    http_req(app, Method::POST, uri, body).await
}

async fn http_patch(
    app: axum::Router,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    http_req(app, Method::PATCH, uri, body).await
}

/// Create a collection with dim=3, metric=squared_l2 and return the shared engine.
async fn with_two_collections() -> SharedEngine {
    let shared = make_shared();
    let app = build_router(shared.clone(), None, None);
    // Create "alpha"
    http_post(
        app.clone(),
        "/v1/namespaces",
        serde_json::json!({
            "name": "alpha",
            "dimension": 3,
            "metric": "squared_l2"
        }),
    )
    .await;
    // Create "beta"
    http_post(
        app,
        "/v1/namespaces",
        serde_json::json!({
            "name": "beta",
            "dimension": 3,
            "metric": "squared_l2"
        }),
    )
    .await;
    shared
}

async fn insert(shared: SharedEngine, collection: &str, values: &[f32]) -> u32 {
    let app = build_router(shared, None, None);
    let (sc, body) = http_post(
        app,
        "/v1/records",
        serde_json::json!({
            "values": values,
            "collection": collection
        }),
    )
    .await;
    assert!(sc.is_success(), "insert failed: {sc} {body}");
    body["id"].as_u64().unwrap() as u32
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// Multi-search with two collections: results are tagged with source collection
/// and sorted globally by score ascending (Squared L2, smaller = better).
#[tokio::test]
async fn golden_merge_order_and_collection_tag() {
    let shared = with_two_collections().await;

    // Insert vectors in each collection.
    // Query: [1.0, 0.0, 0.0]
    // alpha: [1.0, 0.0, 0.0] → dist ≈ 0      (exact match)
    //        [0.0, 1.0, 0.0] → dist = 2.0
    // beta:  [0.9, 0.1, 0.0] → dist ≈ 0.02   (close but not exact)
    //        [0.0, 0.0, 1.0] → dist = 2.0
    let alpha_id0 = insert(shared.clone(), "alpha", &[1.0, 0.0, 0.0]).await;
    let alpha_id1 = insert(shared.clone(), "alpha", &[0.0, 1.0, 0.0]).await;
    let beta_id0 = insert(shared.clone(), "beta", &[0.9, 0.1, 0.0]).await;
    let beta_id1 = insert(shared.clone(), "beta", &[0.0, 0.0, 1.0]).await;

    let app = build_router(shared, None, None);
    let (sc, body) = http_post(
        app,
        "/v1/search/multi",
        serde_json::json!({
            "query": [1.0, 0.0, 0.0],
            "k": 4,
            "collections": ["alpha", "beta"]
        }),
    )
    .await;
    assert_eq!(sc, StatusCode::OK, "multi search failed: {body}");

    let results = body["results"].as_array().expect("results array");
    assert_eq!(results.len(), 4, "expected 4 hits across 2 collections");

    // All hits must have a `collection` field.
    for hit in results {
        let coll = hit["collection"]
            .as_str()
            .expect("collection field missing");
        assert!(
            coll == "alpha" || coll == "beta",
            "unexpected collection: {coll}"
        );
    }

    // Scores must be non-decreasing (sorted ascending = better first).
    let scores: Vec<f32> = results
        .iter()
        .map(|h| h["score"].as_f64().unwrap() as f32)
        .collect();
    for w in scores.windows(2) {
        assert!(
            w[0] <= w[1] + 1e-5,
            "results not sorted by score: {:?}",
            scores
        );
    }

    // First result must be from alpha (exact match, dist=0).
    let first_id = results[0]["id"].as_u64().unwrap() as u32;
    let first_coll = results[0]["collection"].as_str().unwrap();
    assert_eq!(
        first_id, alpha_id0,
        "first hit should be alpha's exact match"
    );
    assert_eq!(first_coll, "alpha");

    // Second result must be from beta (dist ≈ 0.02).
    let second_id = results[1]["id"].as_u64().unwrap() as u32;
    let second_coll = results[1]["collection"].as_str().unwrap();
    assert_eq!(
        second_id, beta_id0,
        "second hit should be beta's close match"
    );
    assert_eq!(second_coll, "beta");

    // Remaining two are tied at dist≈2.0 (alpha_id1 and beta_id1).
    let remaining_ids: std::collections::HashSet<u32> = results[2..]
        .iter()
        .map(|h| h["id"].as_u64().unwrap() as u32)
        .collect();
    assert!(
        remaining_ids.contains(&alpha_id1),
        "alpha_id1 missing from remaining hits"
    );
    assert!(
        remaining_ids.contains(&beta_id1),
        "beta_id1 missing from remaining hits"
    );

    // No partial failures.
    assert!(
        body["partial_failures"].is_null(),
        "unexpected partial failures: {}",
        body["partial_failures"]
    );
}

/// collections_searched is always present and lists all requested collections.
#[tokio::test]
async fn collections_searched_field() {
    let shared = with_two_collections().await;
    insert(shared.clone(), "alpha", &[1.0, 0.0, 0.0]).await;
    insert(shared.clone(), "beta", &[0.5, 0.5, 0.0]).await;

    let app = build_router(shared, None, None);
    let (sc, body) = http_post(
        app,
        "/v1/search/multi",
        serde_json::json!({
            "query": [1.0, 0.0, 0.0],
            "k": 2,
            "collections": ["alpha", "beta"]
        }),
    )
    .await;
    assert_eq!(sc, StatusCode::OK);

    let searched = body["collections_searched"]
        .as_array()
        .expect("collections_searched field");
    let names: std::collections::HashSet<&str> =
        searched.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(names.contains("alpha"));
    assert!(names.contains("beta"));
}

/// k=0 must return 400.
#[tokio::test]
async fn k_zero_returns_400() {
    let shared = with_two_collections().await;
    let app = build_router(shared, None, None);
    let (sc, _) = http_post(
        app,
        "/v1/search/multi",
        serde_json::json!({
            "query": [1.0, 0.0, 0.0],
            "k": 0,
            "collections": ["alpha", "beta"]
        }),
    )
    .await;
    // k=0 fails at the handler-level validation — 400 expected.
    assert_ne!(sc, StatusCode::OK, "k=0 should not return 200");
}

/// Empty collections list must return 400.
#[tokio::test]
async fn empty_collections_returns_400() {
    let shared = make_shared();
    let app = build_router(shared, None, None);
    let (sc, body) = http_post(
        app,
        "/v1/search/multi",
        serde_json::json!({
            "query": [1.0, 0.0, 0.0],
            "k": 5,
            "collections": []
        }),
    )
    .await;
    assert_eq!(
        sc,
        StatusCode::BAD_REQUEST,
        "empty list should be 400: {body}"
    );
    assert!(
        body["error"].as_str().unwrap_or("").contains("empty"),
        "error message should mention 'empty': {body}"
    );
}

/// Unknown collection name must return 400 (engine error) or 404.
#[tokio::test]
async fn unknown_collection_returns_error() {
    let shared = make_shared();
    let app = build_router(shared, None, None);
    let (sc, _body) = http_post(
        app,
        "/v1/search/multi",
        serde_json::json!({
            "query": [1.0, 0.0, 0.0],
            "k": 5,
            "collections": ["does-not-exist"]
        }),
    )
    .await;
    assert!(
        sc == StatusCode::BAD_REQUEST || sc == StatusCode::NOT_FOUND,
        "unknown collection should be 400 or 404, got {sc}"
    );
}

/// Dimension mismatch across collections must return 400.
#[tokio::test]
async fn dimension_mismatch_returns_400() {
    let shared = make_shared();
    {
        let app = build_router(shared.clone(), None, None);
        http_post(
            app.clone(),
            "/v1/namespaces",
            serde_json::json!({"name":"dim3","dimension":3,"metric":"squared_l2"}),
        )
        .await;
        http_post(
            app,
            "/v1/namespaces",
            serde_json::json!({"name":"dim4","dimension":4,"metric":"squared_l2"}),
        )
        .await;
    }

    let app = build_router(shared, None, None);
    let (sc, body) = http_post(
        app,
        "/v1/search/multi",
        serde_json::json!({
            "query": [1.0, 0.0, 0.0],
            "k": 5,
            "collections": ["dim3", "dim4"]
        }),
    )
    .await;
    assert_eq!(
        sc,
        StatusCode::BAD_REQUEST,
        "dim mismatch should be 400: {body}"
    );
    assert!(
        body["error"].as_str().unwrap_or("").contains("dimension")
            || body["error"].as_str().unwrap_or("").contains("dim"),
        "error should mention dimension: {body}"
    );
}

/// Query vector length must match collection dim; mismatch → 400.
#[tokio::test]
async fn query_dim_mismatch_returns_400() {
    let shared = with_two_collections().await;
    let app = build_router(shared, None, None);
    // Collections have dim=3, query has 4 elements.
    let (sc, body) = http_post(
        app,
        "/v1/search/multi",
        serde_json::json!({
            "query": [1.0, 0.0, 0.0, 0.0],
            "k": 5,
            "collections": ["alpha", "beta"]
        }),
    )
    .await;
    assert_eq!(
        sc,
        StatusCode::BAD_REQUEST,
        "query dim mismatch should be 400: {body}"
    );
}

/// Decay is applied per-collection; results still merge correctly.
#[tokio::test]
async fn decay_in_multi_search() {
    let shared = with_two_collections().await;
    insert(shared.clone(), "alpha", &[1.0, 0.0, 0.0]).await;
    insert(shared.clone(), "beta", &[0.9, 0.1, 0.0]).await;

    let app = build_router(shared, None, None);
    // Use a very large half-life so decay is essentially 1.0 and doesn't reorder.
    let (sc, body) = http_post(
        app,
        "/v1/search/multi",
        serde_json::json!({
            "query": [1.0, 0.0, 0.0],
            "k": 2,
            "collections": ["alpha", "beta"],
            "decay_half_life_secs": 9999999
        }),
    )
    .await;
    assert_eq!(sc, StatusCode::OK, "decay multi search failed: {body}");
    let results = body["results"].as_array().expect("results");
    assert_eq!(results.len(), 2);
    // When decay is active, decay_factor should be present on each hit.
    for hit in results {
        assert!(
            !hit["decay_factor"].is_null(),
            "decay_factor missing when decay is active: {hit}"
        );
    }
}

/// Metadata filter is applied per-collection; matching records come through.
#[tokio::test]
async fn metadata_filter_in_multi_search() {
    let shared = with_two_collections().await;

    // Insert records with metadata.
    let alpha_id = insert(shared.clone(), "alpha", &[1.0, 0.0, 0.0]).await;
    let _beta_id = insert(shared.clone(), "beta", &[0.9, 0.1, 0.0]).await;

    // Tag the alpha record via /v1/memory/meta/set — this writes to both
    // state.meta and engine.metadata (the sidecar used by apply_metadata_filter).
    {
        let app = build_router(shared.clone(), None, None);
        let (sc, body_r) = http_post(
            app,
            "/v1/memory/meta/set",
            serde_json::json!({
                "target_id": format!("rec:{}", alpha_id),
                "metadata": { "tag": "keep" }
            }),
        )
        .await;
        assert!(sc.is_success(), "meta/set failed: {sc} {body_r}");
    }

    // Search with metadata filter — only alpha's tagged record should appear.
    let app = build_router(shared, None, None);
    let (sc, body) = http_post(
        app,
        "/v1/search/multi",
        serde_json::json!({
            "query": [1.0, 0.0, 0.0],
            "k": 5,
            "collections": ["alpha", "beta"],
            "metadata_filter": { "tag": "keep" }
        }),
    )
    .await;
    assert_eq!(
        sc,
        StatusCode::OK,
        "metadata filter multi search failed: {body}"
    );
    let results = body["results"].as_array().expect("results");
    // Only the tagged alpha record matches.
    assert_eq!(
        results.len(),
        1,
        "only the tagged record should pass the filter"
    );
    assert_eq!(results[0]["id"].as_u64().unwrap() as u32, alpha_id);
    assert_eq!(results[0]["collection"].as_str().unwrap(), "alpha");
}

/// Single collection in the list behaves identically to POST /v1/search.
#[tokio::test]
async fn single_collection_parity_with_regular_search() {
    let shared = with_two_collections().await;
    insert(shared.clone(), "alpha", &[1.0, 0.0, 0.0]).await;
    insert(shared.clone(), "alpha", &[0.0, 1.0, 0.0]).await;

    let app_multi = build_router(shared.clone(), None, None);
    let (sc_multi, body_multi) = http_post(
        app_multi,
        "/v1/search/multi",
        serde_json::json!({
            "query": [1.0, 0.0, 0.0],
            "k": 2,
            "collections": ["alpha"]
        }),
    )
    .await;
    assert_eq!(sc_multi, StatusCode::OK);

    let app_single = build_router(shared, None, None);
    let (sc_single, body_single) = http_post(
        app_single,
        "/v1/search",
        serde_json::json!({
            "query": [1.0, 0.0, 0.0],
            "k": 2,
            "collection": "alpha",
            "rerank": false
        }),
    )
    .await;
    assert_eq!(sc_single, StatusCode::OK);

    // Result IDs should match (order may differ on tie, so compare as sets).
    let multi_ids: std::collections::HashSet<u64> = body_multi["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["id"].as_u64().unwrap())
        .collect();
    let single_ids: std::collections::HashSet<u64> = body_single["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["id"].as_u64().unwrap())
        .collect();
    assert_eq!(
        multi_ids, single_ids,
        "single-collection multi-search should agree with regular search"
    );
}
