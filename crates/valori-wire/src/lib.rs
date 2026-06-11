// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! # valori-wire — the event-log on-disk contract
//!
//! Single source of truth for Valori's log format, consumed by:
//! * `valori-node` — writes the log (and reads it on recovery/replication)
//! * `valori-verify` — replays and audits logs offline
//! * `valori-cli` — forensic timeline / replay / diff
//!
//! Before this crate existed the format was defined three times and drifted
//! twice. Every change to the on-disk layout MUST happen here, behind a
//! segment-version bump.
//!
//! ## Segment layout
//!
//! ```text
//! v2:  [16-byte header][bincode EntryV2][bincode EntryV2]...
//! v3:  [48-byte header][bincode EntryV3][bincode EntryV3]...
//! ```
//!
//! v2 header: `version u32 LE (=2) | dim u32 LE | reserved u64 LE`
//! v3 header: `version u32 LE (=3) | dim u32 LE | format_id u8 |
//!             reserved [u8;3] | segment_seq u32 LE |
//!             prev_segment_chain_head [u8;32]`
//!
//! ## Hash chain
//!
//! ```text
//! v2: chain[i] = BLAKE3(chain[i-1] || bincode((wall_time_secs, entry)))
//! v3: chain[i] = BLAKE3(chain[i-1] || bincode((wall_time_secs, request_id, entry)))
//! ```
//!
//! Genesis chain head is `[0u8; 32]`. In v3 the chain **continues across
//! segment rotations**: a new segment's header records the previous
//! segment's final chain head (`prev_segment_chain_head`), and its first
//! entry's `prev_hash` equals that value. Deleting or substituting a whole
//! segment therefore breaks the splice — closing the v2 gap where each
//! segment restarted its chain from zeros and archived history could be
//! removed undetected.
//!
//! ## Evolution policy (enforced by fixture tests)
//!
//! 1. **Enum variants are append-only.** bincode encodes variants by index;
//!    reordering or removing a `LogEntry` or `KernelEvent` variant silently
//!    corrupts every existing log. New variants go at the end.
//! 2. **Struct fields never change shape within a version.** Any field
//!    addition/removal/retype requires a segment-version bump and a new
//!    entry struct here.
//! 3. **Readers keep every shipped version readable.** vN tooling must read
//!    vN-1 logs. Committed fixture logs under `tests/fixtures/` replay in CI
//!    forever — if a refactor breaks decoding of old bytes, CI fails.
//! 4. **Writers emit only the newest version** for new files; existing
//!    older-version files continue in their own format until rotation,
//!    which upgrades the segment (and splices the chain).

use serde::{Deserialize, Serialize};
use valori_kernel::event::KernelEvent;

pub const VERSION_V2: u32 = 2;
pub const VERSION_V3: u32 = 3;
pub const HEADER_SIZE_V2: usize = 16;
pub const HEADER_SIZE_V3: usize = 48;

/// Arithmetic format identifiers (hash-domain relevant — a Q8.8 log must
/// never verify as a Q16.16 log). Only Q16.16 is implemented today; the id
/// exists so Phase 1.3's `FxpFormat` work needs no further format bump.
pub const FORMAT_Q16_16: u8 = 1;

#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("file is {0} bytes — smaller than the smallest valid header; not an event log")]
    TooShort(usize),
    #[error("unsupported segment version {0} (this build understands v2 and v3)")]
    UnsupportedVersion(u32),
    #[error("unsupported arithmetic format id {0} (this build understands {FORMAT_Q16_16} = Q16.16)")]
    UnsupportedFormat(u8),
    #[error("entry decode failed: {0}")]
    Decode(String),
    #[error("entry encode failed: {0}")]
    Encode(String),
}

pub type Result<T> = core::result::Result<T, WireError>;

/// Payload of one log entry — shared by every segment version.
///
/// EVOLUTION: variants are append-only (see crate docs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogEntry {
    Event(KernelEvent),
    Checkpoint {
        event_count: u64,
        snapshot_hash: [u8; 32],
        timestamp: u64,
    },
}

