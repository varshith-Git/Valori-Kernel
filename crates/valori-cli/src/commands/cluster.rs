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
