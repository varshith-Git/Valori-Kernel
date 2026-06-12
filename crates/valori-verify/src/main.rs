// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `valori-verify` — standalone offline verifier for Valori event logs.
//!
//! Replays every event in an `events.log` file through the deterministic
//! kernel and recomputes the BLAKE3 state hash — the SAME hash a live server
//! reports at `GET /v1/proof/state`. No server, no network, no trust required.
//!
//! ## Usage
//! ```text
//! valori-verify events.log
//! valori-verify events.log --expected-hash <hex>
//! valori-verify events.log --expected-hash <hex> --report findings.json
//! ```
//!
//! ## Verdicts
//! * `VERIFIED`             — full replay + chain validation passed; hash matches.
//! * `TAMPERED (chain)`     — per-entry BLAKE3 chain breaks at a specific event;
//!                            reports exact event, payload, and commit timestamp.
//! * `TAMPERED (structural)`— an entry failed to decode; reports event + offset.
//! * `TAMPERED (semantic)`  — entry decoded but kernel rejected it.
//! * `TAMPERED (content)`   — chain intact but final state hash differs.

use clap::Parser;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::state::kernel::KernelState;

use valori_wire::{
    chain_advance, decode_entry, format_utc, hex, parse_header, LogEntry, SegmentHeader,
};

#[derive(Parser, Debug)]
#[command(
    name = "valori-verify",
    version,
    about = "Offline verifier for Valori event logs — replay, chain-validate, hash, compare. No server required."
)]
struct Args {
    /// Path to the event log file (e.g. events.log)
    log: PathBuf,

    /// Expected BLAKE3 state hash (64 hex chars), e.g. from GET /v1/proof/state
    #[arg(long, value_name = "HEX")]
    expected_hash: Option<String>,

    /// Write a machine-readable JSON forensic report to this path
    #[arg(long, value_name = "PATH")]
    report: Option<PathBuf>,

    /// Print each event as it is replayed
    #[arg(long)]
    trace: bool,
}

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
        LogEntry::Checkpoint { event_count, .. } => format!("Checkpoint {{ event_count: {event_count} }}"),
        LogEntry::Admin(a) => a.describe(),
    }
}

fn replay(body: &[u8], header: &SegmentHeader, trace: bool) -> ReplayOutcome {
    let mut state = KernelState::new();
    let mut events_applied: u64 = 0;
    let mut checkpoints_seen: u64 = 0;
    let mut offset: usize = 0;
    // v3 segments continue the chain from the previous segment's final head
    // (recorded in the header); v2 and genesis segments start from zeros.
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
                if trace {
                    eprintln!("  event #{:<6} [{}] {:?}", events_applied + 1, format_utc(chained.wall_time_secs), event);
                }
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
            LogEntry::Checkpoint { event_count, .. } => {
                if trace { eprintln!("  checkpoint (event_count = {event_count})"); }
                checkpoints_seen += 1;
            }
            LogEntry::Admin(admin) => {
                // Admin events are chain-verified like everything else but
                // never touch kernel state — membership history rides in
                // the same chain as the data it interleaves with.
                if trace { eprintln!("  admin: {}", admin.describe()); }
            }
        }

        last_entry_summary = entry_summary(&chained.entry);
        chain_head = new_chain_head;
    }

    ReplayOutcome { state, events_applied, checkpoints_seen, chain_head, failure: None }
}

