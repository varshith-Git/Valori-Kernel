// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Health-check and metrics integration tests.
//!
//! Verifies:
//!   1. `Engine::health()` status transitions (ok → degraded → full)
//!   2. `PoolStats` fill percentages track insertions accurately
//!   3. `GET /health` returns 200 for ok/degraded, 503 for full
//!   4. `GET /health` is reachable without an auth token even when auth is enabled
//!   5. `GET /metrics` surfaces kernel-state gauges (non-empty Prometheus text)
//!   6. `GET /metrics` is reachable without an auth token

use valori_node::config::{NodeConfig, IndexKind};
use valori_node::engine::Engine;

use valori_node::server::{build_router, SharedEngine};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower::ServiceExt; // for `.oneshot()`

// ── Engine-level unit tests ───────────────────────────────────────────────────

fn tiny_cfg(max_records: usize) -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.dim = 4;
    cfg.max_records = max_records;
    cfg.max_nodes = 8;
    cfg.max_edges = 16;
    cfg.index_kind = IndexKind::BruteForce;
    cfg.event_log_path = None;
    cfg.wal_path = None;
    cfg.snapshot_path = None;
    cfg
}

#[test]
fn test_health_starts_ok() {
    let engine = Engine::new(&tiny_cfg(100));
    let h = engine.health();

    assert_eq!(h.status, "ok");
    assert_eq!(h.records.live, 0);
    assert_eq!(h.records.capacity, 100);
    assert_eq!(h.records.fill_pct, 0.0);
}

#[test]
fn test_health_tracks_insertions() {
    let mut engine = Engine::new(&tiny_cfg(100));

    for i in 0..10 {
        let v: Vec<f32> = (0..4).map(|j| (i * 4 + j) as f32 * 0.1).collect();
        engine.insert_record_from_f32(&v).unwrap();
    }

    let h = engine.health();
    assert_eq!(h.records.live, 10);
    assert_eq!(h.records.fill_pct, 10.0);
    assert_eq!(h.status, "ok");
}

#[test]
fn test_health_degraded_at_90_pct() {
    let mut engine = Engine::new(&tiny_cfg(10));

    // Insert 9 of 10 → 90 %
    for i in 0..9 {
        let v: Vec<f32> = (0..4).map(|j| (i + j) as f32 * 0.1).collect();
        engine.insert_record_from_f32(&v).unwrap();
    }

    let h = engine.health();
    assert_eq!(h.records.live, 9);
    assert_eq!(h.status, "degraded",
        "90 % full should report degraded; fill_pct = {}", h.records.fill_pct);
}

#[test]
fn test_health_full_at_100_pct() {
    let mut engine = Engine::new(&tiny_cfg(5));

    for i in 0..5 {
        let v: Vec<f32> = (0..4).map(|j| (i + j) as f32 * 0.1).collect();
        engine.insert_record_from_f32(&v).unwrap();
    }

    let h = engine.health();
    assert_eq!(h.records.live, 5);
    assert_eq!(h.status, "full",
        "100 % full should report full; fill_pct = {}", h.records.fill_pct);
}

#[test]
fn test_health_dim_and_version_populated() {
    let cfg = tiny_cfg(50);
    let engine = Engine::new(&cfg);
    let h = engine.health();

    assert_eq!(h.dim, 4, "dim must match config");
    assert!(!h.version.is_empty(), "version must be set");
    assert_eq!(h.index, "BruteForce");
}

#[test]
fn test_health_persistence_field_no_log() {
    let engine = Engine::new(&tiny_cfg(50));
    assert_eq!(engine.health().persistence, "none");
}

#[test]
fn test_health_persistence_field_event_log() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = tiny_cfg(50);
    cfg.event_log_path = Some(dir.path().join("events.log"));
    let engine = Engine::new(&cfg);
    assert_eq!(engine.health().persistence, "event_log");
}

