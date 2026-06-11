// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Event-log wire format (v2).
//!
//! Mirrors the on-disk contract defined in `node/src/events/event_log.rs`.
//! Duplicated here ON PURPOSE: the verifier must stay free of the server
//! crate so an auditor can build and read a ~400-line tool without trusting
//! the full database binary.
//!
//! ## Layout
//! ```text
//! [Header: 16 bytes][bincode ChainedEntry][bincode ChainedEntry]...
//! ```
//! Header: version u32 LE (=2) | dim u32 LE | reserved u64 LE (=0)
//!
//! ## Hash chain
//! `chain_hash[i] = BLAKE3(chain_hash[i-1] || bincode((wall_time_secs_i, entry_i)))`
//! Genesis chain hash = `[0u8; 32]`.

use serde::{Deserialize, Serialize};
use valori_kernel::event::KernelEvent;

pub const HEADER_SIZE: usize = 16;
pub const SUPPORTED_VERSION: u32 = 2;

/// Mirror of `valori_node::events::event_log::LogEntry`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogEntry {
    Event(KernelEvent),
    Checkpoint {
        event_count: u64,
        snapshot_hash: [u8; 32],
        timestamp: u64,
    },
}

/// Mirror of `valori_node::events::event_log::ChainedEntry`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainedEntry {
    pub prev_hash: [u8; 32],
    pub wall_time_secs: u64,
    pub entry: LogEntry,
}

pub struct Header {
    pub version: u32,
    pub dim: u32,
}

pub fn parse_header(bytes: &[u8]) -> anyhow::Result<Header> {
    if bytes.len() < HEADER_SIZE {
        anyhow::bail!(
            "file is {} bytes — smaller than the {}-byte header; not an event log",
            bytes.len(),
            HEADER_SIZE
        );
    }
    let version = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let dim = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if version != SUPPORTED_VERSION {
        anyhow::bail!(
            "unsupported event-log version {} (this verifier understands version {}; \
             re-generate the log with an up-to-date node to get the hash chain)",
            version,
            SUPPORTED_VERSION
        );
    }
    Ok(Header { version, dim })
}

pub fn encode_header(dim: u32) -> [u8; HEADER_SIZE] {
    let mut bytes = [0u8; HEADER_SIZE];
    bytes[0..4].copy_from_slice(&SUPPORTED_VERSION.to_le_bytes());
    bytes[4..8].copy_from_slice(&dim.to_le_bytes());
    // bytes[8..16] reserved, zero
    bytes
}

/// Advance the chain head by one entry (same formula as the node writer).
pub fn chain_advance(head: &[u8; 32], wall_time_secs: u64, entry: &LogEntry) -> [u8; 32] {
    let commit = bincode::serde::encode_to_vec(
        &(wall_time_secs, entry),
        bincode::config::standard(),
    )
    .expect("LogEntry is always serialisable");
    let mut hasher = blake3::Hasher::new();
    hasher.update(head);
    hasher.update(&commit);
    *hasher.finalize().as_bytes()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Format a Unix timestamp as `YYYY-MM-DDTHH:MM:SSZ` without external deps.
pub fn format_utc(unix_secs: u64) -> String {
    const DAYS_BEFORE_MONTH: [u32; 13] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365];

    let secs_of_day = unix_secs % 86400;
    let h = secs_of_day / 3600;
    let m = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;

    let mut days = (unix_secs / 86400) as u32;
    let mut year = 1970u32;
    loop {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let mut month = 12u32;
    for mi in 1..=12u32 {
        let days_before =
            DAYS_BEFORE_MONTH[mi as usize] + if leap && mi > 2 { 1 } else { 0 };
        if days < days_before {
            month = mi;
            break;
        }
    }
    let days_before_month =
        DAYS_BEFORE_MONTH[(month - 1) as usize] + if leap && month > 2 { 1 } else { 0 };
    let day = days - days_before_month + 1;

    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
}
