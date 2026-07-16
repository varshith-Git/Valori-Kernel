// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `valori inspect` — structural status report for a database directory.

use crate::engine::{inspect_snapshot_bytes, parse_kernel_from_snapshot_bytes};
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table};
use std::path::PathBuf;
use valori_node::events::event_log::LogEntry;
use valori_wire::{decode_entry, parse_header};

const DEFAULT_SNAPSHOT: &str = "snapshot.val";
const DEFAULT_LOG: &str = "events.log";

pub fn run(
    dir: Option<PathBuf>,
    snapshot_arg: Option<String>,
    log_arg: Option<String>,
) -> anyhow::Result<()> {
    let (s_path, w_path) = match &dir {
        Some(d) => (d.join(DEFAULT_SNAPSHOT), d.join(DEFAULT_LOG)),
        None => (
            PathBuf::from(snapshot_arg.as_deref().unwrap_or(DEFAULT_SNAPSHOT)),
            PathBuf::from(log_arg.as_deref().unwrap_or(DEFAULT_LOG)),
        ),
    };

    let db_label = dir
        .as_ref()
        .map(|d| d.display().to_string())
        .unwrap_or_else(|| ".".to_string());

    println!("\nValori Status Report  ·  {db_label}");
    println!("{}", "─".repeat(52));

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("File").add_attribute(Attribute::Bold),
            Cell::new("Status").add_attribute(Attribute::Bold),
            Cell::new("Details").add_attribute(Attribute::Bold),
        ]);

    // ── Snapshot ─────────────────────────────────────────────────────────────
    if s_path.exists() {
        match std::fs::read(&s_path) {
            Err(e) => {
                table.add_row(vec![
                    Cell::new("snapshot.val"),
                    Cell::new("ERROR").fg(Color::Red),
                    Cell::new(e.to_string()),
                ]);
            }
            Ok(bytes) => match inspect_snapshot_bytes(&bytes) {
                Err(e) => {
                    table.add_row(vec![
                        Cell::new("snapshot.val"),
                        Cell::new("ERROR").fg(Color::Red),
                        Cell::new(e.to_string()),
                    ]);
                }
                Ok(info) if !info.magic_ok => {
                    table.add_row(vec![
                        Cell::new("snapshot.val"),
                        Cell::new("CORRUPT").fg(Color::Red),
                        Cell::new(format!(
                            "Invalid magic bytes — expected VAL1  ({:.2} KB)",
                            bytes.len() as f64 / 1024.0
                        )),
                    ]);
                }
                Ok(_info) => {
                    // Try to fully decode for richer statistics.
                    let detail = match parse_kernel_from_snapshot_bytes(&bytes) {
                        Ok(state) => format!(
                            "{:.2} KB  │  {} record(s)  │  {} node(s)  │  {} edge(s)  │  dim {}",
                            bytes.len() as f64 / 1024.0,
                            state.record_count(),
                            state.node_count(),
                            state.edge_count(),
                            state.dim.unwrap_or(0),
                        ),
                        Err(_) => format!(
                            "{:.2} KB  │  kernel={} B  │  (decode error — state may be from a newer schema)",
                            bytes.len() as f64 / 1024.0,
                            _info.kernel_len,
                        ),
                    };
                    table.add_row(vec![
                        Cell::new("snapshot.val"),
                        Cell::new("OK")
                            .fg(Color::Green)
                            .add_attribute(Attribute::Bold),
                        Cell::new(detail),
                    ]);
                }
            },
        }
    } else {
        table.add_row(vec![
            Cell::new("snapshot.val"),
            Cell::new("MISSING").fg(Color::Yellow),
            Cell::new("No snapshot — call db.snapshot() to create one"),
        ]);
    }

    // ── Event Log ────────────────────────────────────────────────────────────
    if w_path.exists() {
        match std::fs::read(&w_path) {
            Err(e) => {
                table.add_row(vec![
                    Cell::new("events.log"),
                    Cell::new("ERROR").fg(Color::Red),
                    Cell::new(e.to_string()),
                ]);
            }
            Ok(bytes) if bytes.len() < 16 => {
                table.add_row(vec![
                    Cell::new("events.log"),
                    Cell::new("CORRUPT").fg(Color::Red),
                    Cell::new(format!(
                        "Only {} byte(s) — smaller than the required 16-byte header",
                        bytes.len()
                    )),
                ]);
            }
            Ok(bytes) => {
                let header = match parse_header(&bytes) {
                    Ok(h) => h,
                    Err(e) => {
                        table.add_row(vec![
                            Cell::new("events.log"),
                            Cell::new("CORRUPT").fg(Color::Red),
                            Cell::new(format!("Invalid header: {e}")),
                        ]);
                        println!("{table}\n");
                        return Ok(());
                    }
                };
                let log_version = header.version;
                let dim = header.dim;
                let mut event_count: u64 = 0;
                let mut offset = header.header_len;
                let mut corrupt_msg: Option<String> = None;

                'parse: while offset < bytes.len() {
                    match decode_entry(header.version, &bytes[offset..]) {
                        Ok((chained, n)) => {
                            offset += n;
                            match chained.entry {
                                LogEntry::Event(_) | LogEntry::EventNs { .. } => event_count += 1,
                                LogEntry::Checkpoint { event_count: c, .. } => {
                                    event_count = c;
                                }
                                LogEntry::Admin(_) => {}
                            }
                        }
                        Err(e) => {
                            corrupt_msg = Some(format!(
                                "Decode error at byte {offset} after {event_count} event(s): {e}"
                            ));
                            break 'parse;
                        }
                    }
                }

                if let Some(msg) = corrupt_msg {
                    table.add_row(vec![
                        Cell::new("events.log"),
                        Cell::new("CORRUPT").fg(Color::Red),
                        Cell::new(msg),
                    ]);
                } else {
                    table.add_row(vec![
                        Cell::new("events.log"),
                        Cell::new("OK")
                            .fg(Color::Green)
                            .add_attribute(Attribute::Bold),
                        Cell::new(format!(
                            "{:.2} KB  │  {} event(s)  │  dim {}  │  log-version {}",
                            bytes.len() as f64 / 1024.0,
                            event_count,
                            dim,
                            log_version,
                        )),
                    ]);
                }
            }
        }
    } else {
        table.add_row(vec![
            Cell::new("events.log"),
            Cell::new("MISSING").fg(Color::Yellow),
            Cell::new("No event log found"),
        ]);
    }

    println!("{table}\n");
    Ok(())
}