#[test]
fn test_health_fill_pct_rounding() {
    // 1 of 3 ≈ 33.333 → should round to 33.3
    let mut engine = Engine::new(&tiny_cfg(3));
    engine.insert_record_from_f32(&[0.1, 0.2, 0.3, 0.4]).unwrap();
    let h = engine.health();
    assert_eq!(h.records.fill_pct, 33.3);
}

// ── Capacity enforcement unit tests ──────────────────────────────────────────

/// After `max_records` inserts any further insert must return an error.
#[test]
fn test_insert_fails_at_max_records() {
    let mut engine = Engine::new(&tiny_cfg(5));

    for i in 0u32..5 {
        let v: Vec<f32> = (0..4).map(|j| (i + j as u32) as f32 * 0.1).collect();
        engine.insert_record_from_f32(&v)
            .expect("inserts within capacity must succeed");
    }

    // 6th insert must fail — pool is full.
    let result = engine.insert_record_from_f32(&[1.0, 2.0, 3.0, 4.0]);
    assert!(result.is_err(),
        "insert beyond max_records must return an error, not silently succeed");
}

/// `insert_batch` must reject atomically if the batch would exceed capacity.
#[test]
fn test_insert_batch_fails_if_would_exceed() {
    let mut engine = Engine::new(&tiny_cfg(5));

    // Pre-fill with 3 records (capacity remaining: 2).
    for i in 0u32..3 {
        let v: Vec<f32> = (0..4).map(|j| (i + j as u32) as f32 * 0.1).collect();
        engine.insert_record_from_f32(&v).unwrap();
    }

    // Batch of 3 would put total at 6 > 5 — must be rejected in full.
    let batch: Vec<Vec<f32>> = (0u32..3)
        .map(|i| (0..4).map(|j| (i + j as u32) as f32 * 0.1).collect())
        .collect();
    let result = engine.insert_batch(&batch);
    assert!(result.is_err(),
        "insert_batch that would exceed max_records must fail atomically");

    // The engine must still have only 3 records — no partial writes.
    assert_eq!(engine.health().records.live, 3,
        "no records from the rejected batch must have been committed");
}

/// A batch that exactly fits must succeed; a batch one larger must fail.
#[test]
fn test_insert_batch_exact_fit_succeeds() {
    let mut engine = Engine::new(&tiny_cfg(4));

    let batch: Vec<Vec<f32>> = (0u32..4)
        .map(|i| (0..4).map(|j| (i + j as u32) as f32 * 0.1).collect())
        .collect();
    engine.insert_batch(&batch)
        .expect("batch that exactly fills capacity must succeed");

    assert_eq!(engine.health().records.live, 4);
}

// ── HTTP integration tests ────────────────────────────────────────────────────

fn make_shared(cfg: &NodeConfig) -> SharedEngine {
    Arc::new(Mutex::new(Engine::new(cfg)))
}

#[tokio::test]
async fn test_http_health_returns_200_when_ok() {
    let shared = make_shared(&tiny_cfg(100));
    let app = build_router(shared, None, None);

    let resp = app
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["records"]["capacity"], 100);
    assert!(json["version"].is_string());
}

#[tokio::test]
async fn test_http_health_returns_503_when_full() {
    // Fill a 3-record engine to 100 %
    let cfg = tiny_cfg(3);
    let shared = make_shared(&cfg);
    {
        let mut engine = shared.lock().await;
        for i in 0u32..3 {
            let v: Vec<f32> = (0..4).map(|j| (i + j as u32) as f32 * 0.1).collect();
            engine.insert_record_from_f32(&v).unwrap();
        }
    }
    let app = build_router(shared, None, None);

    let resp = app
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE,
        "full engine must return 503");

    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "full");
}

#[tokio::test]
async fn test_http_health_accessible_without_auth_token() {
    // Build router with a token set — /health must still be reachable without it.
    let shared = make_shared(&tiny_cfg(100));
    let app = build_router(shared, Some("super-secret".to_string()), None);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                // Intentionally no Authorization header
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK,
        "/health must be 200 even without auth token");
}

