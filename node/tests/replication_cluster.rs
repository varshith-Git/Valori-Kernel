// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use valori_node::engine::Engine;
use valori_node::server::build_router;
use std::sync::Arc;
use tokio::sync::Mutex;
use tempfile::tempdir;
use tokio::time::{sleep, Duration};

#[tokio::test]
async fn test_replication_cluster() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init();

    // ── 1. Leader ─────────────────────────────────────────────────────────────
    let leader_dir = tempdir().unwrap();
    let leader_config = valori_node::config::NodeConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        wal_path: Some(leader_dir.path().join("wal.log")),
        event_log_path: Some(leader_dir.path().join("events.log")),
        mode: valori_node::config::NodeMode::Leader,
        max_records: 128,
        dim: 4,
        max_nodes: 128,
        max_edges: 256,
        ..Default::default()
    };

    let leader_engine = Engine::new(&leader_config);
    let leader_state = Arc::new(Mutex::new(leader_engine));

    {
        let mut engine = leader_state.lock().await;
        assert!(engine.event_committer.is_some(), "Leader must have event committer");
        let id0 = engine.insert_record_from_f32(&vec![0.1; 4]).unwrap();
        assert_eq!(id0, 0);
    }

    let leader_app = build_router(leader_state.clone(), None);
    let leader_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let leader_addr = leader_listener.local_addr().unwrap();
    let leader_url = format!("http://{}", leader_addr);

    tokio::spawn(async move { axum::serve(leader_listener, leader_app).await.unwrap(); });
    println!("Leader running at {}", leader_url);

    // ── 2. Follower ───────────────────────────────────────────────────────────
    let follower_dir = tempdir().unwrap();
    let follower_config = valori_node::config::NodeConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        wal_path: Some(follower_dir.path().join("wal.log")),
        event_log_path: Some(follower_dir.path().join("events.log")),
        mode: valori_node::config::NodeMode::Follower { leader_url: leader_url.clone() },
        max_records: 128,
        dim: 4,
        max_nodes: 128,
        max_edges: 256,
        ..Default::default()
    };

    let follower_engine = Engine::new(&follower_config);
    let follower_state = Arc::new(Mutex::new(follower_engine));

    let f_state = follower_state.clone();
    let f_url = leader_url.clone();
    tokio::spawn(async move {
        valori_node::replication::run_follower_loop(f_state, f_url).await;
    });

    // ── 3. Verify initial sync ─────────────────────────────────────────────────
    let mut hits = vec![];
    for _ in 0..50 { // wait up to 5 seconds
        tokio::time::sleep(Duration::from_millis(100)).await;
        let engine = follower_state.lock().await;
        hits = engine.search_l2(&vec![0.1; 4], 1).unwrap();
        if !hits.is_empty() && hits[0].0 == 0 {
            break;
        }
    }
    assert!(!hits.is_empty(), "Follower should have replicated Record 0");
    assert_eq!(hits[0].0, 0);

    // ── 4. Verify live replication ─────────────────────────────────────────────
    {
        let mut engine = leader_state.lock().await;
        let id1 = engine.insert_record_from_f32(&vec![0.2; 4]).unwrap();
        assert_eq!(id1, 1);
    }

    let mut hits = vec![];
    for _ in 0..50 { // wait up to 5 seconds
        tokio::time::sleep(Duration::from_millis(100)).await;
        let engine = follower_state.lock().await;
        hits = engine.search_l2(&vec![0.2; 4], 1).unwrap();
        if !hits.is_empty() && hits[0].0 == 1 {
            break;
        }
    }
    assert!(!hits.is_empty(), "Follower should have replicated Record 1");
    assert_eq!(hits[0].0, 1);

    println!("Replication cluster test passed.");
}
