use comfy_table::presets::UTF8_FULL;
use comfy_table::{ContentArrangement, Table};
use valori_node::events::event_log::LogEntry;

pub fn run(events_path: &str) -> anyhow::Result<()> {
    let file_bytes = std::fs::read(events_path)?;
    if file_bytes.len() < 16 {
        println!("\n⚠️  WARNING: Event log is empty or corrupt.\n");
        return Ok(());
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["Event ID", "Type", "Details"]);

    let mut offset = 16;
    let mut current_id = 0;

    while offset < file_bytes.len() {
        match bincode::serde::decode_from_slice::<LogEntry, _>(
            &file_bytes[offset..],
            bincode::config::standard()
        ) {
            Ok((entry, bytes_read)) => {
                match entry {
                    LogEntry::Event(event) => {
                        let details = match &event {
                            valori_kernel::event::KernelEvent::InsertRecord { id, tag, .. } => {
                                format!("Record {} (Tag: {})", id.0, tag)
                            }
                            valori_kernel::event::KernelEvent::CreateNode { id, kind, .. } => {
                                format!("Node {} ({:?})", id.0, kind)
                            }
                            valori_kernel::event::KernelEvent::CreateEdge { id, from, to, kind } => {
                                format!("Edge {} ({} -> {}) [{:?}]", id.0, from.0, to.0, kind)
                            }
                            _ => String::new(),
                        };
                        table.add_row(vec![
                            current_id.to_string(),
                            event.event_type().to_string(),
                            details,
                        ]);
                        current_id += 1;
                    }
                    LogEntry::Checkpoint { event_count, .. } => {
                        table.add_row(vec![
                            "-".to_string(),
                            "Checkpoint".to_string(),
                            format!("Count: {}", event_count),
                        ]);
                    }
                }
                offset += bytes_read;
            }
            Err(e) => {
                println!("\n⚠️  WARNING: Decoding stopped at offset {} due to error: {}\n", offset, e);
                break;
            }
        }
    }

    println!("\nEvent Timeline (from Phase 23 Event Log)\n");
    println!("{table}\n");

    Ok(())
}
