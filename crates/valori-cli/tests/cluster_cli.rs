// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `valori cluster …` against a real running node — the operator harness
//! proven end to end: the compiled CLI binary talking HTTP to a live
//! Raft-backed server.

use std::process::Command;
use std::time::Duration;

use valori_consensus::types::ValoriNode;
use valori_node::cluster::{bootstrap_cluster, ClusterConfig};
use valori_node::cluster_server::build_cluster_router;

fn valori_bin() -> &'static str {
    env!("CARGO_BIN_EXE_valori")
}

/// Boot a single-node cluster and serve its HTTP API on a real port.
async fn serve_node() -> (String, tokio::task::JoinHandle<()>) {
    let cfg = ClusterConfig {
        node_id: 1,
        raft_bind: "127.0.0.1:0".into(),
        members: [(
            1,
            ValoriNode {
                api_addr: String::new(),
                raft_addr: String::new(),
            },
        )]
        .into_iter()
        .collect(),
        init: true,
        raft_log_path: None,
        tls: None,
        shard_count: 1,
    };
    let handle = bootstrap_cluster(&cfg, None, None, 0).await.unwrap();
    handle
        .raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader == Some(1), "self-elected")
        .await
        .unwrap();

    let app = build_cluster_router(&handle, None);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), task)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cli_status_and_health_against_a_live_node() {
    let (url, _task) = serve_node().await;

    // The CLI is a separate process — run it off the async runtime.
    let url2 = url.clone();
    let status_out = tokio::task::spawn_blocking(move || {
        Command::new(valori_bin())
            .args(["cluster", "status", "--url", &url2])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    let stdout = String::from_utf8_lossy(&status_out.stdout);
    assert!(
        status_out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&status_out.stderr)
    );
    assert!(stdout.contains("leader"), "{stdout}");
    assert!(stdout.contains("term"), "{stdout}");

    let health_out = tokio::task::spawn_blocking(move || {
        Command::new(valori_bin())
            .args(["cluster", "health", "--url", &url])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(health_out.status.success());
    assert!(String::from_utf8_lossy(&health_out.stdout).contains("healthy"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cli_health_exits_nonzero_when_unreachable() {
    let out = tokio::task::spawn_blocking(|| {
        Command::new(valori_bin())
            .args(["cluster", "health", "--url", "http://127.0.0.1:1"])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        !out.status.success(),
        "health against a dead node must exit non-zero (script-friendly)"
    );
}
