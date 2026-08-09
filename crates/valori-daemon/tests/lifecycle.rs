// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! End-to-end lifecycle test: the daemon actually spawns a real `valori-node`,
//! confirms it becomes healthy, then stops it. Requires the debug binary at
//! `target/debug/valori-node` (built by `cargo build -p valori-node`).

use valori_daemon::{
    ClusterConfig, Daemon, EmbeddingConfig, ProjectManifest, ProjectNode, StorageConfig,
};

/// Locate `target/debug/valori-node` relative to this crate.
fn node_binary() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/valori-node")
}

/// An OS-assigned free port, released immediately (standard test-only
/// best-effort allocation — same technique `PortAllocator` itself uses to
/// probe availability).
fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test]
async fn create_start_health_stop_delete() {
    let bin = node_binary();
    if !bin.exists() {
        eprintln!(
            "skipping: {} not built (run `cargo build -p valori-node`)",
            bin.display()
        );
        return;
    }
    std::env::set_var("VALORI_NODE_BIN", &bin);

    let home = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::new(home.path()).unwrap();

    // create
    let project = daemon
        .create_project(ProjectManifest {
            id: valori_daemon::new_id(),
            name: "healthcare".into(),
            dim: 8,
            index: "brute".into(),
            workspace: "default".into(),
            restart_policy: valori_daemon::RestartPolicy::Never,
            created_at: 0,
            last_opened_at: None,
            cluster: None,
            embedding: EmbeddingConfig::default(),
            storage: StorageConfig::default(),
        })
        .unwrap();
    assert!(project.dir.exists());

    // list shows it, stopped
    let listed = daemon.list_projects().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].1.status, valori_daemon::RuntimeState::Stopped);

    // start — real process spawn + health poll
    let info = daemon.start_project("healthcare").await.unwrap();
    assert_eq!(info.status, valori_daemon::RuntimeState::Running);
    assert!(info.pid.is_some(), "should have a PID");
    let port = info.port.expect("should have a port");

    // the node really answers on its allocated port
    let health = reqwest::get(format!("http://127.0.0.1:{port}/health"))
        .await
        .expect("node health request");
    assert!(health.status().is_success());

    // cannot delete while running
    assert!(daemon.delete_project("healthcare").is_err());

    // stop, then delete
    let stopped = daemon.stop_project("healthcare").await.unwrap();
    assert_eq!(stopped.status, valori_daemon::RuntimeState::Stopped);
    daemon.delete_project("healthcare").unwrap();
    assert!(daemon.list_projects().unwrap().is_empty());
}

