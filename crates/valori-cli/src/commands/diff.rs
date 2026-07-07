// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `valori diff` — compare kernel state between two event counts.
//!
//! Replays the event log twice from the same snapshot baseline — once to
//! `--from` and once to `--to` — then reports the state-hash delta and,
//! optionally, nearest-neighbour rank changes for a query vector.

use crate::engine::ForensicEngine;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table};
use std::collections::{HashMap, HashSet};
use valori_kernel::index::SearchResult;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

pub fn run(
    snapshot_path: &str,
    log_path:      &str,
    from_count:    u64,
    to_count:      u64,
    query_arg:     Option<String>,
    top_k:         usize,
) -> anyhow::Result<()> {
    // ── Engine A — state at `from` ────────────────────────────────────────────
    let mut engine_a = ForensicEngine::from_snapshot(snapshot_path)?;
    engine_a.replay_to(log_path, from_count)?;
    let hash_a   = engine_a.blake3_hex();
    let events_a: HashSet<u64> = engine_a.applied_events.iter().copied().collect();

    // ── Engine B — state at `to` ──────────────────────────────────────────────
    let mut engine_b = ForensicEngine::from_snapshot(snapshot_path)?;
    engine_b.replay_to(log_path, to_count)?;
    let hash_b = engine_b.blake3_hex();

    let state_changed = hash_a != hash_b;
    let status_label  = if state_changed { "DRIFTED" } else { "IDENTICAL" };

    // ── State comparison table ────────────────────────────────────────────────
    let mut cmp = Table::new();
    cmp.load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Property").add_attribute(Attribute::Bold),
            Cell::new(format!("Event #{from_count}  (A)")).add_attribute(Attribute::Bold),
            Cell::new(format!("Event #{to_count}  (B)")).add_attribute(Attribute::Bold),
        ]);

    cmp.add_row(vec![
        "Records",
        &engine_a.record_count().to_string(),
        &engine_b.record_count().to_string(),
    ]);
    cmp.add_row(vec![
        "Nodes",
        &engine_a.node_count().to_string(),
        &engine_b.node_count().to_string(),
    ]);
    cmp.add_row(vec![
        "Edges",
        &engine_a.state.edge_count().to_string(),
        &engine_b.state.edge_count().to_string(),
    ]);
    cmp.add_row(vec!["State hash (BLAKE3)", &hash_a, &hash_b]);

    println!("\nState Comparison");
    println!("{}", "─".repeat(46));
    println!("{cmp}");
    let status_str = if state_changed {
        format!("\x1b[33m{status_label}\x1b[0m") // yellow
    } else {
        format!("\x1b[32m{status_label}\x1b[0m") // green
    };
    println!("  Status: {status_str}\n");

    // ── Drift Analysis (only when states differ) ──────────────────────────────
    if state_changed {
        let new_events: Vec<u64> = engine_b
            .applied_events
            .iter()
            .filter(|e| !events_a.contains(e))
            .copied()
            .collect();

        if !new_events.is_empty() {
            let mut drift = Table::new();
            drift
                .load_preset(UTF8_FULL)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(vec![
                    Cell::new("Event #").add_attribute(Attribute::Bold),
                    Cell::new("Applied in B, absent in A").add_attribute(Attribute::Bold),
                ]);

            for eid in &new_events {
                drift.add_row(vec![
                    Cell::new(eid),
                    Cell::new("state-changing event not present in A"),
                ]);
            }

            println!("Drift Analysis  ({} new event(s))", new_events.len());
            println!("{}", "─".repeat(46));
            println!("{drift}\n");
        }
    }

    // ── Semantic diff (optional) ──────────────────────────────────────────────
    if let Some(query_str) = query_arg {
        let floats: Vec<f64> = serde_json::from_str(&query_str)
            .map_err(|_| anyhow::anyhow!(
                "Invalid --query value. Expected a JSON float array, e.g. '[0.1, 0.2]'. \
                 Got: {query_str}"
            ))?;

        let query_fxp = floats_to_fxp(&floats);
        let k         = top_k.max(1);

        let results_a = search(&engine_a, &query_fxp, k);
        let results_b = search(&engine_b, &query_fxp, k);

        let ranks_a = rank_map(&results_a);
        let ranks_b = rank_map(&results_b);

        let mut sem = Table::new();
        sem.load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![
                Cell::new("Record ID").add_attribute(Attribute::Bold),
                Cell::new("Change").add_attribute(Attribute::Bold),
                Cell::new("Detail").add_attribute(Attribute::Bold),
            ]);

        // Entered or shifted in B.
        for (&id, &rank_b) in &ranks_b {
            match ranks_a.get(&id) {
                Some(&rank_a) if rank_a != rank_b => {
                    sem.add_row(vec![
                        Cell::new(id),
                        Cell::new("~ Rank shift").fg(Color::Yellow),
                        Cell::new(format!("{} → {}", rank_a + 1, rank_b + 1)),
                    ]);
                }
                None => {
                    sem.add_row(vec![
                        Cell::new(id),
                        Cell::new("+ Entered top-K").fg(Color::Green),
                        Cell::new(format!("rank {}", rank_b + 1)),
                    ]);
                }
                _ => {} // Unchanged — skip.
            }
        }

        // Dropped from top-K in B.
        for (&id, &rank_a) in &ranks_a {
            if !ranks_b.contains_key(&id) {
                sem.add_row(vec![
                    Cell::new(id),
                    Cell::new("- Left top-K").fg(Color::Red),
                    Cell::new(format!("was rank {}", rank_a + 1)),
                ]);
            }
        }

        if sem.is_empty() {
            println!("Semantic Diff (top-{k}): no rank changes detected.\n");
        } else {
            println!("Semantic Diff  ·  top-{k}");
            println!("{}", "─".repeat(46));
            println!("{sem}\n");
        }
    }

    Ok(())
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Convert f64 values to Q16.16 fixed-point vectors.
/// Replicates `valori_kernel::fxp::ops::from_f32` without requiring the `std` feature.
fn floats_to_fxp(floats: &[f64]) -> FxpVector {
    FxpVector {
        data: floats
            .iter()
            .map(|&f| FxpScalar(
                (f as f32 * 65536.0)
                    .round()
                    .clamp(i32::MIN as f32, i32::MAX as f32) as i32,
            ))
            .collect(),
    }
}

fn search(engine: &ForensicEngine, query: &FxpVector, k: usize) -> Vec<SearchResult> {
    let mut buf = vec![SearchResult { id: RecordId(0), score: i64::MAX }; k];
    let found   = engine.kernel_state().search_l2(query, &mut buf, None);
    buf.truncate(found);
    buf
}

fn rank_map(results: &[SearchResult]) -> HashMap<u32, usize> {
    results
        .iter()
        .enumerate()
        .map(|(rank, r)| (r.id.0, rank))
        .collect()
}
