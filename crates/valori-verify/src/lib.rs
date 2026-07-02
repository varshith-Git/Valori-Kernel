// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Library surface of `valori-verify`.
//!
//! The wire format lives in the `valori-wire` crate (shared with the node
//! and the forensic CLI — one definition, no drift). Re-exported here so
//! auditors' tooling and the integration tests reach it through this crate.

pub use valori_wire as wire;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::state::kernel::KernelState;
use valori_wire::{chain_advance, decode_entry, format_utc, hex, parse_header, LogEntry, SegmentHeader};

// ── Internal replay types ─────────────────────────────────────────────────────

struct ReplayOutcome {
    state: KernelState,
    events_applied: u64,
    checkpoints_seen: u64,
    chain_head: [u8; 32],
    failure: Option<Failure>,
}

enum Failure {
    ChainBroken {
        breach_at: u64,
        byte_offset: usize,
        wall_time_secs: u64,
        prior_entry_summary: String,
        computed_chain_head: [u8; 32],
        stored_prev_hash: [u8; 32],
    },
    Decode {
        event_no: u64,
        byte_offset: usize,
        bytes_remaining: usize,
    },
    Apply {
        event_no: u64,
        byte_offset: usize,
        detail: String,
    },
}

fn entry_summary(entry: &LogEntry) -> String {
    match entry {
        LogEntry::Event(e) => format!("{e:?}"),
        LogEntry::EventNs { namespace_id, event } => format!("[ns {namespace_id}] {event:?}"),
        LogEntry::Checkpoint { event_count, .. } => format!("Checkpoint {{ event_count: {event_count} }}"),
        LogEntry::Admin(a) => a.describe(),
    }
}

fn replay(body: &[u8], header: &SegmentHeader) -> ReplayOutcome {
    let mut state = KernelState::new();
    let mut events_applied: u64 = 0;
    let mut checkpoints_seen: u64 = 0;
    let mut offset: usize = 0;
    let mut chain_head = header.prev_segment_chain_head;
    let mut last_entry_summary = String::from("<none>");

    while offset < body.len() {
        let chained = match decode_entry(header.version, &body[offset..]) {
            Ok((ce, n)) => { offset += n; ce }
            Err(_) => {
                return ReplayOutcome {
                    state, events_applied, checkpoints_seen, chain_head,
                    failure: Some(Failure::Decode {
                        event_no: events_applied + 1,
                        byte_offset: header.header_len + offset,
                        bytes_remaining: body.len() - offset,
                    }),
                };
            }
        };

        if chained.prev_hash != chain_head {
            return ReplayOutcome {
                state, events_applied, checkpoints_seen, chain_head,
                failure: Some(Failure::ChainBroken {
                    breach_at: events_applied + 1,
                    byte_offset: header.header_len + offset,
                    wall_time_secs: chained.wall_time_secs,
                    prior_entry_summary: last_entry_summary,
                    computed_chain_head: chain_head,
                    stored_prev_hash: chained.prev_hash,
                }),
            };
        }

        let new_chain_head = chain_advance(header.version, &chain_head, &chained)
            .expect("version already validated by parse_header");

        match &chained.entry {
            LogEntry::Event(event) => {
                if let Err(e) = state.apply_event(event) {
                    return ReplayOutcome {
                        state, events_applied, checkpoints_seen, chain_head,
                        failure: Some(Failure::Apply {
                            event_no: events_applied + 1,
                            byte_offset: header.header_len + offset,
                            detail: format!("{e:?} while applying {event:?}"),
                        }),
                    };
                }
                events_applied += 1;
            }
            // S15: namespace-scoped events must replay into their own
            // collection, or the verifier's recomputed state hash would
            // diverge from the node's (which applied them namespaced).
            LogEntry::EventNs { namespace_id, event } => {
                if let Err(e) = state.apply_event_ns(event, *namespace_id) {
                    return ReplayOutcome {
                        state, events_applied, checkpoints_seen, chain_head,
                        failure: Some(Failure::Apply {
                            event_no: events_applied + 1,
                            byte_offset: header.header_len + offset,
                            detail: format!("{e:?} while applying [ns {namespace_id}] {event:?}"),
                        }),
                    };
                }
                events_applied += 1;
            }
            LogEntry::Checkpoint { event_count, .. } => { let _ = event_count; checkpoints_seen += 1; }
            LogEntry::Admin(_) => {}
        }

        last_entry_summary = entry_summary(&chained.entry);
        chain_head = new_chain_head;
    }

    ReplayOutcome { state, events_applied, checkpoints_seen, chain_head, failure: None }
}