/// The supervisor detects a crashed node and restarts it per RestartPolicy.
#[tokio::test]
async fn supervisor_restarts_crashed_node() {
    let bin = node_binary();
    if !bin.exists() {
        return;
    }
    std::env::set_var("VALORI_NODE_BIN", &bin);

    let home = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::new(home.path()).unwrap();
    daemon
        .create_project(ProjectManifest {
            id: valori_daemon::new_id(),
            name: "hc".into(),
            dim: 8,
            index: "brute".into(),
            workspace: "default".into(),
            restart_policy: valori_daemon::RestartPolicy::Always,
            created_at: 0,
            last_opened_at: None,
            cluster: None,
            embedding: EmbeddingConfig::default(),
            storage: StorageConfig::default(),
        })
        .unwrap();

    let info = daemon.start_project("hc").await.unwrap();
    let original_pid = info.pid.expect("pid");

    // Simulate a crash: kill the child out from under the daemon.
    let _ = std::process::Command::new("kill")
        .arg("-9")
        .arg(original_pid.to_string())
        .status();

    // Detecting the exit is a race against the OS reaping the killed process,
    // so poll ticks until it's seen rather than assuming a fixed tick catches
    // it. Once detected, the backoff for restart #0 is 2s, so wait it out,
    // then tick again until the restart lands.
    let mut restarted = 0;
    for _ in 0..20 {
        restarted = daemon.supervise_tick().await;
        if restarted > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    assert_eq!(restarted, 1, "supervisor should have restarted the node");

    // New process, restart count incremented, back to running.
    let (_p, status, sup) = daemon.project_detail("hc").unwrap();
    assert_eq!(status.status, valori_daemon::RuntimeState::Running);
    let new_pid = status.pid.expect("new pid");
    assert_ne!(new_pid, original_pid, "should be a fresh process");
    assert_eq!(sup.unwrap().restarts, 1);

    daemon.stop_project("hc").await.unwrap();
}

#[tokio::test]
async fn start_unknown_project_is_not_found() {
    // No node binary needed — fails at project lookup before any spawn.
    let home = tempfile::tempdir().unwrap();
    // Point at the real binary if present so Supervisor::new() succeeds; else skip.
    let bin = node_binary();
    if !bin.exists() {
        return;
    }
    std::env::set_var("VALORI_NODE_BIN", &bin);
    let mut daemon = Daemon::new(home.path()).unwrap();
    let err = daemon.start_project("ghost").await.unwrap_err();
    assert!(matches!(err, valori_daemon::DaemonError::NotFound(_)));
}

/// RFC-0007: a `replication: 3` project actually starts a working 3-node
/// Raft cluster (not just a single node), reports per-node health via
/// `project_cluster_status`, survives one node being killed (the daemon
/// respawns only that node, leaving its two peers alone), and shuts all
/// three down cleanly.
#[tokio::test]
async fn cluster_project_forms_and_recovers_from_one_node_crash() {
    let bin = node_binary();
    if !bin.exists() {
        eprintln!(
            "skipping: {} not built (run `cargo build -p valori-node`)",
            bin.display()
        );
        return;
    }
    std::env::set_var("VALORI_NODE_BIN", &bin);

    let home = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::new(home.path()).unwrap();

    let nodes = vec![
        ProjectNode {
            id: 1,
            http_port: free_port(),
            raft_port: Some(free_port()),
        },
        ProjectNode {
            id: 2,
            http_port: free_port(),
            raft_port: Some(free_port()),
        },
        ProjectNode {
            id: 3,
            http_port: free_port(),
            raft_port: Some(free_port()),
        },
    ];

    daemon
        .create_project(ProjectManifest {
            id: valori_daemon::new_id(),
            name: "trio".into(),
            dim: 8,
            index: "brute".into(),
            workspace: "default".into(),
            restart_policy: valori_daemon::RestartPolicy::Always,
            created_at: 0,
            last_opened_at: None,
            cluster: Some(ClusterConfig {
                replication: 3,
                nodes: nodes.clone(),
                shard_count: 1,
            }),
            embedding: EmbeddingConfig::default(),
            storage: StorageConfig::default(),
        })
        .unwrap();

    // start — should bring up all 3 processes and wait for all 3 to be healthy.
    let info = daemon.start_project("trio").await.unwrap();
    assert_eq!(info.status, valori_daemon::RuntimeState::Running);

    let cluster = daemon.project_cluster_status("trio").await.unwrap();
    assert_eq!(cluster["nodes_total"], serde_json::json!(3));
    assert_eq!(cluster["nodes_reachable"], serde_json::json!(3));

    // Each node individually answers its own /health.
    for n in &nodes {
        let health = reqwest::get(format!("http://127.0.0.1:{}/health", n.http_port))
            .await
            .expect("node health request");
        assert!(
            health.status().is_success(),
            "node {} should be healthy",
            n.id
        );
    }

    // Kill node 2 directly (simulating a crash) and confirm the daemon
    // respawns only that node — nodes 1 and 3 must never restart.
    let before = daemon.project_cluster_status("trio").await.unwrap();
    let node2_pid = before["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["node_id"] == serde_json::json!(2))
        .and_then(|n| n["pid"].as_u64())
        .expect("node 2 pid");
    let _ = std::process::Command::new("kill")
        .arg("-9")
        .arg(node2_pid.to_string())
        .status();

    let mut restarted = 0;
    for _ in 0..20 {
        restarted = daemon.supervise_tick().await;
        if restarted > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    assert_eq!(
        restarted, 1,
        "supervisor should have restarted exactly node 2"
    );

    // node 2 is healthy again, on the same port, with a new pid.
    let health2 = reqwest::get(format!("http://127.0.0.1:{}/health", nodes[1].http_port))
        .await
        .expect("node 2 health request after respawn");
    assert!(health2.status().is_success());

    let after = daemon.project_cluster_status("trio").await.unwrap();
    assert_eq!(after["nodes_reachable"], serde_json::json!(3));

    daemon.stop_project("trio").await.unwrap();
    for n in &nodes {
        assert!(
            !reqwest::get(format!("http://127.0.0.1:{}/health", n.http_port))
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false),
            "node {} should be stopped",
            n.id
        );
    }
}