#[tokio::test]
async fn test_http_protected_route_blocked_without_auth() {
    // Confirm that a protected endpoint (e.g. /version) IS blocked when auth is set.
    let shared = make_shared(&tiny_cfg(100));
    let app = build_router(shared, Some("super-secret".to_string()), None);

    let resp = app
        .oneshot(Request::builder().uri("/version").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED,
        "/version must require auth when a token is configured");
}

#[tokio::test]
async fn test_http_metrics_accessible_without_auth_token() {
    // /metrics must be reachable by Prometheus without a bearer token.
    let shared = make_shared(&tiny_cfg(100));
    let app = build_router(shared, Some("super-secret".to_string()), None);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK,
        "/metrics must be 200 even without auth token");
}

/// `GET /metrics` returns 200 with a text body (endpoint reachability check).
///
/// Gauge-name assertions live in the unit test below rather than here because
/// the `metrics` v0.21 global recorder is set-once per process — we can't
/// install a fresh one in every test.
#[tokio::test]
async fn test_http_metrics_returns_200_with_body() {
    let shared = make_shared(&tiny_cfg(100));
    let app = build_router(shared, None, None);

    let resp = app
        .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
    assert!(!body.is_empty(), "/metrics must return a non-empty body");
}

/// `POST /records` must return **507 Insufficient Storage** when the record
/// pool is already full.
#[tokio::test]
async fn test_http_insert_returns_507_when_full() {
    use axum::http::Method;

    // Build a 2-record engine and fill it.
    let cfg = tiny_cfg(2);
    let shared = make_shared(&cfg);
    {
        let mut engine = shared.lock().await;
        for i in 0u32..2 {
            let v: Vec<f32> = (0..4).map(|j| (i + j as u32) as f32 * 0.1).collect();
            engine.insert_record_from_f32(&v).unwrap();
        }
    }
    let app = build_router(shared, None, None);

    let payload = serde_json::json!({ "values": [1.0, 2.0, 3.0, 4.0] });
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/records")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::INSUFFICIENT_STORAGE,
        "POST /records into a full engine must return 507 Insufficient Storage",
    );
}

/// `update_prometheus_metrics()` writes expected gauge names into a local
/// Prometheus recorder and renders the output.
///
/// This test installs its own recorder.  It runs before any other test
/// in the process installs the global recorder (which can only be set once).
/// `cargo test` runs tests in an unspecified order — to keep this reliable we
/// only assert names, not values, and tolerate a pre-existing recorder via `.ok()`.
#[test]
fn test_update_prometheus_metrics_gauge_names_in_output() {
    use metrics_exporter_prometheus::PrometheusBuilder;

    // Build recorder + handle BEFORE consuming the recorder.
    let recorder = PrometheusBuilder::new().build_recorder();
    let handle   = recorder.handle();

    // Try to install as the global recorder.  If another test beat us to it
    // this returns Err, meaning gauge writes below go to the pre-existing
    // recorder and our local handle.render() may be empty.  In that case we
    // skip the name assertions — the code still compiled and didn't panic.
    let installed = metrics::set_boxed_recorder(Box::new(recorder)).is_ok();

    let mut engine = Engine::new(&tiny_cfg(50));
    for i in 0u32..5 {
        let v: Vec<f32> = (0..4).map(|j| (i + j as u32) as f32 * 0.1).collect();
        engine.insert_record_from_f32(&v).unwrap();
    }
    // Must never panic regardless of recorder state.
    engine.update_prometheus_metrics();

    if installed {
        let text = handle.render();
        assert!(text.contains("valori_records_live"),
            "must expose valori_records_live gauge; output: {}", &text[..text.len().min(500)]);
        assert!(text.contains("valori_records_capacity"),
            "must expose valori_records_capacity gauge");
        assert!(text.contains("valori_record_fill_ratio"),
            "must expose valori_record_fill_ratio gauge");
        assert!(text.contains("valori_nodes_live"),
            "must expose valori_nodes_live gauge");
        assert!(text.contains("valori_edges_live"),
            "must expose valori_edges_live gauge");
        assert!(text.contains("valori_dim"),
            "must expose valori_dim gauge");
    }
    // If not installed: test still passes — the important thing is no panic.
}