/// v2 on-disk entry (legacy — read-only since v3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryV2 {
    pub prev_hash: [u8; 32],
    pub wall_time_secs: u64,
    pub entry: LogEntry,
}

/// v3 on-disk entry.
///
/// `request_id` is the client-supplied idempotency token (UUID bytes).
/// `None` for internally generated entries (checkpoints, replication
/// re-writes). Phase 2's Raft dedup table is keyed on it; the schema exists
/// now so production v3 logs never need migrating to gain it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryV3 {
    pub prev_hash: [u8; 32],
    pub wall_time_secs: u64,
    pub request_id: Option<[u8; 16]>,
    pub entry: LogEntry,
}

/// Version-independent view of a decoded entry.
#[derive(Debug, Clone)]
pub struct DecodedEntry {
    pub prev_hash: [u8; 32],
    pub wall_time_secs: u64,
    pub request_id: Option<[u8; 16]>,
    pub entry: LogEntry,
}

/// Parsed segment header, normalized across versions.
#[derive(Debug, Clone)]
pub struct SegmentHeader {
    pub version: u32,
    pub dim: u32,
    pub format_id: u8,
    /// 0 for the genesis segment; v2 segments report 0.
    pub segment_seq: u32,
    /// Final chain head of the previous segment ([0;32] for genesis and v2).
    pub prev_segment_chain_head: [u8; 32],
    /// Byte length of the header — entries start at this offset.
    pub header_len: usize,
}

pub fn parse_header(bytes: &[u8]) -> Result<SegmentHeader> {
    if bytes.len() < HEADER_SIZE_V2 {
        return Err(WireError::TooShort(bytes.len()));
    }
    let version = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let dim = u32::from_le_bytes(bytes[4..8].try_into().unwrap());

    match version {
        VERSION_V2 => Ok(SegmentHeader {
            version,
            dim,
            format_id: FORMAT_Q16_16,
            segment_seq: 0,
            prev_segment_chain_head: [0u8; 32],
            header_len: HEADER_SIZE_V2,
        }),
        VERSION_V3 => {
            if bytes.len() < HEADER_SIZE_V3 {
                return Err(WireError::TooShort(bytes.len()));
            }
            let format_id = bytes[8];
            if format_id != FORMAT_Q16_16 {
                return Err(WireError::UnsupportedFormat(format_id));
            }
            let segment_seq = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
            let prev_segment_chain_head: [u8; 32] = bytes[16..48].try_into().unwrap();
            Ok(SegmentHeader {
                version,
                dim,
                format_id,
                segment_seq,
                prev_segment_chain_head,
                header_len: HEADER_SIZE_V3,
            })
        }
        v => Err(WireError::UnsupportedVersion(v)),
    }
}

pub fn encode_header_v3(
    dim: u32,
    format_id: u8,
    segment_seq: u32,
    prev_segment_chain_head: &[u8; 32],
) -> [u8; HEADER_SIZE_V3] {
    let mut bytes = [0u8; HEADER_SIZE_V3];
    bytes[0..4].copy_from_slice(&VERSION_V3.to_le_bytes());
    bytes[4..8].copy_from_slice(&dim.to_le_bytes());
    bytes[8] = format_id;
    // bytes[9..12] reserved, zero
    bytes[12..16].copy_from_slice(&segment_seq.to_le_bytes());
    bytes[16..48].copy_from_slice(prev_segment_chain_head);
    bytes
}

/// Legacy v2 header encoder — kept for fixture generation and tests only;
/// writers must not emit new v2 segments.
pub fn encode_header_v2(dim: u32) -> [u8; HEADER_SIZE_V2] {
    let mut bytes = [0u8; HEADER_SIZE_V2];
    bytes[0..4].copy_from_slice(&VERSION_V2.to_le_bytes());
    bytes[4..8].copy_from_slice(&dim.to_le_bytes());
    bytes
}

fn cfg() -> bincode::config::Configuration {
    bincode::config::standard()
}

