use comfy_table::presets::UTF8_FULL;
use comfy_table::{ContentArrangement, Table};
use valori_persistence::idx;

pub fn run(idx_path: &str) -> anyhow::Result<()> {
    let mut entries = idx::read_all(idx_path)?;
    
    // Check strict sequential order
    let is_sorted = entries.windows(2).all(|w| w[0].event_id < w[1].event_id);
    
    if !is_sorted {
        println!("\n⚠️  WARNING: Index is not sequential. Displaying logical timeline order.\n");
        entries.sort_by_key(|e| e.event_id);
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["ID", "Timestamp", "Label"]);

    for entry in entries {
        let ts = chrono::DateTime::from_timestamp(entry.timestamp as i64, 0)
            .unwrap_or_default()
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        
        table.add_row(vec![
            entry.event_id.to_string(),
            ts,
            entry.label,
        ]);
    }

    println!("\nEvent Timeline\n");
    println!("{table}\n");

    Ok(())
}
