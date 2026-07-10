// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Regression test: `replay_log` (used by valori-anchor) and `verify_log_file`
//! (used by the CLI and FFI) must produce identical chain_head, state_hash,
//! and event_count for the same log — including logs with namespace-scoped
//! (`EventNs`) entries.
//!
//! This test was added after discovering that the original `valori_anchor.rs`
//! `replay_log` silently skipped `LogEntry::EventNs`, causing anchor state
//! hashes to diverge from verifier state hashes on any log that used collections.

use std::io::Write as _;

use tempfile::NamedTempFile;
use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;
use valori_wire::{
    chain_advance_v3, encode_entry, encode_header_v4, LogEntry, FORMAT_Q16_16, VERSION_V4,
};
use valori_verify::{replay_log, verify_log_file};

const DIM: usize = 4;

fn vec4(a: i32, b: i32, c: i32, d: i32) -> FxpVector {
    FxpVector { data: vec![FxpScalar(a), FxpScalar(b), FxpScalar(c), FxpScalar(d)] }
}

/// Write a log containing both plain `Event` and namespace-scoped `EventNs`
/// entries and return the path to the temp file.
fn write_mixed_log() -> NamedTempFile {
    let tmp = NamedTempFile::new().unwrap();
    let mut out = std::io::BufWriter::new(tmp.reopen().unwrap());

    out.write_all(&encode_header_v4(DIM as u32, FORMAT_Q16_16, 0, &[0u8; 32])).unwrap();

    let entries: Vec<(u16, LogEntry)> = vec![
        // ns 0 — default namespace
        (0, LogEntry::Event(KernelEvent::InsertRecord {
            id: RecordId(0),
            vector: vec4(1000, 2000, -1000, 500),
            metadata: None,
            tag: 0,
        })),
        // ns 1 — collection "tenant-a"
        (1, LogEntry::EventNs {
            namespace_id: 1,
            event: KernelEvent::InsertRecord {
                id: RecordId(1),
                vector: vec4(-500, 1500, 2500, -2000),
                metadata: None,
                tag: 0,
            },
        }),
        // ns 2 — collection "tenant-b"
        (2, LogEntry::EventNs {
            namespace_id: 2,
            event: KernelEvent::InsertRecord {
                id: RecordId(2),
                vector: vec4(3000, -1000, 500, 1000),
                metadata: None,
                tag: 0,
            },
        }),
        // back to ns 0
        (0, LogEntry::Event(KernelEvent::InsertRecord {
            id: RecordId(3),
            vector: vec4(0, 0, 65535, -65536),
            metadata: None,
            tag: 0,
        })),
    ];

    let base_ts: u64 = 1_750_000_000;
    let mut chain_head = [0u8; 32];

    for (i, (_, entry)) in entries.iter().enumerate() {
        let wall_time = base_ts + i as u64;
        let bytes = encode_entry(VERSION_V4, &chain_head, wall_time, None, entry).unwrap();
        out.write_all(&bytes).unwrap();
        chain_head = chain_advance_v3(&chain_head, wall_time, None, entry);
    }

    out.flush().unwrap();
    drop(out);
    tmp
}

#[test]
fn anchor_and_verify_agree_on_mixed_ns_log() {
    let tmp = write_mixed_log();
    let path = tmp.path();

    // Path A: verify_log_file (used by CLI binary and FFI)
    let report = verify_log_file(path, None).expect("verify_log_file failed");
    // no expected_hash supplied → verdict is the chain-integrity result
    assert!(
        matches!(report["verdict"].as_str(), Some("verified") | Some("no_expected_hash")),
        "unexpected verdict: {}", report["verdict"]
    );
    let vf_state_hash = report["replay"]["state_hash"].as_str().unwrap().to_owned();
    let vf_chain_head = report["replay"]["chain_head"].as_str().unwrap().to_owned();
    let vf_events: u64 = report["replay"]["events_replayed"].as_u64().unwrap();

    // Path B: replay_log (used by valori-anchor binary)
    let summary = replay_log(path).expect("replay_log failed");
    let anchor_state_hash = valori_wire::hex(&summary.state_hash);
    let anchor_chain_head = valori_wire::hex(&summary.chain_head);

    assert_eq!(
        vf_state_hash, anchor_state_hash,
        "state_hash mismatch: verify_log_file saw {vf_state_hash} but replay_log saw {anchor_state_hash}"
    );
    assert_eq!(
        vf_chain_head, anchor_chain_head,
        "chain_head mismatch: verify_log_file saw {vf_chain_head} but replay_log saw {anchor_chain_head}"
    );
    assert_eq!(
        vf_events, summary.event_count,
        "event_count mismatch: verify_log_file counted {vf_events} but replay_log counted {}",
        summary.event_count
    );
    assert_eq!(summary.trailing_bytes, 0, "unexpected trailing bytes");
}