fn build_report(
    log_path: &str,
    log_bytes: usize,
    format_version: u32,
    dim: u32,
    expected_hash: Option<&str>,
    outcome: &ReplayOutcome,
    verdict: &str,
) -> serde_json::Value {
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let state_hash = hex(&hash_state_blake3(&outcome.state));

    let finding: serde_json::Value = match &outcome.failure {
        None if expected_hash.is_some() && expected_hash.unwrap() != state_hash => {
            serde_json::json!({
                "type": "content",
                "expected_state_hash": expected_hash.unwrap(),
                "computed_state_hash": state_hash,
                "chain_head": hex(&outcome.chain_head),
                "note": "log chain is intact but final state hash differs"
            })
        }
        None => serde_json::Value::Null,
        Some(Failure::ChainBroken {
            breach_at, byte_offset, wall_time_secs,
            prior_entry_summary, computed_chain_head, stored_prev_hash,
        }) => {
            let mut finding = serde_json::json!({
                "type": "chain_breach",
                "breach_entry_no": breach_at,
                "breach_byte_offset": byte_offset,
                "breach_entry_committed": format_utc(*wall_time_secs),
                "breach_entry_committed_unix": wall_time_secs,
                "computed_chain_head": hex(computed_chain_head),
                "stored_prev_hash": hex(stored_prev_hash),
                "events_clean_before_breach": breach_at - 1,
            });
            if *breach_at > 1 {
                finding["likely_altered_entry_no"] = serde_json::json!(breach_at - 1);
                finding["likely_altered_entry_payload"] = serde_json::json!(prior_entry_summary);
            }
            finding
        }
        Some(Failure::Decode { event_no, byte_offset, bytes_remaining }) => serde_json::json!({
            "type": "structural",
            "failed_entry_no": event_no,
            "failed_byte_offset": byte_offset,
            "trailing_unreadable_bytes": bytes_remaining,
            "events_clean_before_failure": outcome.events_applied,
        }),
        Some(Failure::Apply { event_no, byte_offset, detail }) => serde_json::json!({
            "type": "semantic",
            "rejected_entry_no": event_no,
            "rejected_byte_offset": byte_offset,
            "kernel_error": detail,
            "events_clean_before_rejection": outcome.events_applied,
        }),
    };

    serde_json::json!({
        "schema_version": 1,
        "verdict": verdict,
        "log": {
            "path": log_path,
            "size_bytes": log_bytes,
            "format_version": format_version,
            "dim": dim,
        },
        "replay": {
            "events_replayed": outcome.events_applied,
            "checkpoints_seen": outcome.checkpoints_seen,
            "state_hash": state_hash,
            "chain_head": hex(&outcome.chain_head),
        },
        "expected_hash": expected_hash,
        "generated_at": format_utc(now_unix),
        "generated_at_unix": now_unix,
        "finding": finding,
    })
}

/// Replay an event log file and return a JSON verification report.
///
/// This is the same logic as the `valori-verify` binary, exposed as a library
/// function so callers (the FFI, tests) don't need a subprocess or a binary on PATH.
///
/// Returns a `serde_json::Value` with schema identical to `valori-verify --report`.
pub fn verify_log_file(
    path: &Path,
    expected_hash: Option<&str>,
) -> Result<serde_json::Value, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
    let header = parse_header(&bytes).map_err(|e| format!("cannot parse header: {e}"))?;

    let expected = expected_hash.map(|h| {
        let h = h.trim().to_lowercase();
        if h.len() != 64 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("expected_hash must be 64 hex characters".to_string());
        }
        Ok(h)
    }).transpose()?;

    let outcome = replay(&bytes[header.header_len..], &header);

    let state_hash = hex(&hash_state_blake3(&outcome.state));
    let verdict = if outcome.failure.is_some() {
        match &outcome.failure {
            Some(Failure::ChainBroken { .. }) => "tampered_chain",
            Some(Failure::Decode { .. })      => "tampered_structural",
            Some(Failure::Apply { .. })       => "tampered_semantic",
            None => unreachable!(),
        }
    } else if expected.as_deref().is_some_and(|h| h != state_hash) {
        "tampered_content"
    } else {
        "verified"
    };

    Ok(build_report(
        &path.display().to_string(),
        bytes.len(),
        header.version,
        header.dim,
        expected.as_deref(),
        &outcome,
        verdict,
    ))
}
