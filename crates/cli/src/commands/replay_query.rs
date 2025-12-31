use crate::engine::ForensicEngine;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{ContentArrangement, Table};

pub fn run(snapshot_path: &str, wal_path: &str, target_index: u64, query_arg: Option<String>) -> anyhow::Result<()> {
    let mut engine = ForensicEngine::new(snapshot_path)?;
    
    // Check if target is before snapshot
    if target_index < engine.snapshot_index {
        println!("\n⚠️  WARNING: Target index ({}) is older than snapshot ({})", target_index, engine.snapshot_index);
        println!("Cannot time travel backwards without an older snapshot.\n");
        return Ok(());
    }

    let replayed = engine.replay_to(wal_path, target_index)?;
    
    // Check for partial replay
    if engine.current_index < target_index {
         println!("\n⚠️  WARNING: Reached end of WAL before target index.");
         println!("State fast-forwarded to last available event: {}\n", engine.current_index);
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["Metric", "Value"]);

    table.add_row(vec!["Snapshot Index", &engine.snapshot_index.to_string()]);
    table.add_row(vec!["Target Index", &target_index.to_string()]);
    table.add_row(vec!["Current Index (Final)", &engine.current_index.to_string()]);
    table.add_row(vec!["Replayed Events", &replayed.to_string()]);
    table.add_row(vec!["State Hash (Mock)", &format!("{:016x}", engine.state.state_hash())]);
    table.add_row(vec!["Query Status", if query_arg.is_some() { "Executed" } else { "Skipped" }]);

    println!("\nSimulation Report");
    println!("-----------------");
    println!("{table}\n");

    if let Some(query_str) = query_arg {
        let query_vec: Vec<i32> = serde_json::from_str(&query_str)
            .map_err(|_| anyhow::anyhow!("Invalid JSON query. Expected [x, y, z]"))?;
        
        // Hardcoded K=5 for now
        let results = engine.state.search(&query_vec, 5, None)?;
        
        let mut table_results = Table::new();
        table_results
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec!["Rank", "ID", "Distance"]);

        for (rank, (id, dist)) in results.iter().enumerate() {
            table_results.add_row(vec![
                (rank + 1).to_string(),
                id.to_string(),
                dist.to_string(),
            ]);
        }

        println!("Search Results (Top-5)");
        println!("----------------------");
        println!("{table_results}\n");
    }

    Ok(())
}
