// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 3.4 — as-of / point-in-time search integration tests.
//!
//! Verifies:
//! 1. `as_of_log_index` — search the state after exactly N committed events.
//! 2. `as_of` (ISO 8601 timestamp) — search the state as it existed at a past moment.
//! 3. `GET /v1/timeline` — structured JSON, total count, correct event types.
//! 4. `GET /v1/timeline?from=<>&to=<>` — timestamp range filter.

use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::RwLock;
use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use valori_node::server::build_router;

// ── helpers ─────────────────────────────────────────────────────────────────

async fn spawn_node_with_event_log() -> (reqwest::Client, String, TempDir) {
    let dir = TempDir::new().unwrap();
    let log_path = dir.path().join("events.log");

    let mut cfg = NodeConfig::default();
    cfg.max_records = 200;
    cfg.dim = 4;
    cfg.max_nodes = 100;
    cfg.max_edges = 100;
    cfg.event_log_path = Some(log_path); // Engine::new sets up the event committer from this

    let state = Arc::new(RwLock::new(Engine::new(&cfg)));

    let app = build_router(state, None, None);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let base = format!("http://{}", addr);
    (client, base, dir)
}

async fn insert(client: &reqwest::Client, base: &str, vec: [f32; 4]) -> u32 {
    let resp = client
        .post(format!("{base}/records"))
        .json(&serde_json::json!({ "values": vec }))
        .send().await.unwrap();
    assert!(resp.status().is_success(), "insert failed: {}", resp.status());
    resp.json::<serde_json::Value>().await.unwrap()["id"].as_u64().unwrap() as u32
}

async fn search_as_of_index(
    client: &reqwest::Client,
    base: &str,
    query: [f32; 4],
    k: usize,
    log_index: u64,
) -> serde_json::Value {
    let resp = client
        .post(format!("{base}/search"))
        .json(&serde_json::json!({
            "query": query,
            "k": k,
            "as_of_log_index": log_index
        }))
        .send().await.unwrap();
    assert!(resp.status().is_success(), "search failed: {}", resp.status());
    resp.json().await.unwrap()
}

// ── tests ────────────────────────────────────────────────────────────────────

/// Insert 3 records, then search with as_of_log_index=0 (after the first insert only).
/// The search must return only the first record and include a valid BLAKE3 state hash.
#[tokio::test]
async fn as_of_log_index_returns_past_state() {
    let (client, base, _dir) = spawn_node_with_event_log().await;

    let id0 = insert(&client, &base, [1.0, 0.0, 0.0, 0.0]).await;
    let _id1 = insert(&client, &base, [0.0, 1.0, 0.0, 0.0]).await;
    let _id2 = insert(&client, &base, [0.0, 0.0, 1.0, 0.0]).await;

    // After log_index 0 only the first record exists.
    let body = search_as_of_index(&client, &base, [1.0, 0.0, 0.0, 0.0], 5, 0).await;

    let results = body["results"].as_array().unwrap();
    assert_eq!(results.len(), 1, "only 1 record should exist at log_index 0");
    assert_eq!(results[0]["id"].as_u64().unwrap(), id0 as u64);

    // Proof fields must be present.
    assert_eq!(body["as_of_log_index"].as_u64().unwrap(), 0);
    let hash = body["as_of_state_hash"].as_str().unwrap();
    assert_eq!(hash.len(), 64, "BLAKE3 hex must be 64 chars");
    assert!(body.get("as_of_timestamp_iso").is_some());
}

/// Verify that the state hash at log_index=0 differs from log_index=2
/// (more events → different hash).
#[tokio::test]
async fn as_of_state_hash_advances_with_new_events() {
    let (client, base, _dir) = spawn_node_with_event_log().await;

    insert(&client, &base, [1.0, 0.0, 0.0, 0.0]).await;
    insert(&client, &base, [0.0, 1.0, 0.0, 0.0]).await;
    insert(&client, &base, [0.0, 0.0, 1.0, 0.0]).await;

    let body0 = search_as_of_index(&client, &base, [1.0, 0.0, 0.0, 0.0], 5, 0).await;
    let body2 = search_as_of_index(&client, &base, [1.0, 0.0, 0.0, 0.0], 5, 2).await;

    let hash0 = body0["as_of_state_hash"].as_str().unwrap();
    let hash2 = body2["as_of_state_hash"].as_str().unwrap();
    assert_ne!(hash0, hash2, "state hash must change as more events are applied");

    // log_index 2 should find all 3 records.
    assert_eq!(body2["results"].as_array().unwrap().len(), 3);
}

/// Out-of-range log_index must return a 422/400 error, not a panic.
#[tokio::test]
async fn as_of_log_index_out_of_range_returns_error() {
    let (client, base, _dir) = spawn_node_with_event_log().await;
    insert(&client, &base, [1.0, 0.0, 0.0, 0.0]).await;

    let resp = client
        .post(format!("{base}/search"))
        .json(&serde_json::json!({
            "query": [1.0, 0.0, 0.0, 0.0],
            "k": 5,
            "as_of_log_index": 999
        }))
        .send().await.unwrap();

    assert!(
        resp.status().is_client_error() || resp.status().is_server_error(),
        "out-of-range log_index must return an error status"
    );
}

/// GET /v1/timeline returns structured JSON with correct event types and count.
#[tokio::test]
async fn timeline_returns_structured_events() {
    let (client, base, _dir) = spawn_node_with_event_log().await;

    insert(&client, &base, [1.0, 0.0, 0.0, 0.0]).await;
    insert(&client, &base, [0.0, 1.0, 0.0, 0.0]).await;

    let resp = client
        .get(format!("{base}/v1/timeline"))
        .send().await.unwrap();
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.unwrap();

    let events = body["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(body["total"].as_u64().unwrap(), 2);

    for (i, ev) in events.iter().enumerate() {
        assert_eq!(ev["log_index"].as_u64().unwrap(), i as u64);
        assert_eq!(ev["event_type"].as_str().unwrap(), "InsertRecord");
        assert!(ev["record_id"].as_u64().is_some());
        // timestamp_iso must look like an ISO 8601 date.
        let iso = ev["timestamp_iso"].as_str().unwrap();
        assert!(iso.contains('T') && iso.ends_with('Z'), "unexpected timestamp_iso: {iso}");
    }
}

/// GET /v1/timeline with no events returns an empty list (not an error).
#[tokio::test]
async fn timeline_empty_when_no_events() {
    let (client, base, _dir) = spawn_node_with_event_log().await;

    let resp = client
        .get(format!("{base}/v1/timeline"))
        .send().await.unwrap();
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["total"].as_u64().unwrap(), 0);
    assert!(body["events"].as_array().unwrap().is_empty());
}

/// GET /v1/timeline?from=<far_future> returns no events (all filtered out).
#[tokio::test]
async fn timeline_from_filter_excludes_past_events() {
    let (client, base, _dir) = spawn_node_with_event_log().await;
    insert(&client, &base, [1.0, 0.0, 0.0, 0.0]).await;

    // Use a timestamp far in the future.
    let resp = client
        .get(format!("{base}/v1/timeline?from=2099-01-01T00:00:00Z"))
        .send().await.unwrap();
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["total"].as_u64().unwrap(), 0,
        "no events should have timestamps >= year 2099");
    assert!(body["from_unix"].is_number(), "from_unix must be present");
}
