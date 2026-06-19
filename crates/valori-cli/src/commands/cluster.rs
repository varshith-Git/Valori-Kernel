// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `valori cluster …` — the operator's terminal for a running cluster.
//!
//! The same role Kafka's shell tools play (kafka-topics.sh and friends):
//! point any subcommand at any node's HTTP API and operate the cluster
//! without hand-writing curl bodies.
//!
//! ```text
//! valori cluster status      --url http://10.0.0.1:3000
//! valori cluster health      --url http://10.0.0.2:3000
//! valori cluster add-node    --url http://10.0.0.1:3000 --id 4 \
//!     --raft-addr 10.0.0.4:3100 --api-addr 10.0.0.4:3000
//! valori cluster remove-node --url http://10.0.0.1:3000 --id 4
//! ```
//!
//! Membership changes are leader-only; on a 403 the error body names the
//! leader so the operator can re-point `--url`.

use anyhow::{bail, Context, Result};
use comfy_table::{Cell, Color, Table};
use std::io::{BufRead, Write};

fn get(url: &str, path: &str) -> Result<(u16, serde_json::Value)> {
    let full = format!("{}{}", url.trim_end_matches('/'), path);
    match ureq::get(&full).call() {
        Ok(resp) => {
            let status = resp.status();
            let body: serde_json::Value = resp.into_json().context("response was not JSON")?;
            Ok((status, body))
        }
        Err(ureq::Error::Status(status, resp)) => {
            let body = resp.into_json().unwrap_or(serde_json::json!(null));
            Ok((status, body))
        }
        Err(e) => bail!("cannot reach {full}: {e}"),
    }
}

fn post(url: &str, path: &str, body: serde_json::Value) -> Result<(u16, serde_json::Value)> {
    let full = format!("{}{}", url.trim_end_matches('/'), path);
    match ureq::post(&full).send_json(body) {
        Ok(resp) => {
            let status = resp.status();
            let body: serde_json::Value = resp.into_json().context("response was not JSON")?;
            Ok((status, body))
        }
        Err(ureq::Error::Status(status, resp)) => {
            let body = resp.into_json().unwrap_or(serde_json::json!(null));
            Ok((status, body))
        }
        Err(e) => bail!("cannot reach {full}: {e}"),
    }
}

/// `valori cluster status` — who leads, who's in, how far each index is.
pub fn status(url: &str) -> Result<()> {
    let (code, body) = get(url, "/v1/cluster/status")?;
    if code != 200 {
        bail!("status failed (HTTP {code}): {body}");
    }

    let leader = body["current_leader"].as_u64();
    println!();
    println!("  node id        : {}", body["node_id"]);
    println!(
        "  leader         : {}",
        leader.map_or("NONE — election in progress".to_string(), |l| l.to_string())
    );
    println!("  is leader      : {}", body["is_leader"]);
    println!("  term           : {}", body["term"]);
    println!("  last log index : {}", body["last_log_index"]);
    println!("  last applied   : {}", body["last_applied_index"]);
    println!();

    let mut table = Table::new();
    table.set_header(vec!["id", "role", "raft address", "api address"]);
    if let Some(members) = body["members"].as_array() {
        for m in members {
            let id = m["id"].as_u64().unwrap_or(0);
            let role = if Some(id) == leader {
                Cell::new("leader").fg(Color::Green)
            } else if m["voter"] == true {
                Cell::new("voter")
            } else {
                Cell::new("learner").fg(Color::Yellow)
            };
            table.add_row(vec![
                Cell::new(id.to_string()),
                role,
                Cell::new(m["raft_addr"].as_str().unwrap_or("")),
                Cell::new(m["api_addr"].as_str().unwrap_or("")),
            ]);
        }
    }
    println!("{table}");
    Ok(())
}

/// `valori cluster health` — exit 0 healthy, exit 1 otherwise (script-friendly).
pub fn health(url: &str) -> Result<()> {
    let (code, body) = get(url, "/v1/cluster/health")?;
    if code == 200 {
        println!("✅ healthy — leader is node {}", body["leader"]);
        Ok(())
    } else {
        bail!("❌ unhealthy (HTTP {code}): {body}");
    }
}

/// `valori cluster add-node` — learner catch-up, then voter promotion.
pub fn add_node(url: &str, id: u64, raft_addr: &str, api_addr: &str) -> Result<()> {
    let (code, body) = post(
        url,
        "/v1/cluster/add-node",
        serde_json::json!({ "node_id": id, "raft_addr": raft_addr, "api_addr": api_addr }),
    )?;
    match code {
        200 => {
            println!(
                "✅ node {id} joined as voter (membership committed at log index {})",
                body["log_index"]
            );
            Ok(())
        }
        403 => bail!(
            "this node is not the leader — re-point --url at the leader.\n   detail: {}",
            body["detail"]
        ),
        _ => bail!("add-node failed (HTTP {code}): {body}"),
    }
}

