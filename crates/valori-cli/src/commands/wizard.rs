// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `valori setup` — interactive cluster wizard.
//!
//! Guides the operator through architecture choice, node count, and cluster
//! startup, then drops into a live operations menu (insert, search, status).
//! All nodes run in-process on a shared tokio runtime; for production
//! deployments use docker-compose or the `valori node` environment variables.

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Context, Result};
use comfy_table::{Cell, Color, Table};
use dialoguer::{Confirm, Input, Select};
use indicatif::{ProgressBar, ProgressStyle};

use valori_consensus::types::ValoriNode;
use valori_consensus::NullAuditSink;
use valori_node::cluster::{bootstrap_cluster, ClusterConfig, ClusterHandle};
use valori_node::cluster_server::serve_cluster_api;

use super::cluster as cluster_cmd;

const BASE_API: u16 = 3000;
const BASE_RAFT: u16 = 3100;

struct NodeSetup {
    node_id: u64,
    handle: ClusterHandle,
    api_url: String,
    _http_task: tokio::task::JoinHandle<()>,
}

pub async fn run() -> Result<()> {
    print_header();

    // ── Architecture ─────────────────────────────────────────────────────────
    let arch = tokio::task::spawn_blocking(|| {
        Select::new()
            .with_prompt("  Architecture")
            .items(&[
                "Multi-node  (Raft consensus — fault-tolerant, default)",
                "Single-node (standalone — simplest, no replication)",
            ])
            .default(0)
            .interact()
    })
    .await??;

    let n_nodes: usize = if arch == 0 {
        tokio::task::spawn_blocking(|| {
            Input::<usize>::new()
                .with_prompt("  Number of nodes")
                .default(3)
                .interact_text()
        })
        .await??
    } else {
        1
    };

    if n_nodes == 0 || n_nodes > 7 {
        anyhow::bail!("node count must be 1–7");
    }

    // ── Show planned topology ────────────────────────────────────────────────
    println!();
    println!("  Planned topology:");
    let mut plan = Table::new();
    plan.set_header(vec!["node", "api port", "raft port", "role"]);
    for i in 0..n_nodes {
        let role = if i == 0 { "bootstrap (will elect leader)" } else { "voter" };
        plan.add_row(vec![
            Cell::new(i + 1),
            Cell::new(BASE_API + i as u16),
            Cell::new(BASE_RAFT + i as u16),
            Cell::new(role),
        ]);
    }
    println!("{plan}");

    let go = tokio::task::spawn_blocking(|| {
        Confirm::new()
            .with_prompt("  Start cluster?")
            .default(true)
            .interact()
    })
    .await??;
    if !go {
        println!("  Aborted.");
        return Ok(());
    }
    println!();

    // ── Build the full members map (all addresses known upfront) ─────────────
    let members: BTreeMap<u64, ValoriNode> = (0..n_nodes)
        .map(|i| {
            (
                (i + 1) as u64,
                ValoriNode {
                    raft_addr: format!("127.0.0.1:{}", BASE_RAFT + i as u16),
                    api_addr: format!("127.0.0.1:{}", BASE_API + i as u16),
                },
            )
        })
        .collect();

    // ── Start every node (init=false; we call initialize after all are up) ───
    let mut setups: Vec<NodeSetup> = Vec::new();
    for i in 0..n_nodes {
        let node_id = (i + 1) as u64;
        let api_port = BASE_API + i as u16;
        let raft_port = BASE_RAFT + i as u16;

        let pb = spinner(&format!(
            "  Starting node {}  (api=:{} raft=:{}) ...",
            node_id, api_port, raft_port
        ));

        let cfg = ClusterConfig {
            node_id,
            raft_bind: format!("127.0.0.1:{raft_port}"),
            members: members.clone(),
            init: false,
            raft_log_path: None,
            tls: None,
        };

        let handle = bootstrap_cluster(&cfg, Box::new(NullAuditSink))
            .await
            .with_context(|| format!("node {node_id} failed to start"))?;

        let api_bind = format!("127.0.0.1:{api_port}");
        let (_, task) = serve_cluster_api(&handle, &api_bind, None)
            .await
            .with_context(|| {
                format!(
                    "port {api_port} already in use — stop other Valori processes and retry"
                )
            })?;

        pb.finish_with_message(format!(
            "  ✓  node {node_id}   api=http://127.0.0.1:{api_port}  raft=:{}",
            raft_port
        ));

        setups.push(NodeSetup {
            node_id,
            handle,
            api_url: format!("http://127.0.0.1:{api_port}"),
            _http_task: task,
        });
    }

    // ── Initialize the Raft cluster on node 1 (all gRPC servers now up) ──────
    {
        let pb = spinner("  Initializing Raft cluster ...");
        setups[0]
            .handle
            .raft
            .initialize(members)
            .await
            .map_err(|e| anyhow::anyhow!("cluster init failed: {e}"))?;
        pb.finish_with_message("  ✓  Raft cluster initialized");
    }

    // ── Wait for a leader to be elected ──────────────────────────────────────
    let pb = spinner("  Waiting for leader election ...");
    let leader_url = find_leader(&setups, Duration::from_secs(15)).await?;
    let leader_id = setups
        .iter()
        .find(|s| s.api_url == leader_url)
        .map(|s| s.node_id)
        .unwrap_or(1);
    pb.finish_with_message(format!(
        "  ✓  Leader: node {leader_id}  ({})",
        leader_url
    ));
    println!();

    // ── Show live cluster status ──────────────────────────────────────────────
    cluster_cmd::status(&leader_url).ok();

    // ── Operations menu ───────────────────────────────────────────────────────
    menu_loop(leader_url, &setups).await
}

