// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 3.5 — per-tenant API key integration tests.
//!
//! Covers: create, list, revoke, scope enforcement (read_only vs read_write),
//! legacy VALORI_AUTH_TOKEN fallback, unauthenticated rejection.

use std::sync::Arc;
use tokio::sync::RwLock;
use valori_node::api_keys::KeyStore;
use valori_node::EngineFromNodeConfig;
use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use valori_node::server::build_router_with_keys;

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn spawn_node(
    auth_token: Option<&str>,
    key_store: Arc<KeyStore>,
) -> (reqwest::Client, String) {
    let mut cfg = NodeConfig::default();
    cfg.max_records = 100;
    cfg.dim = 4;
    cfg.max_nodes = 50;
    cfg.max_edges = 50;

    let state = Arc::new(RwLock::new(Engine::new(&cfg)));
    let app = build_router_with_keys(
        state,
        auth_token.map(|s| s.to_string()),
        None,
        key_store,
        std::sync::Arc::new(valori_effect::ReceiptStore::new(64)),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let client = reqwest::Client::new();
    (client, format!("http://{}", addr))
}

async fn insert(client: &reqwest::Client, base: &str, bearer: Option<&str>) -> reqwest::Response {
    let mut req = client
        .post(format!("{base}/records"))
        .json(&serde_json::json!({ "values": [1.0, 0.0, 0.0, 0.0] }));
    if let Some(t) = bearer {
        req = req.bearer_auth(t);
    }
    req.send().await.unwrap()
}

async fn search(client: &reqwest::Client, base: &str, bearer: Option<&str>) -> reqwest::Response {
    let mut req = client
        .post(format!("{base}/search"))
        .json(&serde_json::json!({ "query": [1.0, 0.0, 0.0, 0.0], "k": 3 }));
    if let Some(t) = bearer {
        req = req.bearer_auth(t);
    }
    req.send().await.unwrap()
}

async fn create_key(
    client: &reqwest::Client,
    base: &str,
    bearer: &str,
    scope: &str,
) -> serde_json::Value {
    client
        .post(format!("{base}/v1/keys"))
        .bearer_auth(bearer)
        .json(&serde_json::json!({ "scope": scope }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn list_keys(
    client: &reqwest::Client,
    base: &str,
    bearer: &str,
) -> serde_json::Value {
    client
        .get(format!("{base}/v1/keys"))
        .bearer_auth(bearer)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn revoke_key(
    client: &reqwest::Client,
    base: &str,
    bearer: &str,
    id: &str,
) -> u16 {
    client
        .delete(format!("{base}/v1/keys/{id}"))
        .bearer_auth(bearer)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// No auth configured — all requests succeed without a token.
#[tokio::test]
async fn no_auth_all_requests_pass() {
    let (client, base) = spawn_node(None, Arc::new(KeyStore::new(None))).await;
    assert!(insert(&client, &base, None).await.status().is_success());
    assert!(search(&client, &base, None).await.status().is_success());
}

/// Legacy token set — correct token passes, wrong token fails.
#[tokio::test]
async fn legacy_token_accept_and_reject() {
    let (client, base) = spawn_node(Some("super-secret"), Arc::new(KeyStore::new(None))).await;

    // No token → 401.
    assert_eq!(insert(&client, &base, None).await.status().as_u16(), 401);
    // Wrong token → 401.
    assert_eq!(insert(&client, &base, Some("wrong")).await.status().as_u16(), 401);
    // Correct token → 200.
    assert!(insert(&client, &base, Some("super-secret")).await.status().is_success());
}

/// Create a key via the API, then use it for reads and writes.
#[tokio::test]
async fn create_key_and_use_it() {
    let (client, base) = spawn_node(Some("admin"), Arc::new(KeyStore::new(None))).await;

    // Create a read_write key using the legacy admin token.
    let body = create_key(&client, &base, "admin", "read_write").await;
    let token = body["token"].as_str().unwrap().to_string();
    let id = body["id"].as_str().unwrap().to_string();
    assert!(token.starts_with("vk_"), "token must start with vk_ prefix");

    // The new key can insert records.
    assert!(insert(&client, &base, Some(&token)).await.status().is_success());
    // The new key can search.
    assert!(search(&client, &base, Some(&token)).await.status().is_success());

    // The key appears in the list.
    let list = list_keys(&client, &base, "admin").await;
    let keys = list["keys"].as_array().unwrap();
    assert!(keys.iter().any(|k| k["id"].as_str() == Some(&id)));
}

/// List keys requires admin scope — a read_only key must be rejected with 403.
#[tokio::test]
async fn list_keys_requires_admin() {
    let (client, base) = spawn_node(Some("admin"), Arc::new(KeyStore::new(None))).await;

    // Create a read_only key.
    let body = create_key(&client, &base, "admin", "read_only").await;
    let ro_token = body["token"].as_str().unwrap().to_string();

    // read_only key cannot list keys.
    let status = client
        .get(format!("{base}/v1/keys"))
        .bearer_auth(&ro_token)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(status, 403);
}

/// read_only key can search but cannot insert.
#[tokio::test]
async fn read_only_key_cannot_write() {
    let (client, base) = spawn_node(Some("admin"), Arc::new(KeyStore::new(None))).await;

    // Pre-insert a record with admin token.
    assert!(insert(&client, &base, Some("admin")).await.status().is_success());

    // Create a read_only key.
    let body = create_key(&client, &base, "admin", "read_only").await;
    let ro_token = body["token"].as_str().unwrap().to_string();

    // read_only can search.
    assert!(search(&client, &base, Some(&ro_token)).await.status().is_success());
    // read_only cannot insert.
    assert_eq!(insert(&client, &base, Some(&ro_token)).await.status().as_u16(), 403);
}

/// Revoke a key — it must be rejected afterward.
#[tokio::test]
async fn revoke_key_stops_access() {
    let (client, base) = spawn_node(Some("admin"), Arc::new(KeyStore::new(None))).await;

    let body = create_key(&client, &base, "admin", "read_write").await;
    let token = body["token"].as_str().unwrap().to_string();
    let id = body["id"].as_str().unwrap().to_string();

    // Key works before revocation.
    assert!(insert(&client, &base, Some(&token)).await.status().is_success());

    // Revoke the key.
    assert_eq!(revoke_key(&client, &base, "admin", &id).await, 204);

    // Key is rejected after revocation.
    assert_eq!(insert(&client, &base, Some(&token)).await.status().as_u16(), 401);
}

/// Revoking a non-existent key returns 404.
#[tokio::test]
async fn revoke_nonexistent_key_returns_404() {
    let (client, base) = spawn_node(Some("admin"), Arc::new(KeyStore::new(None))).await;
    assert_eq!(revoke_key(&client, &base, "admin", "key_doesnotexist").await, 404);
}

/// health and metrics are always public — even when auth is configured.
#[tokio::test]
async fn health_always_public() {
    let (client, base) = spawn_node(Some("admin"), Arc::new(KeyStore::new(None))).await;
    let status = client
        .get(format!("{base}/health"))
        .send()
        .await
        .unwrap()
        .status();
    assert!(status.is_success());
}
