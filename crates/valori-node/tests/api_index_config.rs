// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 3.13 — HNSW parameter exposure tests.

use valori_node::config::{NodeConfig, IndexKind};
use valori_node::server::build_router;
use valori_node::engine::Engine;
use axum::{body::Body, http::{Request, StatusCode}};
use tower::ServiceExt;
use std::sync::Arc;
use tokio::sync::RwLock;

const DIM: usize = 4;

fn make_engine_brute() -> Arc<RwLock<Engine>> {
    let mut cfg = NodeConfig::default();
    cfg.dim = DIM;
    cfg.index_kind = IndexKind::BruteForce;
    Arc::new(RwLock::new(Engine::new(&cfg)))
}

fn make_engine_hnsw(m: Option<usize>, ef_construction: Option<usize>, ef_search: Option<usize>) -> Arc<RwLock<Engine>> {
    let mut cfg = NodeConfig::default();
    cfg.dim = DIM;
    cfg.index_kind = IndexKind::Hnsw;
    cfg.hnsw_m = m;
    cfg.hnsw_ef_construction = ef_construction;
    cfg.hnsw_ef_search = ef_search;
    Arc::new(RwLock::new(Engine::new(&cfg)))
}

async fn get_index_config(shared: Arc<RwLock<Engine>>) -> (StatusCode, serde_json::Value) {
    let app = build_router(shared, None, None);
    let req = Request::builder()
        .method("GET")
        .uri("/v1/index/config")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), 1 << 16).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (status, json)
}

#[tokio::test]
async fn brute_force_config_returns_correct_type() {
    let engine = make_engine_brute();
    let (status, json) = get_index_config(engine).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["index_type"], "brute_force");
    assert!(json["hnsw"].is_null());
}

#[tokio::test]
async fn hnsw_default_config_returns_defaults() {
    let engine = make_engine_hnsw(None, None, None);
    let (status, json) = get_index_config(engine).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["index_type"], "hnsw");
    let hnsw = &json["hnsw"];
    assert_eq!(hnsw["m"], 16);
    assert_eq!(hnsw["m_max0"], 32);
    assert_eq!(hnsw["ef_construction"], 100);
    assert_eq!(hnsw["ef_search"], 50);
}

#[tokio::test]
async fn hnsw_custom_m_derives_m_max0_and_lambda() {
    let engine = make_engine_hnsw(Some(8), None, None);
    let (status, json) = get_index_config(engine).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["index_type"], "hnsw");
    assert_eq!(json["hnsw"]["m"], 8);
    assert_eq!(json["hnsw"]["m_max0"], 16); // m_max0 = 2*m
}

#[tokio::test]
async fn hnsw_custom_ef_search_is_reflected() {
    let engine = make_engine_hnsw(None, None, Some(200));
    let (status, json) = get_index_config(engine).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["hnsw"]["ef_search"], 200);
    assert_eq!(json["hnsw"]["ef_construction"], 100); // default unchanged
}

#[tokio::test]
async fn hnsw_all_params_set() {
    let engine = make_engine_hnsw(Some(32), Some(400), Some(100));
    let (status, json) = get_index_config(engine).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["hnsw"]["m"], 32);
    assert_eq!(json["hnsw"]["m_max0"], 64);
    assert_eq!(json["hnsw"]["ef_construction"], 400);
    assert_eq!(json["hnsw"]["ef_search"], 100);
}