// ── Helpers ──────────────────────────────────────────────────────────────────

async fn find_leader(setups: &[NodeSetup], timeout: Duration) -> Result<String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        for s in setups {
            if let Some(lid) = s.handle.raft.metrics().borrow().current_leader {
                if let Some(ls) = setups.iter().find(|x| x.node_id == lid) {
                    return Ok(ls.api_url.clone());
                }
            }
        }
        anyhow::ensure!(
            tokio::time::Instant::now() < deadline,
            "no leader elected within {}s — check that ports are free",
            timeout.as_secs()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

// ── Menu ─────────────────────────────────────────────────────────────────────

async fn menu_loop(leader_url: String, setups: &[NodeSetup]) -> Result<()> {
    const ITEMS: &[&str] = &[
        "Insert a vector",
        "Search  (k-nearest neighbours)",
        "Cluster status",
        "Add an external node",
        "Exit",
    ];

    loop {
        println!();
        let choice = tokio::task::spawn_blocking(|| {
            Select::new()
                .with_prompt("  What next?")
                .items(ITEMS)
                .default(0)
                .interact()
        })
        .await??;
        println!();

        match choice {
            0 => insert_vector(&leader_url).await?,
            1 => search_vectors(setups).await?,
            2 => {
                cluster_cmd::status(&leader_url).ok();
            }
            3 => add_node_prompt(&leader_url).await?,
            _ => {
                println!("  Shutting down cluster...");
                for s in setups {
                    s.handle.raft.shutdown().await.ok();
                }
                println!("  Done. Goodbye.");
                return Ok(());
            }
        }
    }
}

async fn insert_vector(leader_url: &str) -> Result<()> {
    let raw = tokio::task::spawn_blocking(|| {
        Input::<String>::new()
            .with_prompt("  Values (comma-separated floats, e.g. 1.0, 2.5, -0.3)")
            .interact_text()
    })
    .await??;

    let values = parse_floats(&raw)?;
    let url = format!("{}/records", leader_url.trim_end_matches('/'));

    let result = tokio::task::spawn_blocking(move || {
        match ureq::post(&url).send_json(serde_json::json!({ "values": values })) {
            Ok(r) => r
                .into_json::<serde_json::Value>()
                .map_err(|e| anyhow::anyhow!("bad response: {e}")),
            Err(ureq::Error::Status(code, r)) => {
                let b = r.into_json::<serde_json::Value>().unwrap_or_default();
                Err(anyhow::anyhow!("HTTP {code}: {b}"))
            }
            Err(e) => Err(anyhow::anyhow!("network: {e}")),
        }
    })
    .await??;

    println!(
        "  ✅  inserted  id={}  log_index={}",
        result["id"], result["log_index"]
    );
    Ok(())
}

async fn search_vectors(setups: &[NodeSetup]) -> Result<()> {
    // Search is read-only and served on any node — use node 1 for simplicity.
    let any_url = setups.first().map(|s| s.api_url.clone()).unwrap_or_default();

    let (raw, k) = tokio::task::spawn_blocking(|| -> Result<(String, usize)> {
        let raw = Input::<String>::new()
            .with_prompt("  Query vector (comma-separated floats)")
            .interact_text()?;
        let k = Input::<usize>::new()
            .with_prompt("  k (results to return)")
            .default(5)
            .interact_text()?;
        Ok((raw, k))
    })
    .await??;

    let query = parse_floats(&raw)?;
    let url = format!("{}/search", any_url.trim_end_matches('/'));

    let body = tokio::task::spawn_blocking(move || {
        match ureq::post(&url).send_json(serde_json::json!({ "query": query, "k": k })) {
            Ok(r) => r
                .into_json::<serde_json::Value>()
                .map_err(|e| anyhow::anyhow!("bad response: {e}")),
            Err(ureq::Error::Status(code, r)) => {
                let b = r.into_json::<serde_json::Value>().unwrap_or_default();
                Err(anyhow::anyhow!("HTTP {code}: {b}"))
            }
            Err(e) => Err(anyhow::anyhow!("network: {e}")),
        }
    })
    .await??;

    match body["hits"].as_array() {
        None => println!("  (no records in cluster yet)"),
        Some(hits) if hits.is_empty() => println!("  (no records in cluster yet)"),
        Some(hits) => {
            let mut t = Table::new();
            t.set_header(vec!["rank", "record id", "distance²"]);
            for (rank, h) in hits.iter().enumerate() {
                let is_top = rank == 0;
                let rank_cell = if is_top {
                    Cell::new(rank + 1).fg(Color::Green)
                } else {
                    Cell::new(rank + 1)
                };
                t.add_row(vec![
                    rank_cell,
                    Cell::new(h["id"].to_string()),
                    Cell::new(h["distance_sq"].to_string()),
                ]);
            }
            println!("{t}");
        }
    }
    Ok(())
}

async fn add_node_prompt(leader_url: &str) -> Result<()> {
    println!("  Add an external node (must already be running with its Raft server up).");
    let (id, raft_addr, api_addr) =
        tokio::task::spawn_blocking(|| -> Result<(u64, String, String)> {
            let id = Input::<u64>::new()
                .with_prompt("  New node id")
                .interact_text()?;
            let raft = Input::<String>::new()
                .with_prompt("  Raft address (host:port)")
                .interact_text()?;
            let api = Input::<String>::new()
                .with_prompt("  API address  (host:port, blank = none)")
                .default(String::new())
                .interact_text()?;
            Ok((id, raft, api))
        })
        .await??;

    cluster_cmd::add_node(leader_url, id, &raft_addr, &api_addr)
}

fn parse_floats(raw: &str) -> Result<Vec<f32>> {
    raw.split(',')
        .map(|s| {
            s.trim()
                .parse::<f32>()
                .map_err(|_| anyhow::anyhow!("'{s}' is not a float"))
        })
        .collect()
}

fn print_header() {
    println!();
    println!("  ╔══════════════════════════════════════╗");
    println!("  ║        Valori  Cluster  Setup        ║");
    println!("  ║   forensic vector database — v{}   ║", env!("CARGO_PKG_VERSION"));
    println!("  ╚══════════════════════════════════════╝");
    println!();
}
