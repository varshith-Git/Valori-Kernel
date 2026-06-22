// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Integration test for the replication streaming endpoint.
//!
//! Inserts one record, then verifies /v1/replication/events yields
//! the historical InsertRecord event and streams new ones live.
use valori_node::engine::Engine;
use valori_node::server::build_router;
use std::sync::Arc;
use tokio::sync::RwLock;
use tempfile::tempdir;

#[tokio::test]
async fn test_replication_stream_endpoint() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init();

    // ── 1. Engine with event log enabled ──────────────────────────────────────
    let dir = tempdir().unwrap();

    let config = valori_node::config::NodeConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        wal_path: Some(dir.path().join("wal.log")),
        event_log_path: Some(dir.path().join("events.log")),
        mode: valori_node::config::NodeMode::Leader,
        max_records: 128,
        dim: 4,
        max_nodes: 128,
        max_edges: 256,
        ..Default::default()
    };

    let mut engine = Engine::new(&config);

    // ── 2. Insert initial record ──────────────────────────────────────────────
    let vec = vec![0.1f32; 4];
    engine.insert_record_from_f32(&vec).unwrap();

    // Verify the event was committed in-memory.
    assert!(engine.event_committer.is_some());
    assert_eq!(
        engine.event_committer.as_ref().unwrap().journal().committed_height(),
        1
    );

    let state = Arc::new(RwLock::new(engine));

    // ── 3. Start server ───────────────────────────────────────────────────────
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_router(state.clone(), None, None);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // ── 4. Connect streaming client ───────────────────────────────────────────
    let client = reqwest::Client::new();
    let url = format!("http://{}/v1/replication/events", addr);

    let mut res = client.get(&url).send().await.unwrap();
    assert!(res.status().is_success(), "Streaming endpoint must return 200");

    // ── 5. First chunk: historical InsertRecord event ─────────────────────────
    let chunk1 = res.chunk().await.unwrap().unwrap();
    let s1 = String::from_utf8(chunk1.to_vec()).unwrap();
    println!("Chunk 1: {}", s1);
    assert!(s1.contains("b64"), "First chunk must contain the historical event in base64");

    // ── 6. Insert a live record ───────────────────────────────────────────────
    {
        let mut engine_lock = state.write().await;
        engine_lock.insert_record_from_f32(&vec).unwrap();
    }

    // ── 7. Second chunk: live event ───────────────────────────────────────────
    // Chunking is non-deterministic (network buffering), but the event must arrive.
    let chunk2 = res.chunk().await.unwrap().unwrap();
    let s2 = String::from_utf8(chunk2.to_vec()).unwrap();
    println!("Chunk 2: {}", s2);
    assert!(s2.contains("b64"), "Second chunk must contain at least one base64 event");
}
