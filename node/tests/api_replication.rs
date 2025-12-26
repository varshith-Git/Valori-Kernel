use valori_node::engine::Engine;
use valori_node::server::build_router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::util::ServiceExt; 
use std::sync::Arc;
use tokio::sync::Mutex;
use tempfile::tempdir;

// Integration test for Replication Stream
#[tokio::test]
async fn test_replication_stream_endpoint() {
    // 1. Setup Engine & Server
    let dir = tempdir().unwrap();
    let wal_path = dir.path().join("wal.log");
    let event_log_path = dir.path().join("events.log"); // Engine uses this by default if configured?
    
    // We need to configure Engine to use Event Logging
    // By default `Engine::new()` doesn't enabling event log unless we do something?
    // Looking at `Engine::new`: it takes `wal_path` and `event_log_path`.
    
    // We need to construct Engine manually or via new.
    // Let's check Engine::new signature.
    // Use defaults for consts: 1024, 16, 1024, 2048
    
    let config = valori_node::config::NodeConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        wal_path: Some(wal_path.clone()),
        event_log_path: Some(event_log_path.clone()),
        mode: valori_node::config::NodeMode::Leader,
        max_records: 128,
        dim: 4,
        max_nodes: 128,
        max_edges: 256,
        ..Default::default()
    };

    let mut engine = valori_node::engine::Engine::<128, 4, 128, 256>::new(&config);
    
    // 2. Insert Initial Data (Historical)
    let vec = vec![0.1; 4];
    let id1 = engine.insert_record_from_f32(&vec).unwrap();
    
    // Verify it's in event log (committed)
    assert!(engine.event_committer.is_some());
    assert_eq!(engine.event_committer.as_ref().unwrap().journal().committed_height(), 1);

    let state = Arc::new(Mutex::new(engine));
    let app = build_router(state.clone(), None);

    // 3. Start Streaming Client
    // We cannot easily use `oneshot` for streaming body in a test because we need to parse the chunks
    // and `axum::body::Body` is opaque.
    // Instead we spawn a real server and use reqwest which handles streaming well.
    
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    
    // Configure app (no auth)
    let app = build_router(state.clone(), None);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    
    // 4. Client Connects
    let client = reqwest::Client::new();
    let url = format!("http://{}/v1/replication/events", addr);
    
    let mut res = client.get(&url)
        .send()
        .await
        .unwrap();
        
    assert!(res.status().is_success());
    
    // 5. Read Stream
    // We expect 1 event first (the one we inserted)
    // reqwest stream
    let chunk1 = res.chunk().await.unwrap().unwrap();
    let s1 = String::from_utf8(chunk1.to_vec()).unwrap();
    println!("Chunk 1: {}", s1);
    assert!(s1.contains("Event"));
    assert!(s1.contains("InsertRecord"));

    // 6. Insert Live Data
    {
        let mut engine_lock = state.lock().await;
        // Insert another record
        engine_lock.insert_record_from_f32(&vec).unwrap();
    }
    
    // 7. verification: Read Stream Again
    // We expect the new event
    // The stream should yield new data
    let chunk2 = res.chunk().await.unwrap().unwrap();
    let s2 = String::from_utf8(chunk2.to_vec()).unwrap();
    println!("Chunk 2: {}", s2);
    // Since chunking is non-deterministic (network), we might get partial or combined chunks.
    // But since we inserted cleanly, we likely get it.
    assert!(s2.contains("Event"));
}