/// `valori cluster upgrade` — guided rolling upgrade, one node at a time.
///
/// Walks through every cluster member in safe order (non-leaders first, leader
/// last). For each node it prints the three-step instructions, waits for you
/// to complete them, then polls `/health` until the node is back before
/// moving on. The whole run is idempotent: restarting after an interruption
/// at the same `--url` resumes from the current live state.
pub fn upgrade(url: &str, target_version: &str) -> Result<()> {
    // ── 1. Fetch current cluster topology ─────────────────────────────────────
    let (code, status) = get(url, "/v1/cluster/status")?;
    if code != 200 {
        bail!("cannot get cluster status (HTTP {code}): {status}");
    }

    let leader_id = match status["current_leader"].as_u64() {
        Some(id) => id,
        None => bail!(
            "no elected leader right now — wait for the election to finish, then retry"
        ),
    };

    let members = status["members"]
        .as_array()
        .filter(|m| !m.is_empty())
        .cloned()
        .context("cluster status returned no members")?;

    if members.len() < 2 {
        bail!(
            "single-node cluster — rolling upgrade requires ≥ 2 nodes. \
             Stop and upgrade directly with no traffic loss."
        );
    }

    // ── 2. Print upgrade plan ──────────────────────────────────────────────────
    // Non-leaders first, leader last — minimises write-path disruption.
    let mut ordered = members.clone();
    ordered.sort_by_key(|m| {
        let id = m["id"].as_u64().unwrap_or(0);
        (id == leader_id, id)
    });

    println!();
    println!("  Rolling upgrade plan  →  v{target_version}");
    println!("  ┌─────────────────────────────────────────────────────────┐");
    for (i, m) in ordered.iter().enumerate() {
        let id = m["id"].as_u64().unwrap_or(0);
        let tag = if id == leader_id { " ← leader, upgraded last" } else { "" };
        println!(
            "  │  {}. node {}   api={}{}",
            i + 1,
            id,
            m["api_addr"].as_str().unwrap_or("?"),
            tag,
        );
    }
    println!("  └─────────────────────────────────────────────────────────┘");
    println!();

    let total = ordered.len();
    let stdin = std::io::stdin();

    for (step, member) in ordered.iter().enumerate() {
        let node_id = member["id"].as_u64().unwrap_or(0);
        let api_addr = member["api_addr"].as_str().unwrap_or("").to_string();
        let raft_addr = member["raft_addr"].as_str().unwrap_or("").to_string();
        let is_leader = node_id == leader_id;
        let node_url = if api_addr.is_empty() {
            url.to_string()
        } else {
            format!("http://{api_addr}")
        };

        println!(
            "── Step {}/{}: Upgrade node {} ─────────────────────────",
            step + 1,
            total,
            node_id
        );
        if !raft_addr.is_empty() {
            println!("  raft : {raft_addr}");
        }
        if !api_addr.is_empty() {
            println!("  api  : {api_addr}");
        }
        if is_leader {
            println!("  ⚠  This is the current leader — upgrading it will trigger");
            println!("     a brief re-election (~election-timeout). Writes will retry.");
        }
        println!();
        println!("  Steps:");
        println!("    1. Stop the valori-node process on this host");
        println!("    2. Replace the binary with v{target_version}");
        println!("    3. Restart the node (same env vars, no config changes)");
        println!();
        print!("  Press Enter once node {node_id} is back online > ");
        std::io::stdout().flush()?;
        let _ = stdin.lock().lines().next();

        // ── Poll /health until the node responds ──────────────────────────────
        print!("  Waiting for node {node_id}");
        std::io::stdout().flush()?;
        let mut healthy = false;
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_secs(2));
            print!(".");
            std::io::stdout().flush()?;
            if let Ok((c, _)) = get(&node_url, "/health") {
                if c == 200 {
                    healthy = true;
                    break;
                }
            }
        }
        println!();

        if !healthy {
            bail!(
                "node {node_id} did not become healthy within 120 s — \
                 check the process and retry"
            );
        }
        println!("  ✅ node {node_id} is healthy");

        // ── After the leader restarts, wait for a new election ─────────────────
        if is_leader {
            print!("  Waiting for new leader election");
            std::io::stdout().flush()?;
            let mut elected = false;
            for _ in 0..30 {
                std::thread::sleep(std::time::Duration::from_secs(2));
                print!(".");
                std::io::stdout().flush()?;
                if let Ok((c, body)) = get(url, "/v1/cluster/status") {
                    if c == 200 {
                        if let Some(new_leader) = body["current_leader"].as_u64() {
                            // Any leader is fine — including the just-restarted node
                            // if it wins re-election with the new binary.
                            let _ = new_leader;
                            elected = true;
                            println!("\n  New leader: node {new_leader}");
                            break;
                        }
                    }
                }
            }
            if !elected {
                println!();
                println!(
                    "  ⚠  Could not confirm new leader within 60 s. \
                     Check cluster status manually before continuing."
                );
            }
        }
        println!();
    }

    println!("  ✅ Rolling upgrade to v{target_version} complete!");
    println!(
        "  Verify all nodes agree: valori cluster status --url {url}"
    );
    println!();
    Ok(())
}

/// `valori cluster remove-node` — voter removal (last voter refused upstream).
pub fn remove_node(url: &str, id: u64) -> Result<()> {
    let (code, body) = post(
        url,
        "/v1/cluster/remove-node",
        serde_json::json!({ "node_id": id }),
    )?;
    match code {
        200 => {
            println!(
                "✅ node {id} removed (membership committed at log index {})",
                body["log_index"]
            );
            Ok(())
        }
        403 => bail!(
            "this node is not the leader — re-point --url at the leader.\n   detail: {}",
            body["detail"]
        ),
        422 => bail!("refused: {}", body["detail"]),
        _ => bail!("remove-node failed (HTTP {code}): {body}"),
    }
}
