// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `valori replay-query` — time-travel to a specific event count and query.

use crate::engine::ForensicEngine;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table};
use std::time::Instant;
use valori_kernel::index::SearchResult;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

pub fn run(
    snapshot_path: &str,
    log_path:      &str,
    target_count:  u64,
    query_arg:     Option<String>,
    top_k:         usize,
) -> anyhow::Result<()> {
    // ── Restore baseline ─────────────────────────────────────────────────────
    let mut engine = ForensicEngine::from_snapshot(snapshot_path)?;

    // ── Replay ───────────────────────────────────────────────────────────────
    let t0 = Instant::now();
    let replayed = engine.replay_to(log_path, target_count)?;
    let elapsed  = t0.elapsed();

    if engine.current_event_count < target_count {
        println!(
            "\n⚠️  Reached end of event log before target event #{target_count}.\n\
             State is fast-forwarded to event #{}.\n",
            engine.current_event_count
        );
    }

    // ── State summary table ───────────────────────────────────────────────────
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Metric").add_attribute(Attribute::Bold),
            Cell::new("Value").add_attribute(Attribute::Bold),
        ]);

    table.add_row(vec!["Target event",    &target_count.to_string()]);
    table.add_row(vec!["Current event",   &engine.current_event_count.to_string()]);
    table.add_row(vec!["Events replayed", &replayed.to_string()]);
    table.add_row(vec!["Replay time",     &format!("{:.3} ms", elapsed.as_secs_f64() * 1000.0)]);
    table.add_row(vec!["Records",         &engine.record_count().to_string()]);
    table.add_row(vec!["Nodes",           &engine.node_count().to_string()]);
    table.add_row(vec!["Edges",           &engine.edge_count().to_string()]);
    table.add_row(vec!["State Hash (BLAKE3)", &engine.blake3_hex()]);

    println!("\nSimulation Report");
    println!("{}", "─".repeat(40));
    println!("{table}\n");

    // ── Optional search ───────────────────────────────────────────────────────
    if let Some(query_str) = query_arg {
        let floats: Vec<f64> = serde_json::from_str(&query_str)
            .map_err(|_| anyhow::anyhow!(
                "Invalid --query value. Expected a JSON float array, e.g. '[0.1, 0.2, 0.3]'. \
                 Got: {query_str}"
            ))?;

        let query_fxp = floats_to_fxp(&floats);
        let top_k     = top_k.max(1);
        let mut buf   = vec![SearchResult { id: RecordId(0), score: i64::MAX }; top_k];

        let qt = Instant::now();
        let found = engine.kernel_state().search_l2(&query_fxp, &mut buf, None);
        let query_ms = qt.elapsed().as_secs_f64() * 1000.0;

        buf.truncate(found);

        let mut res_table = Table::new();
        res_table
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![
                Cell::new("Rank").add_attribute(Attribute::Bold),
                Cell::new("Record ID").add_attribute(Attribute::Bold),
                Cell::new("L2 Distance").add_attribute(Attribute::Bold),
            ]);

        for (rank, r) in buf.iter().enumerate() {
            res_table.add_row(vec![
                Cell::new(rank + 1),
                Cell::new(r.id.0).fg(Color::Cyan),
                Cell::new(r.score),
            ]);
        }

        println!(
            "Search Results  ·  top-{}  ·  {:.3} ms",
            top_k, query_ms
        );
        println!("{}", "─".repeat(40));
        println!("{res_table}\n");
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
