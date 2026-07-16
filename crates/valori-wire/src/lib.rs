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
//! v4:  [48-byte header][bincode EntryV4][u32 LE CRC32]...  (per-entry CRC suffix)
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
/// V4 adds a 4-byte CRC32 suffix to every entry for cheap inline corruption detection.
/// Format: `[bincode(EntryV4)][u32 LE CRC32 of the bincode bytes]`
/// The chain hash, header layout, and EntryV4 fields are identical to V3.
pub const VERSION_V4: u32 = 4;
pub const HEADER_SIZE_V2: usize = 16;
pub const HEADER_SIZE_V3: usize = 48;
/// V4 reuses the V3 header layout.
pub const HEADER_SIZE_V4: usize = HEADER_SIZE_V3;
/// Byte length of the per-entry CRC32 suffix in V4 segments.
pub const CRC32_SUFFIX_LEN: usize = 4;

// ── Phase 1.7 hardening constants (reserved; enforced in Phase 1.7) ──────────

/// Maximum bytes bincode may allocate while decoding a SINGLE log entry.
///
/// A crafted entry can encode a Vec<u8> with a claimed length of usize::MAX;
/// without this cap, bincode allocates immediately and causes an OOM.
/// Applied via `bincode::config::standard().with_limit::<DECODE_LIMIT>()`
/// (the usize twin of this constant) in every decode call in this crate.
pub const MAX_ENTRY_DECODE_BYTES: u64 = DECODE_LIMIT as u64;

/// Hard cap on entries decoded from a single segment file.
/// Prevents unbounded loops on crafted/malformed data.
/// Enforced in the valori-verify decode loop.
pub const MAX_ENTRIES_PER_SEGMENT: u64 = 10_000_000;

/// Maximum size in bytes of a metadata blob inside a single event.
/// Kept separately from MAX_ENTRY_DECODE_BYTES so that the per-field guard
/// produces a more specific error than the overall allocation limit.
/// Enforced by `encode_entry` on every metadata-bearing event variant —
/// write-side only, so pre-cap logs remain readable.
pub const METADATA_CAP: usize = 65_536; // 64 KiB

/// Maximum decompressed size for a zstd-compressed segment file.
/// Protects the verifier and the node from zstd "bombs".
///
/// **Reserved — zstd segment compression is not implemented yet; this cap
/// is not enforced anywhere today. It must be applied to the decompression
/// wrapper when zstd lands.**
pub const MAX_SEGMENT_DECOMPRESSED_BYTES: usize = 512 * 1024 * 1024; // 512 MiB

// ─────────────────────────────────────────────────────────────────────────────

/// Arithmetic format identifiers (hash-domain relevant — a Q8.8 log must
/// never verify as a Q16.16 log). Only Q16.16 is implemented today; the id
/// exists so Phase 1.3's `FxpFormat` work needs no further format bump.
pub const FORMAT_Q16_16: u8 = 1;

/// usize-typed decode limit for `bincode::config::with_limit` (const generic requires usize).
/// Equals `MAX_ENTRY_DECODE_BYTES` — kept separate to avoid a u64→usize cast at const position.
const DECODE_LIMIT: usize = 1 << 20; // 1 MiB

/// Maximum dimension for a segment header.
/// No real embedding is zero-dimensional or wider than 32 768 scalars.
pub const MAX_DIM: u32 = 32_768;

#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("file is {0} bytes — smaller than the smallest valid header; not an event log")]
    TooShort(usize),
    #[error("unsupported segment version {0} (this build understands v2, v3, and v4)")]
    UnsupportedVersion(u32),
    #[error(
        "unsupported arithmetic format id {0} (this build understands {FORMAT_Q16_16} = Q16.16)"
    )]
    UnsupportedFormat(u8),
    #[error("entry decode failed: {0}")]
    Decode(String),
    #[error("entry encode failed: {0}")]
    Encode(String),
    #[error("invalid dim {0} in segment header (must be 1..={MAX_DIM})")]
    InvalidDim(u32),
    #[error("entry exceeds the {DECODE_LIMIT}-byte allocation limit — file is likely crafted or corrupted")]
    DecodeLimitExceeded,
    #[error("metadata blob of {0} bytes exceeds the {METADATA_CAP}-byte cap — file is likely crafted or corrupted")]
    MetadataTooLarge(usize),
    /// Fewer bytes remain than a complete entry needs — the shape a crash
    /// mid-write leaves at the tail of a segment. Distinct from `Decode`
    /// (enough bytes, wrong content — real corruption) so segment-replay
    /// callers can tell "safe to stop here" from "must hard-error" without
    /// a byte-offset heuristic.
    #[error(
        "not enough bytes remain to decode a complete entry — likely a truncated trailing write"
    )]
    Truncated,
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
    /// Cluster administration recorded in the SAME hash chain as data —
    /// membership cannot change without it appearing between the data
    /// events it interleaves with. Added Phase 2.9 (append-only variant 2).
    Admin(AdminEvent),
    /// A data event scoped to a non-default namespace (collection). Added
    /// Phase S15 (append-only variant 3): `KernelEvent` itself carries no
    /// namespace, so before this variant existed, standalone recovery
    /// replayed every event into the default namespace and collections
    /// silently lost their contents across restarts. Writers emit this
    /// variant only when `namespace_id != 0` — default-namespace logs stay
    /// byte-identical to pre-S15, and pre-S15 logs (all `Event`) replay
    /// exactly as they always did.
    EventNs {
        namespace_id: u16,
        event: KernelEvent,
    },
}

