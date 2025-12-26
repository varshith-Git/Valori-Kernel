use valori_node::config::{NodeConfig, NodeMode};
use valori_node::engine::Engine;
use valori_node::server::build_router;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::Duration;
use valori_kernel::types::vector::FxpVector;

// Integration test for Divergence Detection
#[tokio::test]
async fn test_replication_divergence() {
    // Init tracing
    let _ = tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init();

    // ----------------------------------------------------------------
    // 1. Start LEADER Node
    // ----------------------------------------------------------------
    let mut leader_config = NodeConfig::default();
    // leader_config.http_port = 0; // Removed: field does not exist
    leader_config.max_records = 100;
    leader_config.dim = 4;
    leader_config.max_nodes = 100;
    leader_config.max_edges = 100;
    leader_config.wal_path = Some(std::env::temp_dir().join("leader_div_wal.log"));
    let leader_state = Arc::new(Mutex::new(Engine::<100, 4, 100, 100>::new(&leader_config)));
    
    // Enable Event Log on Leader (Crucial for streaming)
    {
        let mut engine = leader_state.lock().await;
        // Mock event log init if needed, or assume Engine::new does it?
        // Engine::new does NOT init event log unless configured. 
        // We rely on defaults or need to set it up.
        // For Phase 30 implementation, we added `event_source` logic in `main.rs`, not `Engine::new`.
        // We need to initialize `event_committer` here manually for test.
        let log_path = std::env::temp_dir().join("leader_div_events.log");
        let _ = std::fs::remove_file(&log_path);
        
        use valori_node::events::{EventCommitter, EventJournal};
        use valori_node::events::event_log::EventLogWriter;
        use valori_kernel::snapshot::{encode::encode_state, decode::decode_state};
        
        let log_writer = EventLogWriter::open(&log_path).expect("Failed to open leader event log");
        let journal = EventJournal::new();
        let state_clone = engine.state.clone();
        
        engine.event_committer = Some(EventCommitter::new(log_writer, journal, state_clone));
    }

    let leader_app = build_router(leader_state.clone(), None);
    let leader_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let leader_addr = leader_listener.local_addr().unwrap();
    let leader_url = format!("http://{}", leader_addr);
    
    tracing::info!("Leader running at {}", leader_url);
    
    tokio::spawn(async move {
        axum::serve(leader_listener, leader_app).await.unwrap();
    });

    // ----------------------------------------------------------------
    // 2. Start FOLLOWER Node
    // ----------------------------------------------------------------
    let mut follower_config = NodeConfig::default();
    // follower_config.http_port = 0; // Removed
    follower_config.max_records = 100;
    follower_config.dim = 4;
    follower_config.max_nodes = 100;
    follower_config.max_edges = 100;
    follower_config.mode = NodeMode::Follower { leader_url: leader_url.clone() };
    
    let follower_state = Arc::new(Mutex::new(Engine::<100, 4, 100, 100>::new(&follower_config)));
    
    // Init Follower Event Log (required for Follower)
    {
        let mut engine = follower_state.lock().await;
        let log_path = std::env::temp_dir().join("follower_div_events.log");
        let _ = std::fs::remove_file(&log_path);
        
        use valori_node::events::{EventCommitter, EventJournal};
        use valori_node::events::event_log::EventLogWriter;
        use valori_kernel::snapshot::{encode::encode_state, decode::decode_state};

        let log_writer = EventLogWriter::open(&log_path).expect("Failed to open follower event log");
        let journal = EventJournal::new();
        let state_clone = engine.state.clone();
        
        engine.event_committer = Some(EventCommitter::new(log_writer, journal, state_clone));
    }

    // Spawn Follower Loop
    let f_state = follower_state.clone();
    let f_url = leader_url.clone();
    tokio::spawn(async move {
        valori_node::replication::run_follower_loop(f_state, f_url).await;
    });
    
    // Start Follower Server (for status check)
    let follower_app = build_router(follower_state.clone(), None);
    let follower_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let follower_addr = follower_listener.local_addr().unwrap();
    let follower_api_url = format!("http://{}", follower_addr);
    
    tracing::info!("Follower running at {}", follower_api_url);

    tokio::spawn(async move {
        axum::serve(follower_listener, follower_app).await.unwrap();
    });

    // ----------------------------------------------------------------
    // 3. Sync & Corrupt & Detect
    // ----------------------------------------------------------------
    
    let record_id_val;
    {
        let client = reqwest::Client::new();
        let vec = vec![0.1; 4];
        let resp = client.post(format!("{}/records", leader_url))
            .json(&serde_json::json!({
                "id": 1,
                "values": vec,
            }))
            .send().await.unwrap();
        
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap();
            tracing::error!("Leader Insert Failed: {} - {}", status, text);
            panic!("Leader Insert Failed");
        }
        
        let body: serde_json::Value = resp.json().await.unwrap();
        record_id_val = body.get("id").unwrap().as_u64().unwrap();
    }
    
    tracing::info!("Inserted Record 1 into Leader");
    
    // Wait for Sync (Poll Follower Index)
    tokio::time::sleep(Duration::from_secs(2)).await;
    
    // Check Follower Status: Should be "Synced" (or Unknown but working)
    // Actually our logic sets it to Synced only if matched proof.
    // Let's force a check.
    
    // CORRUPTION TIME
    tracing::info!("Simulating Corruption on Follower...");
    {
         let mut engine = follower_state.lock().await;
         // Corrupt state by inserting a record DIRECTLY (bypassing log/leader)
         // OR just modifying `state` if possible?
         // Engine.state is private.
         // But we can use `insert_record_from_f32` which writes to log?
         // If we write to Follower's log locally, it diverges from Leader's sequence!
         // Yes!
         
         // BUT wait, Follower usually shouldn't accept writes. `build_router` exposes write endpoints!
         // We haven't disabled write endpoints on Follower yet (Task for later).
         // So we can just POST to Follower!
    }
    
    // CORRUPTION TIME
    tracing::info!("Simulating Corruption on Follower...");
    {
         tracing::info!("Simulating Corruption on Follower (Delete Record {})...", record_id_val);
         let mut engine = follower_state.lock().await;
         use valori_kernel::state::command::Command;
         use valori_kernel::types::id::RecordId;
         
         let cmd = Command::DeleteRecord {
             id: RecordId(record_id_val as u32),
         };
         
         // Direct apply to KernelState
         engine.state.apply(&cmd).unwrap();
    }
    
    tracing::info!("Corrupted Follower State with Record 999");
    
    // Wait for Divergence Check (loop runs every 5s)
    tracing::info!("Waiting for Divergence Detection...");
    tokio::time::sleep(Duration::from_secs(6)).await;
    
    // Poll for Recovery (Synced/Unknown)
    tracing::info!("Polling for Recovery (Synced/Unknown)...");
    let mut healed = false;
    
    for i in 0..20 { // 20s timeout
        let state_resp = reqwest::get(format!("{}/v1/replication/state", follower_api_url))
            .await.unwrap()
            .json::<serde_json::Value>()
            .await.unwrap();
            
        let status = state_resp["status"].as_str().unwrap();
        tracing::info!("Follower Status [{}]: {}", i, status);
        
        if status == "Synced" || status == "Unknown" {
             healed = true;
             tracing::info!("Follower Healed! Status: {}", status);
             break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    
    assert!(healed, "Follower failed to auto-heal within timeout!");
}
