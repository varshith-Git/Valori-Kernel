// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Divergence detection integration test.
//!
//! Leader → Follower replication → corrupt follower state directly → assert
//! the divergence-check loop detects it and auto-heals.
use valori_node::config::{NodeConfig, NodeMode};
use valori_node::engine::Engine;
use valori_node::server::build_router;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::Duration;

#[tokio::test]
async fn test_replication_divergence() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init();

    // ── 1. Leader ─────────────────────────────────────────────────────────────
    let mut leader_config = NodeConfig::default();
    leader_config.max_records = 100;
    leader_config.dim = 4;
    leader_config.max_nodes = 100;
    leader_config.max_edges = 100;
    leader_config.wal_path = Some(std::env::temp_dir().join("leader_div_wal.log"));

    let leader_state = Arc::new(RwLock::new(Engine::new(&leader_config)));

    {
        let mut engine = leader_state.write().await;
        let log_path = std::env::temp_dir().join("leader_div_events.log");
        let _ = std::fs::remove_file(&log_path);

        use valori_node::events::{EventCommitter, EventJournal};
        use valori_node::events::event_log::EventLogWriter;

        let log_writer = EventLogWriter::open(&log_path, Some(4))
            .expect("Failed to open leader event log");
        let journal = EventJournal::new();
        let state_clone = engine.state.clone();
        engine.persistence = valori_node::commit::Persistence::EventLog(EventCommitter::new(log_writer, journal, state_clone));
    }

    let leader_app = build_router(leader_state.clone(), None, None);
    let leader_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let leader_addr = leader_listener.local_addr().unwrap();
    let leader_url = format!("http://{}", leader_addr);

    tracing::info!("Leader running at {}", leader_url);
    tokio::spawn(async move {
        axum::serve(leader_listener, leader_app).await.unwrap();
    });

    // ── 2. Follower ───────────────────────────────────────────────────────────
    let mut follower_config = NodeConfig::default();
    follower_config.max_records = 100;
    follower_config.dim = 4;
    follower_config.max_nodes = 100;
    follower_config.max_edges = 100;
    follower_config.mode = NodeMode::Follower { leader_url: leader_url.clone() };

    let follower_state = Arc::new(RwLock::new(Engine::new(&follower_config)));

    {
        let mut engine = follower_state.write().await;
        let log_path = std::env::temp_dir().join("follower_div_events.log");
        let _ = std::fs::remove_file(&log_path);

        use valori_node::events::{EventCommitter, EventJournal};
        use valori_node::events::event_log::EventLogWriter;

        let log_writer = EventLogWriter::open(&log_path, Some(4))
            .expect("Failed to open follower event log");
        let journal = EventJournal::new();
        let state_clone = engine.state.clone();
        engine.persistence = valori_node::commit::Persistence::EventLog(EventCommitter::new(log_writer, journal, state_clone));
    }

    let follower_app = build_router(follower_state.clone(), None, None);
    let follower_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let follower_addr = follower_listener.local_addr().unwrap();
    let follower_api_url = format!("http://{}", follower_addr);

    tracing::info!("Follower running at {}", follower_api_url);
    tokio::spawn(async move {
        axum::serve(follower_listener, follower_app).await.unwrap();
    });

    let f_state = follower_state.clone();
    let f_url = leader_url.clone();
    tokio::spawn(async move {
        valori_node::replication::run_follower_loop(f_state, f_url).await;
    });

    // ── 3. Insert record into leader ──────────────────────────────────────────
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/records", leader_url))
        .json(&serde_json::json!({ "values": [0.1, 0.2, 0.3, 0.4] }))
        .send()
        .await
        .unwrap();

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap();
        panic!("Leader insert failed: {} — {}", status, text);
    }

    let body: serde_json::Value = resp.json().await.unwrap();
    let record_id_val = body["id"].as_u64().unwrap();
    tracing::info!("Inserted record {} into leader", record_id_val);

    // ── 4. Wait for follower to sync ──────────────────────────────────────────
    tokio::time::sleep(Duration::from_secs(2)).await;

    // ── 5. Corrupt follower by deleting the record from its local state ───────
    tracing::info!("Corrupting follower: deleting record {}", record_id_val);
    {
        let mut engine = follower_state.write().await;
        use valori_kernel::state::command::Command;
        use valori_kernel::types::id::RecordId;

        let cmd = Command::DeleteRecord {
            id: RecordId(record_id_val as u32),
        };
        engine.state.apply(&cmd).unwrap();
    }

    // ── 6. Wait for divergence loop to fire (runs every 5 s) ─────────────────
    tracing::info!("Waiting for divergence detection (6 s)...");
    tokio::time::sleep(Duration::from_secs(6)).await;

    // ── 7. Poll follower until healed ─────────────────────────────────────────
    let mut healed = false;
    for attempt in 0..20 {
        let state_resp = reqwest::get(format!("{}/v1/replication/state", follower_api_url))
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap();

        let status = state_resp["status"].as_str().unwrap_or("Unknown");
        tracing::info!("Follower status [{}]: {}", attempt, status);

        if status == "Synced" || status == "Unknown" {
            healed = true;
            tracing::info!("Follower healed — status: {}", status);
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    assert!(healed, "Follower did not auto-heal within 20 s timeout");
}