fn build_report(
    log_path: &PathBuf,
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
                "note": "log chain is intact but final state hash differs; \
                         either the expected hash was altered, or an attacker \
                         edited data and recomputed all chain hashes"
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
            } else {
                finding["note"] = serde_json::json!(
                    "entry #1's prev_hash does not match the segment's starting \
                     chain head — its prev_hash field was altered, or the \
                     original head of the log was removed"
                );
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
            "path": log_path.display().to_string(),
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

fn main() -> ExitCode {
    let args = Args::parse();

    let bytes = match std::fs::read(&args.log) {
        Ok(b) => b,
        Err(e) => { eprintln!("error: cannot read '{}': {e}", args.log.display()); return ExitCode::from(2); }
    };

    let header = match parse_header(&bytes) {
        Ok(h) => h,
        Err(e) => { eprintln!("error: {e}"); return ExitCode::from(2); }
    };

    let expected = match &args.expected_hash {
        Some(h) => {
            let h = h.trim().to_lowercase();
            if h.len() != 64 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
                eprintln!("error: --expected-hash must be 64 hex characters");
                return ExitCode::from(2);
            }
            Some(h)
        }
        None => None,
    };

    println!("valori-verify");
    println!("  log:        {}  ({:.2} KB)", args.log.display(), bytes.len() as f64 / 1024.0);
    println!("  format:     v{}, dim {}", header.version, header.dim);
    if header.version >= valori_wire::VERSION_V3 {
        println!(
            "  segment:    #{}{}",
            header.segment_seq,
            if header.segment_seq > 0 {
                format!("  (splices to prev head {}…)", &hex(&header.prev_segment_chain_head)[..16])
            } else {
                String::new()
            }
        );
    }

    let outcome = replay(&bytes[header.header_len..], &header, args.trace);

    println!("  replayed:   {} events, {} checkpoints", outcome.events_applied, outcome.checkpoints_seen);

    let verdict;

    // ── Chain breach ──────────────────────────────────────────────────────────
    if let Some(Failure::ChainBroken {
        breach_at, byte_offset, wall_time_secs,
        prior_entry_summary, computed_chain_head, stored_prev_hash,
    }) = &outcome.failure {
        verdict = "tampered_chain";
        println!();
        println!("❌  TAMPERED (chain breach at entry #{breach_at})");
        if *breach_at == 1 {
            println!("    entry #1's prev_hash doesn't match the segment's starting chain head —");
            println!("    its prev_hash field was altered, or the original head of the log was removed.");
        } else {
            println!("    entry #{breach_at}'s prev_hash doesn't match — entry #{} was altered.", breach_at - 1);
            println!();
            println!("    altered entry (#{}): {prior_entry_summary}", breach_at - 1);
        }
        println!();
        println!("    breach detected at byte offset {byte_offset}");
        println!("    entry #{breach_at} was committed: {}", format_utc(*wall_time_secs));
        println!();
        println!("    computed chain head: {}", hex(computed_chain_head));
        println!("    stored  prev_hash:   {}", hex(stored_prev_hash));
        println!();
        println!("    {} events replayed cleanly before the breach", breach_at - 1);

        if let Some(path) = &args.report {
            let report = build_report(&args.log, bytes.len(), header.version, header.dim,
                expected.as_deref(), &outcome, verdict);
            if let Err(e) = write_report(path, &report) { eprintln!("warning: report write failed: {e}"); }
        }
        return ExitCode::from(1);
    }

    // ── Structural / semantic ─────────────────────────────────────────────────
    if let Some(failure) = &outcome.failure {
        match failure {
            Failure::Decode { event_no, byte_offset, bytes_remaining } => {
                verdict = "tampered_structural";
                println!();
                println!("❌  TAMPERED (structural)");
                println!("    entry #{event_no} failed to decode at byte offset {byte_offset}");
                println!("    {bytes_remaining} trailing bytes are unreadable");
                println!("    events #1..#{} replayed cleanly before the damage", outcome.events_applied);
            }
            Failure::Apply { event_no, byte_offset, detail } => {
                verdict = "tampered_semantic";
                println!();
                println!("❌  TAMPERED (semantic)");
                println!("    event #{event_no} (byte offset {byte_offset}) was rejected by the kernel:");
                println!("    {detail}");
            }
            Failure::ChainBroken { .. } => unreachable!(),
        }
        if let Some(path) = &args.report {
            let report = build_report(&args.log, bytes.len(), header.version, header.dim,
                expected.as_deref(), &outcome, verdict);
            if let Err(e) = write_report(path, &report) { eprintln!("warning: report write failed: {e}"); }
        }
        return ExitCode::from(1);
    }

    // ── Hash comparison ───────────────────────────────────────────────────────
    let computed = hex(&hash_state_blake3(&outcome.state));
    println!("  state hash: {computed}");
    println!("  chain head: {}", hex(&outcome.chain_head));

    let exit = match &expected {
        Some(exp) if exp.as_str() == computed.as_str() => {
            verdict = "verified";
            println!();
            println!("✅  VERIFIED");
            println!("    {} events replayed deterministically; state hash matches.", outcome.events_applied);
            println!("    hash chain intact across all {} entries.", outcome.events_applied);
            ExitCode::SUCCESS
        }
        Some(exp) => {
            verdict = "tampered_content";
            println!();
            println!("❌  TAMPERED (content)");
            println!("    expected: {exp}");
            println!("    computed: {computed}");
            println!("    chain is intact but state hash differs — expected hash may have been");
            println!("    altered, or attacker edited data and recomputed chain hashes.");
            ExitCode::from(1)
        }
        None => {
            verdict = "no_expected_hash";
            println!();
            println!("ℹ️   no --expected-hash given; hash printed for the record.");
            ExitCode::SUCCESS
        }
    };

    if let Some(path) = &args.report {
        let report = build_report(&args.log, bytes.len(), header.version, header.dim,
            expected.as_deref(), &outcome, verdict);
        if let Err(e) = write_report(path, &report) { eprintln!("warning: report write failed: {e}"); }
    }

    exit
}

fn write_report(path: &PathBuf, report: &serde_json::Value) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(path, json)?;
    eprintln!("  report:     {}", path.display());
    Ok(())
}
