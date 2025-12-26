use valori_node::config::{NodeConfig, NodeMode};
use valori_node::engine::Engine;
use valori_node::server::build_router;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::Duration;

// Integration test for Offline Bootstrap
#[tokio::test]
async fn test_replication_bootstrap() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init();

    // 1. LEADER: Generate History
    let mut leader_config = NodeConfig::default();
    leader_config.max_records = 100;
    leader_config.dim = 4;
    leader_config.max_nodes = 100;
    leader_config.max_edges = 100;
    leader_config.wal_path = Some(std::env::temp_dir().join("leader_boot_wal.log"));
    let leader_state = Arc::new(Mutex::new(Engine::<100, 4, 100, 100>::new(&leader_config)));
    
    {
        let mut engine = leader_state.lock().await;
        let log_path = std::env::temp_dir().join("leader_boot_events.log");
        let _ = std::fs::remove_file(&log_path);
        
        use valori_node::events::{EventCommitter, EventJournal};
        use valori_node::events::event_log::EventLogWriter;
        
        let log_writer = EventLogWriter::open(&log_path).expect("Failed to open leader event log");
        let journal = EventJournal::new();
        engine.event_committer = Some(EventCommitter::new(log_writer, journal, engine.state.clone()));
    }

    let leader_app = build_router(leader_state.clone(), None);
    let leader_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let leader_addr = leader_listener.local_addr().unwrap();
    let leader_url = format!("http://{}", leader_addr);
    
    tokio::spawn(async move {
        axum::serve(leader_listener, leader_app).await.unwrap();
    });

    // 2. Populate Leader
    let client = reqwest::Client::new();
    for i in 0..10 {
        let vec = vec![0.1; 4];
        client.post(format!("{}/records", leader_url))
            .json(&serde_json::json!({
                "id": i,
                "values": vec,
            }))
            .send().await.unwrap();
    }
    
    tracing::info!("Leader populated with 10 records.");
    
    // Force Snapshot on Leader
    {
         client.post(format!("{}/v1/snapshot", leader_url))
            .send().await.expect("Snapshot trigger failed");
         tracing::info!("Leader snapshot triggered.");
    }
    
    // 3. Start FRESH FOLLOWER
    let mut follower_config = NodeConfig::default();
    follower_config.max_records = 100;
    follower_config.dim = 4;
    follower_config.max_nodes = 100;
    follower_config.max_edges = 100;
    follower_config.mode = NodeMode::Follower { leader_url: leader_url.clone() };
    
    let follower_state = Arc::new(Mutex::new(Engine::<100, 4, 100, 100>::new(&follower_config)));
    
    // Init Follower Log (Empty)
    {
        let mut engine = follower_state.lock().await;
        // Clean up any previous run's log
        let log_path = std::env::temp_dir().join("follower_boot_events.log");
        let _ = std::fs::remove_file(&log_path); 
        
        use valori_node::events::{EventCommitter, EventJournal};
        use valori_node::events::event_log::EventLogWriter;
        
        let log_writer = EventLogWriter::open(&log_path).expect("Failed to open follower event log");
        let journal = EventJournal::new();
        engine.event_committer = Some(EventCommitter::new(log_writer, journal, engine.state.clone()));
    }
    
    // Run Follower Loop
    let f_state = follower_state.clone();
    let f_url = leader_url.clone();
    tokio::spawn(async move {
        valori_node::replication::run_follower_loop(f_state, f_url).await;
    });
    
    // 4. Verify Immediate Sync (via Snapshot)
    // We expect the follower to contain 10 records.
    
    tracing::info!("Waiting for bootstrap...");
    tokio::time::sleep(Duration::from_secs(5)).await;
    
    let count = {
        let engine = follower_state.lock().await;
        engine.state.record_count()
    };
    
    assert_eq!(count, 10, "Follower failed to bootstrap 10 records from snapshot!");
    
    // 5. Verify it continues to replicate
    client.post(format!("{}/records", leader_url))
        .json(&serde_json::json!({
             "id": 99,
             "values": vec![0.9; 4],
        }))
        .send().await.unwrap();
        
    tokio::time::sleep(Duration::from_secs(2)).await;

    let count_after = {
        let engine = follower_state.lock().await;
        engine.state.record_count()
    };
    
    assert_eq!(count_after, 11, "Follower failed to replicate new events after bootstrap!");
}
