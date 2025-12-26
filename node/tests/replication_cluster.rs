use valori_node::engine::Engine;
use valori_node::server::build_router;
use std::sync::Arc;
use tokio::sync::Mutex;
use tempfile::tempdir;
use tokio::time::{sleep, Duration};

// Integration test for Leader-Follower Replication Cluster
#[tokio::test]
async fn test_replication_cluster() {
    // Init tracing
    let _ = tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init();

    // ----------------------------------------------------------------
    // 1. Start LEADER Node
    // ----------------------------------------------------------------
    let leader_dir = tempdir().unwrap();
    let leader_wal = leader_dir.path().join("wal.log");
    let leader_events = leader_dir.path().join("events.log");
    
    let leader_config = valori_node::config::NodeConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        wal_path: Some(leader_wal.clone()),
        event_log_path: Some(leader_events.clone()),
        mode: valori_node::config::NodeMode::Leader,
        max_records: 128,
        dim: 4,
        max_nodes: 128,
        max_edges: 256,
        ..Default::default()
    };
    
    // Reduced dimensions to avoid stack overflow
    let leader_engine = valori_node::engine::Engine::<128, 4, 128, 256>::new(&leader_config);
    let leader_state = Arc::new(Mutex::new(leader_engine));
    
    // Insert Initial Data into Leader
    {
        let mut engine = leader_state.lock().await;
        // Verify EventCommitter is active
        assert!(engine.event_committer.is_some(), "Leader must have event committer");
        
        // Insert Record 0
        let vec = vec![0.1; 4];
        let id0 = engine.insert_record_from_f32(&vec).unwrap();
        assert_eq!(id0, 0);
    }
    
    let leader_app = build_router(leader_state.clone(), None);
    let leader_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let leader_addr = leader_listener.local_addr().unwrap();
    let leader_url = format!("http://{}", leader_addr);
    
    tokio::spawn(async move {
        axum::serve(leader_listener, leader_app).await.unwrap();
    });
    
    println!("Leader running at {}", leader_url);
    
    // ----------------------------------------------------------------
    // 2. Start FOLLOWER Node
    // ----------------------------------------------------------------
    let follower_dir = tempdir().unwrap();
    let follower_wal = follower_dir.path().join("wal.log");
    let follower_events = follower_dir.path().join("events.log");
    
    let follower_config = valori_node::config::NodeConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        wal_path: Some(follower_wal.clone()),
        event_log_path: Some(follower_events.clone()),
        mode: valori_node::config::NodeMode::Follower { leader_url: leader_url.clone() },
        max_records: 128,
        dim: 4,
        max_nodes: 128,
        max_edges: 256,
        ..Default::default()
    };
    
    let follower_engine = valori_node::engine::Engine::<128, 4, 128, 256>::new(&follower_config);
    let follower_state = Arc::new(Mutex::new(follower_engine));
    
    // Spawn Follower Loop
    let follower_state_clone = follower_state.clone();
    let leader_url_clone = leader_url.clone();
    tokio::spawn(async move {
        valori_node::replication::run_follower_loop(follower_state_clone, leader_url_clone).await;
    });
    
    // ----------------------------------------------------------------
    // 3. Verify Synchronization (Historical/Initial)
    // ----------------------------------------------------------------
    // Wait slightly for follower to connect and replicate
    sleep(Duration::from_secs(1)).await;
    
    {
        let engine = follower_state.lock().await;
        // Verify using Search (which uses Index!) to confirm side-effects worked
        let query = vec![0.1; 4];
        let hits = engine.search_l2(&query, 1).unwrap();
        assert!(!hits.is_empty(), "Follower should find Record 0 via index");
        assert_eq!(hits[0].0, 0, "Should match Record 0");
    }
    
    // ----------------------------------------------------------------
    // 4. Verify Live Replication
    // ----------------------------------------------------------------
    // Insert Record 1 into Leader
    {
        let mut engine = leader_state.lock().await;
        let vec = vec![0.2; 4];
        let id1 = engine.insert_record_from_f32(&vec).unwrap();
        assert_eq!(id1, 1);
        println!("Inserted Record 1 into Leader");
    }
    
    // Wait for propagation
    sleep(Duration::from_millis(500)).await;
    
    {
        let engine = follower_state.lock().await;
        // Verify Record 1
        let query = vec![0.2; 4];
        let hits = engine.search_l2(&query, 1).unwrap();
        assert!(!hits.is_empty(), "Follower should find Record 1 via index");
        assert_eq!(hits[0].0, 1, "Should match Record 1");
    }

    println!("Replication Cluster Test Passed!");
}