/// Administrative actions worth auditing forever.
///
/// `authorized_by` is the BLAKE3 hash (first 16 bytes) of the credential
/// that authorized the action at the time it happened — rotating the
/// credential later does not change historical attribution. All-zeros
/// means "no authentication configured" (pre-RBAC deployments).
///
/// EVOLUTION: variants append-only; the Phase 1.6 design reserves
/// CertRotated, TenantKeyCreated/Revoked, EraseRecord, ClusterCaRotated
/// for Phases 2.10/3 — they slot in after the last variant here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AdminEvent {
    NodeJoined {
        node_id: u64,
        raft_addr: String,
        api_addr: String,
        authorized_by: [u8; 16],
    },
    NodeLeft {
        node_id: u64,
        authorized_by: [u8; 16],
    },
}

impl AdminEvent {
    pub fn describe(&self) -> String {
        match self {
            AdminEvent::NodeJoined {
                node_id, raft_addr, ..
            } => {
                format!("NodeJoined {{ node {node_id} at {raft_addr} }}")
            }
            AdminEvent::NodeLeft { node_id, .. } => {
                format!("NodeLeft {{ node {node_id} }}")
            }
        }
    }
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

/// V4 on-disk entry — identical fields to V3; the CRC32 suffix is appended
/// after the bincode bytes by `encode_entry` and checked by `decode_entry`.
/// The chain-hash computation is identical to V3.
pub type EntryV4 = EntryV3;

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

    if !(1..=MAX_DIM).contains(&dim) {
        return Err(WireError::InvalidDim(dim));
    }

