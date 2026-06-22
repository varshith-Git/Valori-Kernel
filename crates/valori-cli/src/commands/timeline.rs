// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `valori timeline` — print the event history from an event log.

use comfy_table::presets::UTF8_FULL;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table};
use valori_kernel::event::KernelEvent;
use valori_node::events::event_log::LogEntry;

pub fn run(log_path: &str, limit: usize) -> anyhow::Result<()> {
    let bytes = std::fs::read(log_path)
        .map_err(|e| anyhow::anyhow!("Cannot read '{}': {}", log_path, e))?;

    if bytes.len() < 16 {
        println!("\n⚠️  Event log is empty or too short to parse ({} bytes).\n", bytes.len());
        return Ok(());
    }

    let log_version = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let dim         = u32::from_le_bytes(bytes[4..8].try_into().unwrap());

    println!(
        "\nEvent Timeline  ·  {}  (log-version {}, dim {})\n",
        log_path, log_version, dim
    );

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Event #").add_attribute(Attribute::Bold),
            Cell::new("Type").add_attribute(Attribute::Bold),
            Cell::new("Details").add_attribute(Attribute::Bold),
        ]);

    let header = valori_wire::parse_header(&bytes)
        .map_err(|e| anyhow::anyhow!("Invalid event log header: {e}"))?;
    let mut offset    = header.header_len;
    let mut event_num = 0u64;      // 1-based display counter

    while offset < bytes.len() {
        match valori_wire::decode_entry(header.version, &bytes[offset..]) {
            Ok((chained, bytes_read)) => {
                offset += bytes_read;

                match chained.entry {
                    LogEntry::Event(event) => {
                        event_num += 1;

                        let (type_cell, detail) = describe_event(&event);

                        table.add_row(vec![
                            Cell::new(event_num.to_string()),
                            type_cell,
                            Cell::new(detail),
                        ]);

                        if limit > 0 && event_num as usize >= limit {
                            println!("{table}");
                            println!(
                                "\n  … display limited to first {limit} events. \
                                 Pass --limit 0 to show all.\n"
                            );
                            return Ok(());
                        }
                    }

                    LogEntry::Checkpoint { event_count, .. } => {
                        table.add_row(vec![
                            Cell::new("—"),
                            Cell::new("Checkpoint").fg(Color::Cyan),
                            Cell::new(format!("snapshot taken at event count {event_count}")),
                        ]);
                        event_num = event_count;
                    }

                    LogEntry::Admin(admin) => {
                        table.add_row(vec![
                            Cell::new("—"),
                            Cell::new("Admin").fg(Color::Magenta),
                            Cell::new(admin.describe()),
                        ]);
                    }
                }
            }
            Err(e) => {
                println!("{table}");
                println!(
                    "\n⚠️  Decoding stopped at byte offset {offset} after {} event(s): {e}\n",
                    event_num
                );
                return Ok(());
            }
        }
    }

    println!("{table}");
    println!("\n  Total: {} event(s)\n", event_num);
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn describe_event(event: &KernelEvent) -> (Cell, String) {
    match event {
        KernelEvent::InsertRecord { id, tag, .. } => (
            Cell::new("InsertRecord").fg(Color::Green),
            format!("record_id={} tag={}", id.0, tag),
        ),

        KernelEvent::DeleteRecord { id } => (
            Cell::new("DeleteRecord").fg(Color::Red),
            format!("record_id={}", id.0),
        ),

        KernelEvent::SoftDeleteRecord { id } => (
            Cell::new("SoftDeleteRecord").fg(Color::Yellow),
            format!("record_id={} (tombstoned — slot retained for replay)", id.0),
        ),

        KernelEvent::CreateNode { id, kind, record } => {
            let rec = record
                .as_ref()
                .map(|r| format!(" → record_id={}", r.0))
                .unwrap_or_default();
            (
                Cell::new("CreateNode").fg(Color::Cyan),
                format!("node_id={} kind={:?}{rec}", id.0, kind),
            )
        }

        KernelEvent::CreateEdge { id, from, to, kind } => (
            Cell::new("CreateEdge").fg(Color::Cyan),
            format!(
                "edge_id={}  {}→{}  kind={:?}",
                id.0, from.0, to.0, kind
            ),
        ),

        KernelEvent::DeleteEdge { id } => (
            Cell::new("DeleteEdge").fg(Color::Yellow),
            format!("edge_id={}", id.0),
        ),

        KernelEvent::DeleteNode { id } => (
            Cell::new("DeleteNode").fg(Color::Red),
            format!("node_id={} (cascade-deleted incident edges)", id.0),
        ),

        KernelEvent::InsertRecordEncrypted { id, key_id, .. } => (
            Cell::new("InsertRecordEncrypted").fg(Color::Magenta),
            format!("record_id={}  key={}", id.0,
                key_id.iter().take(4).map(|b| format!("{b:02x}")).collect::<String>()),
        ),

        KernelEvent::ShredKey { key_id } => (
            Cell::new("ShredKey").fg(Color::Magenta),
            format!("key={}  [permanently unrecoverable]",
                key_id.iter().take(4).map(|b| format!("{b:02x}")).collect::<String>()),
        ),

        KernelEvent::AutoInsertRecord { tag, .. } => (
            Cell::new("AutoInsertRecord").fg(Color::Green),
            format!("tag={tag}  (id assigned at apply)"),
        ),

        KernelEvent::AutoCreateNode { kind, record } => {
            let rec = record.map(|r| format!(" → record_id={}", r.0)).unwrap_or_default();
            (
                Cell::new("AutoCreateNode").fg(Color::Cyan),
                format!("kind={:?}{rec}  (id assigned at apply)", kind),
            )
        }

        KernelEvent::AutoCreateEdge { from, to, kind } => (
            Cell::new("AutoCreateEdge").fg(Color::Cyan),
            format!("{}→{}  kind={:?}  (id assigned at apply)", from.0, to.0, kind),
        ),

        KernelEvent::AutoInsertRecordEncrypted { namespace_id, key_id, tag, .. } => (
            Cell::new("AutoInsertRecordEncrypted").fg(Color::Magenta),
            format!("ns={namespace_id} key={}  tag={tag}  (id assigned at apply)",
                key_id.iter().take(4).map(|b| format!("{b:02x}")).collect::<String>()),
        ),
    }
}
