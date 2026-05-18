use comfy_table::presets::UTF8_FULL;
use comfy_table::{ContentArrangement, Table};

use std::path::PathBuf;
use valori_persistence::{idx, snapshot};
use valori_kernel::event::KernelEvent;

use valori_node::events::event_log::LogEntry;

pub fn run(
    dir: Option<PathBuf>,
    snapshot_path_arg: Option<String>,
    wal_path_arg: Option<String>,
    idx_path_arg: Option<String>,
) -> anyhow::Result<()> {

    let (s_path, w_path, i_path) = match dir {
        Some(d) => (
            d.join("snapshot.val"),
            d.join("events.log"),
            d.join("metadata.idx"),
        ),
        None => (
            PathBuf::from(snapshot_path_arg.unwrap_or_else(|| "snapshot.val".to_string())),
            PathBuf::from(wal_path_arg.unwrap_or_else(|| "events.log".to_string())),
            PathBuf::from(idx_path_arg.unwrap_or_else(|| "metadata.idx".to_string())),
        ),
    };

    println!("\nValori Status Report");
    println!("--------------------");

    // Build Table
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["File", "Status", "Details"]);

    // 1. Snapshot Info
    if s_path.exists() {
        match snapshot::read_header(&s_path) {
            Ok(header) => {
                 let msg = format!(
                     "Format: V1, Magic: {:?}, Ver: {}, Idx: {}, Ts: {}", 
                    std::str::from_utf8(&header.magic).unwrap_or("BAD"),
                    header.version,
                    header.event_index,
                    chrono::DateTime::from_timestamp(header.timestamp as i64, 0)
                        .unwrap_or_default()
                        .to_rfc3339()
                 );
                 table.add_row(vec!["Snapshot", "FOUND", &msg]);
            },
            Err(e) => {
                table.add_row(vec!["Snapshot", "CORRUPT", &e.to_string()]);
            }
        }
    } else {
        table.add_row(vec!["Snapshot", "MISSING", ""]);
    }

    // 2. WAL Info (Phase 23 Event Log format)
    if w_path.exists() {
        if let Ok(file_bytes) = std::fs::read(&w_path) {
            if file_bytes.len() < 16 {
                table.add_row(vec!["WAL", "CORRUPT", "File smaller than 16-byte header"]);
            } else {
                let dim = u32::from_le_bytes(file_bytes[4..8].try_into().unwrap());
                let mut event_count = 0;
                let mut offset = 16;
                let mut corrupt = false;
                let mut err_msg = String::new();
                
                while offset < file_bytes.len() {
                    match bincode::serde::decode_from_slice::<LogEntry, _>(
                        &file_bytes[offset..],
                        bincode::config::standard()
                    ) {
                        Ok((entry, bytes_read)) => {
                            match entry {
                                LogEntry::Event(_) => event_count += 1,
                                LogEntry::Checkpoint { event_count: c, .. } => event_count = c,
                            }
                            offset += bytes_read;
                        }
                        Err(e) => {
                            corrupt = true;
                            err_msg = format!("Decoding failed at offset {} after {} events: {}", offset, event_count, e);
                            break;
                        }
                    }
                }
                
                if corrupt {
                    table.add_row(vec!["WAL", "CORRUPT", &err_msg]);
                } else {
                    table.add_row(vec!["WAL", "FOUND", &format!("{} events (dim {})", event_count, dim)]);
                }
            }
        } else {
            table.add_row(vec!["WAL", "ERROR", "Failed to read file"]);
        }
    } else {
        table.add_row(vec!["WAL", "MISSING", ""]);
    }

    // 3. IDX Info
    if i_path.exists() {
         match idx::read_all(&i_path) {
             Ok(entries) => {
                 table.add_row(vec!["Index", "FOUND", &format!("{} labeled entries", entries.len())]);
             }
             Err(e) => {
                 table.add_row(vec!["Index", "CORRUPT", &e.to_string()]);
             }
         }
    } else {
        table.add_row(vec!["Index", "MISSING", ""]);
    }

    println!("{table}\n");

    Ok(())
}
