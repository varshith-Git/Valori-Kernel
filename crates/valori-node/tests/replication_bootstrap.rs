// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Offline bootstrap integration test.
//!
//! Populate leader → snapshot → fresh follower bootstraps from snapshot →
//! verify record count → verify live replication continues.
use valori_node::config::{NodeConfig, NodeMode};
use valori_node::engine::Engine;
use valori_node::server::build_router;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::Duration;

#[tokio::test]
async fn test_replication_bootstrap() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init();

    // ── 1. Leader setup ───────────────────────────────────────────────────────
    let mut leader_config = NodeConfig::default();
    leader_config.max_records = 100;
    leader_config.dim = 4;
    leader_config.max_nodes = 100;
    leader_config.max_edges = 100;
    leader_config.wal_path = Some(std::env::temp_dir().join("leader_boot_wal.log"));

    let leader_state = Arc::new(Mutex::new(Engine::new(&leader_config)));

    {
        let mut engine = leader_state.lock().await;
        let log_path = std::env::temp_dir().join("leader_boot_events.log");
        let _ = std::fs::remove_file(&log_path);

        use valori_node::events::{EventCommitter, EventJournal};
        use valori_node::events::event_log::EventLogWriter;

        let log_writer = EventLogWriter::open(&log_path, Some(4))
            .expect("Failed to open leader event log");
        let journal = EventJournal::new();
        let state_clone = engine.state.clone();
        engine.event_committer = Some(EventCommitter::new(log_writer, journal, state_clone));
    }

    let leader_app = build_router(leader_state.clone(), None, None);
    let leader_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let leader_addr = leader_listener.local_addr().unwrap();
    let leader_url = format!("http://{}", leader_addr);

    tokio::spawn(async move {
        axum::serve(leader_listener, leader_app).await.unwrap();
    });

    // ── 2. Populate leader with 10 records ────────────────────────────────────
    let client = reqwest::Client::new();
    for _i in 0..10 {
        let resp = client
            .post(format!("{}/records", leader_url))
            .json(&serde_json::json!({ "values": [0.1f32, 0.2, 0.3, 0.4] }))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success(), "Leader insert failed");
    }
    tracing::info!("Leader populated with 10 records");

    // ── 3. Snapshot leader ────────────────────────────────────────────────────
    client
        .post(format!("{}/v1/snapshot/save", leader_url))
        .send()
        .await
        .expect("Snapshot trigger failed");
    tracing::info!("Leader snapshot triggered");

    // ── 4. Fresh follower ─────────────────────────────────────────────────────
    let mut follower_config = NodeConfig::default();
    follower_config.max_records = 100;
    follower_config.dim = 4;
    follower_config.max_nodes = 100;
    follower_config.max_edges = 100;
    follower_config.mode = NodeMode::Follower { leader_url: leader_url.clone() };

    let follower_state = Arc::new(Mutex::new(Engine::new(&follower_config)));

    {
        let mut engine = follower_state.lock().await;
        let log_path = std::env::temp_dir().join("follower_boot_events.log");
        let _ = std::fs::remove_file(&log_path);

        use valori_node::events::{EventCommitter, EventJournal};
        use valori_node::events::event_log::EventLogWriter;

        let log_writer = EventLogWriter::open(&log_path, Some(4))
            .expect("Failed to open follower event log");
        let journal = EventJournal::new();
        let state_clone = engine.state.clone();
        engine.event_committer = Some(EventCommitter::new(log_writer, journal, state_clone));
    }

    let f_state = follower_state.clone();
    let f_url = leader_url.clone();
    tokio::spawn(async move {
        valori_node::replication::run_follower_loop(f_state, f_url).await;
    });

    // ── 5. Wait for bootstrap via snapshot ────────────────────────────────────
    tracing::info!("Waiting for follower bootstrap (5 s)...");
    tokio::time::sleep(Duration::from_secs(5)).await;

    let count = {
        let engine = follower_state.lock().await;
        engine.state.record_count()
    };
    assert_eq!(count, 10, "Follower should have bootstrapped 10 records from snapshot");

    // ── 6. Verify live replication continues ──────────────────────────────────
    client
        .post(format!("{}/records", leader_url))
        .json(&serde_json::json!({ "values": [0.9f32, 0.8, 0.7, 0.6] }))
        .send()
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_secs(2)).await;

    let count_after = {
        let engine = follower_state.lock().await;
        engine.state.record_count()
    };
    assert_eq!(count_after, 11, "Follower should have replicated the new event (11 total)");
}
