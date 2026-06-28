// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `valori setup` — interactive cluster wizard.
//!
//! On first run: guides through architecture, node count, cluster start.
//! On subsequent runs: detects saved projects in `~/.valori/projects.json`
//! and offers to resume any of them (or create a new one).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use comfy_table::{Cell, Color, Table};
use dialoguer::{Confirm, Input, Select};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};

use valori_consensus::types::ValoriNode;
use valori_consensus::NullAuditSink;
use valori_node::cluster::{bootstrap_cluster, ClusterConfig, ClusterHandle};
use valori_node::cluster_server::serve_cluster_api;

use super::cluster as cluster_cmd;

// ── Persistent project config ─────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Default)]
struct ValoriConfig {
    projects: Vec<SavedProject>,
}

#[derive(Serialize, Deserialize, Clone)]
struct SavedProject {
    name: String,
    created_at: String,
    n_nodes: usize,
    base_api_port: u16,
    base_raft_port: u16,
}

fn valori_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".valori")
}

fn load_config() -> ValoriConfig {
    let path = valori_dir().join("projects.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
        .unwrap_or_default()
}

fn save_config(cfg: &ValoriConfig) {
    let dir = valori_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(dir.join("projects.json"), json);
    }
}

// ── Node setup tracking ───────────────────────────────────────────────────────