/// Decode one entry at `bytes[0..]` for the given segment version.
/// Returns the normalized entry and the number of bytes consumed.
pub fn decode_entry(version: u32, bytes: &[u8]) -> Result<(DecodedEntry, usize)> {
    match version {
        VERSION_V2 => {
            let (e, n): (EntryV2, usize) = bincode::serde::decode_from_slice(bytes, cfg())
                .map_err(|e| WireError::Decode(e.to_string()))?;
            Ok((
                DecodedEntry {
                    prev_hash: e.prev_hash,
                    wall_time_secs: e.wall_time_secs,
                    request_id: None,
                    entry: e.entry,
                },
                n,
            ))
        }
        VERSION_V3 => {
            let (e, n): (EntryV3, usize) = bincode::serde::decode_from_slice(bytes, cfg())
                .map_err(|e| WireError::Decode(e.to_string()))?;
            Ok((
                DecodedEntry {
                    prev_hash: e.prev_hash,
                    wall_time_secs: e.wall_time_secs,
                    request_id: e.request_id,
                    entry: e.entry,
                },
                n,
            ))
        }
        v => Err(WireError::UnsupportedVersion(v)),
    }
}

/// Encode one entry for the given segment version.
/// `request_id` is dropped (with no error) when encoding legacy v2 —
/// callers should not pass one for v2 segments.
pub fn encode_entry(
    version: u32,
    prev_hash: &[u8; 32],
    wall_time_secs: u64,
    request_id: Option<[u8; 16]>,
    entry: &LogEntry,
) -> Result<Vec<u8>> {
    match version {
        VERSION_V2 => bincode::serde::encode_to_vec(
            &EntryV2 {
                prev_hash: *prev_hash,
                wall_time_secs,
                entry: entry.clone(),
            },
            cfg(),
        )
        .map_err(|e| WireError::Encode(e.to_string())),
        VERSION_V3 => bincode::serde::encode_to_vec(
            &EntryV3 {
                prev_hash: *prev_hash,
                wall_time_secs,
                request_id,
                entry: entry.clone(),
            },
            cfg(),
        )
        .map_err(|e| WireError::Encode(e.to_string())),
        v => Err(WireError::UnsupportedVersion(v)),
    }
}

/// Advance the chain head by one v2 entry:
/// `BLAKE3(head || bincode((wall_time_secs, entry)))`
pub fn chain_advance_v2(head: &[u8; 32], wall_time_secs: u64, entry: &LogEntry) -> [u8; 32] {
    let commit = bincode::serde::encode_to_vec(&(wall_time_secs, entry), cfg())
        .expect("LogEntry is always serialisable");
    let mut hasher = blake3::Hasher::new();
    hasher.update(head);
    hasher.update(&commit);
    *hasher.finalize().as_bytes()
}

/// Advance the chain head by one v3 entry:
/// `BLAKE3(head || bincode((wall_time_secs, request_id, entry)))`
pub fn chain_advance_v3(
    head: &[u8; 32],
    wall_time_secs: u64,
    request_id: Option<[u8; 16]>,
    entry: &LogEntry,
) -> [u8; 32] {
    let commit = bincode::serde::encode_to_vec(&(wall_time_secs, request_id, entry), cfg())
        .expect("LogEntry is always serialisable");
    let mut hasher = blake3::Hasher::new();
    hasher.update(head);
    hasher.update(&commit);
    *hasher.finalize().as_bytes()
}

/// Version-dispatching chain advance over a decoded entry.
pub fn chain_advance(version: u32, head: &[u8; 32], e: &DecodedEntry) -> Result<[u8; 32]> {
    match version {
        VERSION_V2 => Ok(chain_advance_v2(head, e.wall_time_secs, &e.entry)),
        VERSION_V3 => Ok(chain_advance_v3(head, e.wall_time_secs, e.request_id, &e.entry)),
        v => Err(WireError::UnsupportedVersion(v)),
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Format a Unix timestamp as `YYYY-MM-DDTHH:MM:SSZ` without external deps.
pub fn format_utc(unix_secs: u64) -> String {
    const DAYS_BEFORE_MONTH: [u32; 13] =
        [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365];

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
        let days_before = DAYS_BEFORE_MONTH[mi as usize] + if leap && mi > 2 { 1 } else { 0 };
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