    match version {
        VERSION_V2 => Ok(SegmentHeader {
            version,
            dim,
            format_id: FORMAT_Q16_16,
            segment_seq: 0,
            prev_segment_chain_head: [0u8; 32],
            header_len: HEADER_SIZE_V2,
        }),
        // V4 reuses the V3 header layout byte-for-byte (only the version
        // field differs); one arm keeps the two from drifting.
        VERSION_V3 | VERSION_V4 => {
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

/// Legacy v3 header encoder — kept for fixture generation and tests only;
/// writers must not emit new v3 segments (v4 is the current write format).
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

/// V4 header encoder — identical layout to V3, version field set to 4.
pub fn encode_header_v4(
    dim: u32,
    format_id: u8,
    segment_seq: u32,
    prev_segment_chain_head: &[u8; 32],
) -> [u8; HEADER_SIZE_V4] {
    let mut bytes = [0u8; HEADER_SIZE_V4];
    bytes[0..4].copy_from_slice(&VERSION_V4.to_le_bytes());
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

fn cfg() -> impl bincode::config::Config {
    bincode::config::standard().with_limit::<DECODE_LIMIT>()
}

fn map_decode_err(e: bincode::error::DecodeError) -> WireError {
    match e {
        bincode::error::DecodeError::LimitExceeded => WireError::DecodeLimitExceeded,
        // Not enough bytes to finish decoding — truncation, not corruption.
        bincode::error::DecodeError::UnexpectedEnd { .. } => WireError::Truncated,
        other => WireError::Decode(other.to_string()),
    }
}

/// Reject any metadata blob larger than [`METADATA_CAP`].
/// Covers every metadata-bearing `KernelEvent` variant, wrapped in either
/// `LogEntry::Event` or `LogEntry::EventNs`.
///
/// Applied on ENCODE only: logs written before the cap existed may contain
/// larger blobs and must stay readable forever (evolution policy rule 3);
/// `DECODE_LIMIT` still bounds allocation on the read side.
fn check_metadata_cap(entry: &LogEntry) -> Result<()> {
    let event = match entry {
        LogEntry::Event(e) => e,
        LogEntry::EventNs { event, .. } => event,
        _ => return Ok(()),
    };
    let meta_len = match event {
        KernelEvent::InsertRecord { metadata, .. }
        | KernelEvent::AutoInsertRecord { metadata, .. }
        | KernelEvent::UpdateRecordMetadata { metadata, .. } => metadata.as_ref().map(|m| m.len()),
        KernelEvent::InsertRecordEncrypted {
            metadata_ciphertext,
            ..
        } => metadata_ciphertext.as_ref().map(|m| m.len()),
        _ => None,
    };
    match meta_len {
        Some(len) if len > METADATA_CAP => Err(WireError::MetadataTooLarge(len)),
        _ => Ok(()),
    }
}

/// Decode one entry at `bytes[0..]` for the given segment version.
/// Returns the normalized entry and the number of bytes consumed.
pub fn decode_entry(version: u32, bytes: &[u8]) -> Result<(DecodedEntry, usize)> {
    let (decoded, consumed) = match version {
        VERSION_V2 => {
            let (e, n): (EntryV2, usize) =
                bincode::serde::decode_from_slice(bytes, cfg()).map_err(map_decode_err)?;
            (
                DecodedEntry {
                    prev_hash: e.prev_hash,
                    wall_time_secs: e.wall_time_secs,
                    request_id: None,
                    entry: e.entry,
                },
                n,
            )
        }
        VERSION_V3 => {
            let (e, n): (EntryV3, usize) =
                bincode::serde::decode_from_slice(bytes, cfg()).map_err(map_decode_err)?;
            (
                DecodedEntry {
                    prev_hash: e.prev_hash,
                    wall_time_secs: e.wall_time_secs,
                    request_id: e.request_id,
                    entry: e.entry,
                },
                n,
            )
        }
        VERSION_V4 => {
            // Decode the bincode payload, then verify the 4-byte CRC32 suffix.
            let (e, n): (EntryV4, usize) =
                bincode::serde::decode_from_slice(bytes, cfg()).map_err(map_decode_err)?;
            // CRC32 suffix immediately follows the bincode bytes. Missing
            // suffix bytes is a truncation (not enough bytes), not corruption.
            if n + CRC32_SUFFIX_LEN > bytes.len() {
                return Err(WireError::Truncated);
            }
            let stored_crc = u32::from_le_bytes(bytes[n..n + CRC32_SUFFIX_LEN].try_into().unwrap());
            let computed_crc = crc32fast::hash(&bytes[..n]);
            if computed_crc != stored_crc {
                return Err(WireError::Decode(format!(
                    "V4 entry CRC32 mismatch: stored {stored_crc:#010x}, computed {computed_crc:#010x}"
                )));
            }
            (
                DecodedEntry {
                    prev_hash: e.prev_hash,
                    wall_time_secs: e.wall_time_secs,
                    request_id: e.request_id,
                    entry: e.entry,
                },
                n + CRC32_SUFFIX_LEN,
            )
        }
        v => return Err(WireError::UnsupportedVersion(v)),
    };
    Ok((decoded, consumed))
}

/// Encode one entry for the given segment version.
/// `request_id` is dropped (with no error) when encoding legacy v2 —
/// callers should not pass one for v2 segments.
/// V4 appends a 4-byte LE CRC32 of the bincode bytes after the payload.
pub fn encode_entry(
    version: u32,
    prev_hash: &[u8; 32],
    wall_time_secs: u64,
    request_id: Option<[u8; 16]>,
    entry: &LogEntry,
) -> Result<Vec<u8>> {
    check_metadata_cap(entry)?;
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
        VERSION_V4 => {
            let mut payload = bincode::serde::encode_to_vec(
                &EntryV4 {
                    prev_hash: *prev_hash,
                    wall_time_secs,
                    request_id,
                    entry: entry.clone(),
                },
                cfg(),
            )
            .map_err(|e| WireError::Encode(e.to_string()))?;
            let crc = crc32fast::hash(&payload);
            payload.extend_from_slice(&crc.to_le_bytes());
            Ok(payload)
        }
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
        VERSION_V3 => Ok(chain_advance_v3(
            head,
            e.wall_time_secs,
            e.request_id,
            &e.entry,
        )),
        // V4 chain hash is identical to V3 — CRC32 is only a transport check, not part of the chain.
        VERSION_V4 => Ok(chain_advance_v3(
            head,
            e.wall_time_secs,
            e.request_id,
            &e.entry,
        )),
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