struct NodeSetup {
    node_id: u64,
    handle: ClusterHandle,
    api_url: String,
    _http_task: tokio::task::JoinHandle<()>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn run(bind_host: &str) -> Result<()> {
    print_header();

    if bind_host == "0.0.0.0" {
        println!("  ⚠  Binding to 0.0.0.0 — ALL API ports will be reachable from the network.");
        println!("     The cluster starts WITHOUT authentication or mTLS.");
        println!("     On a cloud VM (EC2, GCP, Azure) any internet host can read/write your data.");
        println!("     Restrict access with firewall/security-group rules BEFORE starting,");
        println!("     or use 127.0.0.1 (default) for local development.");
        println!();
    }

    let mut config = load_config();

    // ── Existing projects? ────────────────────────────────────────────────────
    let project: SavedProject = if config.projects.is_empty() {
        prompt_new_cluster(&mut config).await?
    } else {
        let mut items: Vec<String> = config
            .projects
            .iter()
            .map(|p| {
                format!(
                    "{}   ({} nodes · api :{}-:{} · {})",
                    p.name,
                    p.n_nodes,
                    p.base_api_port,
                    p.base_api_port + p.n_nodes as u16 - 1,
                    p.created_at,
                )
            })
            .collect();
        items.push("Create a new Project".to_string());

        let last = items.len() - 1;
        let choice = tokio::task::spawn_blocking(move || {
            Select::new()
                .with_prompt("  Existing projects — pick one or create new")
                .items(&items)
                .default(0)
                .interact()
        })
        .await??;

        if choice == last {
            prompt_new_cluster(&mut config).await?
        } else {
            let p = config.projects[choice].clone();
            println!("  Resuming project \"{}\"", p.name);
            println!();
            p
        }
    };

    start_cluster(project, bind_host, &mut config).await
}

// ── New-cluster setup prompts ─────────────────────────────────────────────────

async fn prompt_new_cluster(config: &mut ValoriConfig) -> Result<SavedProject> {
    println!("  ── New project ────────────────────────────────");
    println!();

    // Name first — makes it clear a new project is being created.
    let name = tokio::task::spawn_blocking(|| {
        Input::<String>::new()
            .with_prompt("  Project name")
            .default("my-cluster".into())
            .interact_text()
    })
    .await??;

    let arch = tokio::task::spawn_blocking(|| {
        Select::new()
            .with_prompt("  Architecture")
            .items(&[
                "Multi-node  (Raft consensus — fault-tolerant, recommended)",
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

    let base_api: u16 = 51000;
    let base_raft: u16 = 51100;

    // Show planned topology
    println!();
    println!("  Planned topology:");
    let mut plan = Table::new();
    plan.set_header(vec!["node", "api port", "raft port", "role"]);
    for i in 0..n_nodes {
        let role = if i == 0 { "bootstrap → leader" } else { "voter" };
        plan.add_row(vec![
            Cell::new(i + 1),
            Cell::new(base_api + i as u16),
            Cell::new(base_raft + i as u16),
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
        std::process::exit(0);
    }
    println!();

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    let project = SavedProject { name, created_at: now, n_nodes, base_api_port: base_api, base_raft_port: base_raft };

    // Persist before starting (so a partial start doesn't lose the record)
    config.projects.retain(|p| p.name != project.name);
    config.projects.push(project.clone());
    save_config(config);
    println!("  Project saved to ~/.valori/projects.json");
    println!();

    Ok(project)
}

// ── Cluster start ─────────────────────────────────────────────────────────────

async fn start_cluster(project: SavedProject, bind_host: &str, _config: &mut ValoriConfig) -> Result<()> {
    let SavedProject { n_nodes, base_api_port, base_raft_port, .. } = project;

    let members: BTreeMap<u64, ValoriNode> = (0..n_nodes)
        .map(|i| {
            (
                (i + 1) as u64,
                ValoriNode {
                    raft_addr: format!("127.0.0.1:{}", base_raft_port + i as u16),
                    api_addr:  format!("127.0.0.1:{}", base_api_port  + i as u16),
                },
            )
        })
        .collect();

    // Start all nodes (init=false; we call initialize after all gRPC servers are up)
    let mut setups: Vec<NodeSetup> = Vec::new();
    for i in 0..n_nodes {
        let node_id  = (i + 1) as u64;
        let api_port  = base_api_port  + i as u16;
        let raft_port = base_raft_port + i as u16;

        let pb = spinner(&format!(
            "Starting node {}  (api=:{} raft=:{})",
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

        let handle = bootstrap_cluster(&cfg, Box::new(NullAuditSink), 0)
            .await
            .with_context(|| format!("node {node_id} failed — is port {raft_port} free?"))?;

        let api_bind = format!("{bind_host}:{api_port}");
        let (_, task) = serve_cluster_api(&handle, &api_bind, None)
            .await
            .with_context(|| format!("port {api_port} already in use — run `lsof -i :{api_port}` to find the process"))?;

        pb.finish_with_message(format!(
            "✓  node {node_id}   http://{bind_host}:{api_port}"
        ));

        setups.push(NodeSetup {
            node_id,
            handle,
            api_url: format!("http://127.0.0.1:{api_port}"),
            _http_task: task,
        });
    }

    // Initialize Raft (all gRPC listeners are up)
    {
        let pb = spinner("Initializing Raft consensus");
        setups[0]
            .handle
            .raft
            .initialize(members)
            .await
            .map_err(|e| anyhow::anyhow!("cluster init failed: {e}"))?;
        pb.finish_with_message("✓  Raft consensus initialized");
    }

    // Wait for leader election
    let pb = spinner("Waiting for leader election");
    let leader_url = find_leader(&setups, Duration::from_secs(15)).await?;
    let leader_id = setups
        .iter()
        .find(|s| s.api_url == leader_url)
        .map(|s| s.node_id)
        .unwrap_or(1);
    pb.finish_with_message(format!("✓  Leader: node {leader_id}  ({})", leader_url));
    println!();

    // Show live status
    cluster_cmd::status(&leader_url).ok();

    // Operations menu
    menu_loop(leader_url, &mut setups, base_api_port, base_raft_port, bind_host).await
}

// ── Operations menu ───────────────────────────────────────────────────────────

async fn menu_loop(
    mut leader_url: String,
    setups: &mut Vec<NodeSetup>,
    base_api_port: u16,
    base_raft_port: u16,
    bind_host: &str,
) -> Result<()> {
    const ITEMS: &[&str] = &[
        "Insert a vector",
        "Search  (k-nearest neighbours)",
        "Cluster status",
        "Add another node  (start one more node on this machine)",
        "Grow cluster      (join a node already running elsewhere)",
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
            2 => { cluster_cmd::status(&leader_url).ok(); }
            3 => {
                add_local_node(setups, base_api_port, base_raft_port, bind_host).await?;
                // Re-read leader in case it changed after membership update.
                if let Ok(url) = find_leader(setups, Duration::from_secs(5)).await {
                    leader_url = url;
                }
                cluster_cmd::status(&leader_url).ok();
            }
            4 => add_remote_node(&leader_url, setups).await?,
            _ => {
                println!("  Shutting down cluster...");
                for s in setups.iter() {
                    s.handle.raft.shutdown().await.ok();
                }
                println!("  Done. Goodbye.");
                return Ok(());
            }
        }
    }
}

// ── Insert ────────────────────────────────────────────────────────────────────

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
            Ok(r) => r.into_json::<serde_json::Value>().map_err(|e| anyhow::anyhow!("{e}")),
            Err(ureq::Error::Status(c, r)) => {
                Err(anyhow::anyhow!("HTTP {c}: {}", r.into_json::<serde_json::Value>().unwrap_or_default()))
            }
            Err(e) => Err(anyhow::anyhow!("{e}")),
        }
    })
    .await??;

    println!("  ✅  inserted  id={}  log_index={}", result["id"], result["log_index"]);
    Ok(())
}

// ── Search ────────────────────────────────────────────────────────────────────

async fn search_vectors(setups: &[NodeSetup]) -> Result<()> {
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
            Ok(r) => r.into_json::<serde_json::Value>().map_err(|e| anyhow::anyhow!("{e}")),
            Err(ureq::Error::Status(c, r)) => {
                Err(anyhow::anyhow!("HTTP {c}: {}", r.into_json::<serde_json::Value>().unwrap_or_default()))
            }
            Err(e) => Err(anyhow::anyhow!("{e}")),
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
                let cell = if rank == 0 { Cell::new(rank + 1).fg(Color::Green) } else { Cell::new(rank + 1) };
                t.add_row(vec![cell, Cell::new(h["id"].to_string()), Cell::new(h["distance_sq"].to_string())]);
            }
            println!("{t}");
        }
    }
    Ok(())
}

// ── Add another local node ────────────────────────────────────────────────────

/// Start one more node in-process and join it to the running cluster.
async fn add_local_node(
    setups: &mut Vec<NodeSetup>,
    base_api_port: u16,
    base_raft_port: u16,
    bind_host: &str,
) -> Result<()> {
    let next_id = setups.iter().map(|s| s.node_id).max().unwrap_or(0) + 1;
    let api_port  = base_api_port  + (next_id - 1) as u16;
    let raft_port = base_raft_port + (next_id - 1) as u16;

    let pb = spinner(&format!(
        "Starting node {}  (api=:{} raft=:{})",
        next_id, api_port, raft_port
    ));

    // Build the new node's ValoriNode descriptor.
    let new_node = ValoriNode {
        raft_addr: format!("127.0.0.1:{raft_port}"),
        api_addr:  format!("127.0.0.1:{api_port}"),
    };

    // Members map for the new node's config (it needs to know all peers to connect).
    let members: BTreeMap<u64, ValoriNode> = setups
        .iter()
        .map(|s| {
            let rp = base_raft_port + (s.node_id - 1) as u16;
            let ap = base_api_port  + (s.node_id - 1) as u16;
            (s.node_id, ValoriNode {
                raft_addr: format!("127.0.0.1:{rp}"),
                api_addr:  format!("127.0.0.1:{ap}"),
            })
        })
        .chain([(next_id, new_node.clone())])
        .collect();

    let cfg = ClusterConfig {
        node_id: next_id,
        raft_bind: format!("127.0.0.1:{raft_port}"),
        members,
        init: false,
        raft_log_path: None,
        tls: None,
    };

    let handle = bootstrap_cluster(&cfg, Box::new(NullAuditSink), 0)
        .await
        .with_context(|| format!("node {next_id} failed — is port {raft_port} free?"))?;

    let api_bind = format!("{bind_host}:{api_port}");
    let (_, task) = serve_cluster_api(&handle, &api_bind, None)
        .await
        .with_context(|| format!("port {api_port} in use — run `lsof -i :{api_port}`"))?;

    pb.finish_with_message(format!("✓  node {next_id} started"));

    // Join: learner catch-up, then promote to voter.
    let leader_id = setups
        .iter()
        .find_map(|s| s.handle.raft.metrics().borrow().current_leader)
        .unwrap_or(1);

    if let Some(leader) = setups.iter().find(|s| s.node_id == leader_id) {
        let pb2 = spinner("Joining cluster (learner → voter)");

        leader.handle.raft
            .add_learner(next_id, new_node, true)
            .await
            .map_err(|e| anyhow::anyhow!("add_learner failed: {e}"))?;

        let voters: std::collections::BTreeSet<u64> = setups
            .iter()
            .map(|s| s.node_id)
            .chain([next_id])
            .collect();
        leader.handle.raft
            .change_membership(voters, false)
            .await
            .map_err(|e| anyhow::anyhow!("change_membership failed: {e}"))?;

        pb2.finish_with_message(format!("✓  node {next_id} joined as voter"));
    }

    setups.push(NodeSetup {
        node_id: next_id,
        handle,
        api_url: format!("http://127.0.0.1:{api_port}"),
        _http_task: task,
    });

    println!("  Cluster is now {} nodes.", setups.len());
    Ok(())
}

// ── Join a remote node ────────────────────────────────────────────────────────

async fn add_remote_node(leader_url: &str, setups: &[NodeSetup]) -> Result<()> {
    let next_suggested = setups.iter().map(|s| s.node_id).max().unwrap_or(0) + 1;
    println!("  This joins a Valori node that is ALREADY RUNNING on another machine.");
    println!("  Start it there first, then enter its addresses below.");
    println!();

    let (id, api_addr) = tokio::task::spawn_blocking(move || -> Result<(u64, String)> {
        let id = Input::<u64>::new()
            .with_prompt("  New node id")
            .default(next_suggested)
            .interact_text()?;
        let api = loop {
            let raw = Input::<String>::new()
                .with_prompt("  API address of that node (host:port, e.g. 10.0.0.4:51000)")
                .interact_text()?;
            if raw.contains(':') {
                break raw;
            }
            eprintln!("  ⚠  Must be host:port (e.g. 10.0.0.4:51000)");
        };
        Ok((id, api))
    })
    .await??;

    // Auto-derive the Raft address: same host, port + 100.
    let raft_default = derive_raft_addr(&api_addr);
    let raft_addr = tokio::task::spawn_blocking(move || {
        Input::<String>::new()
            .with_prompt("  Raft address (auto-derived — press Enter or edit)")
            .default(raft_default)
            .interact_text()
    })
    .await??;

    cluster_cmd::add_node(leader_url, id, &raft_addr, &api_addr)
}

/// Derive raft address from an API address: same host, port + 100.
fn derive_raft_addr(api_addr: &str) -> String {
    if let Some((host, port_str)) = api_addr.rsplit_once(':') {
        if let Ok(port) = port_str.parse::<u16>() {
            return format!("{}:{}", host, port + 100);
        }
    }
    String::new()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

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
            "no leader within {}s — check that ports are free",
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

fn parse_floats(raw: &str) -> Result<Vec<f32>> {
    raw.split(',')
        .map(|s| s.trim().parse::<f32>().map_err(|_| anyhow::anyhow!("'{s}' is not a float")))
        .collect()
}

fn print_header() {
    println!();
    println!("  ╔══════════════════════════════════════╗");
    println!("  ║        Valori  Cluster  Setup        ║");
    println!("  ║    forensic vector database  v{}    ║", env!("CARGO_PKG_VERSION"));
    println!("  ╚══════════════════════════════════════╝");
    println!();
}
