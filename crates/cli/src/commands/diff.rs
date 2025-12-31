use crate::engine::ForensicEngine;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{ContentArrangement, Table};
use std::collections::HashSet;

pub fn run(snapshot_path: &str, wal_path: &str, from_index: u64, to_index: u64, query: Option<String>) -> anyhow::Result<()> {
    // 1. Engine A (From)
    let mut engine_a = ForensicEngine::new(snapshot_path)?;
    engine_a.replay_to(wal_path, from_index)?;
    let hash_a = engine_a.state.state_hash();
    let events_a: HashSet<u64> = engine_a.applied_events.iter().cloned().collect();

    // 2. Engine B (To) - FRESH INSTANCE
    let mut engine_b = ForensicEngine::new(snapshot_path)?;
    engine_b.replay_to(wal_path, to_index)?;
    let hash_b = engine_b.state.state_hash();
    // note: we iterate vector to find diff, HashSet mainly for quick lookup if needed, 
    // but since we want the *sequence* of drift, vector comparison or diffing is better.
    // However, prompt asks to identify events present in B but missing in A.
    
    // Status Determination
    let status = if hash_a == hash_b { "IDENTICAL" } else { "DRIFTED" };

    // Table 1: State Comparison
    let mut table1 = Table::new();
    table1
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["Property", "Value"]);

    table1.add_row(vec!["From Index", &from_index.to_string()]);
    table1.add_row(vec!["From Hash", &format!("0x{:016x}", hash_a)]);
    table1.add_row(vec!["To Index", &to_index.to_string()]);
    table1.add_row(vec!["To Hash", &format!("0x{:016x}", hash_b)]);
    table1.add_row(vec!["Status", status]);

    println!("\nState Comparison");
    println!("----------------");
    println!("{table1}\n");

    // Table 2: Drift Analysis (Only if DRIFTED)
    if status == "DRIFTED" {
        let mut table2 = Table::new();
        table2
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec!["Event ID", "Context"]);

        // Find events in B not in A (Drift)
        // Since replay is monotonic and deterministic, B should be A + [drift] if to > from.
        if to_index > from_index {
             for event_id in &engine_b.applied_events {
                 if !events_a.contains(event_id) {
                     table2.add_row(vec![event_id.to_string(), "Inserted between tA and tB".to_string()]);
                 }
             }
        } else {
             // If to < from (Reverse diff), show what's in A but not B
             // The prompt usually implies forward diff, but let's handle it or just show B-A.
             // Prompt says: "Identify Events present in B but missing in A".
             for event_id in &engine_b.applied_events {
                 if !events_a.contains(event_id) {
                     table2.add_row(vec![event_id.to_string(), "Present in B, Missing in A".to_string()]);
                 }
             }
        }

        println!("Drift Analysis");
        println!("--------------");
        println!("{table2}\n");
    }

    // Semantic Diff (If Query is provided)
    if let Some(query_str) = query {
        let query_vec: Vec<i32> = serde_json::from_str(&query_str)
            .map_err(|_| anyhow::anyhow!("Invalid JSON query. Expected [x, y, z]"))?;

        let k = 5;
        let results_a = engine_a.state.search(&query_vec, k, None)?;
        let results_b = engine_b.state.search(&query_vec, k, None)?;

        let ranks_a = compute_rank_map(&results_a);
        let ranks_b = compute_rank_map(&results_b);

        let mut table3 = Table::new();
        table3
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec!["ID", "Change", "Detail"]);

        // Check for new or changed ranks (In B)
        for (id, rank_b) in &ranks_b {
            match ranks_a.get(id) {
                Some(rank_a) => {
                    if rank_a != rank_b {
                        table3.add_row(vec![id.to_string(), "~ Rank Change".to_string(), format!("{} -> {}", rank_a + 1, rank_b + 1)]);
                    }
                }
                None => {
                    table3.add_row(vec![id.to_string(), "+ Entered Top-K".to_string(), format!("Rank {}", rank_b + 1)]);
                }
            }
        }

        // Check for dropped (In A but not B)
        for (id, rank_a) in &ranks_a {
            if !ranks_b.contains_key(id) {
                 table3.add_row(vec![id.to_string(), "- Left Top-K".to_string(), format!("Was Rank {}", rank_a + 1)]);
            }
        }

        if !table3.is_empty() {
             println!("Semantic Diff (Top-{})", k);
             println!("--------------------");
             println!("{table3}\n");
        } else {
             println!("Semantic Diff: No Top-{} rank changes detected.\n", k);
        }
    }

    Ok(())
}

fn compute_rank_map(results: &[(u64, i64)]) -> std::collections::HashMap<u64, usize> {
    results.iter().enumerate().map(|(rank, (id, _))| (*id, rank)).collect()
}

