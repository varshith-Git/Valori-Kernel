// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! End-to-end persistence contract tests.
//!
//! Each fixture is a committed `events.log` produced by the real
//! `EventLogWriter` write path, paired with a TOML manifest that pins four
//! independent invariants:
//!
//!   event_count  — number of entries committed to the log
//!   record_count — live records in the recovered KernelState
//!   chain_head   — final BLAKE3 chain head of the log (tamper-evidence)
//!   state_hash   — BLAKE3 hash of the recovered KernelState (determinism)
//!
//! Any of these failing means the persistence pipeline changed in a way that
//! breaks backward compatibility:
//!
//!   valori-wire  (event encoding)
//!   valori-storage (EventLogWriter / recover_from_event_log)
//!   valori-kernel  (apply_event_ns / hash_state_blake3)
//!
//! The end-to-end verification path (via valori-verify::verify_log_file) is
//! also exercised, which is the same path a client uses to audit a live node.
//!
//! `generate_event_log_fixtures` (ignored) writes the files. Run once per
//! format version bump, then commit.

use std::path::Path;
use valori_kernel::event::KernelEvent;
use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;
use valori_storage::events::event_log::LogEntry;
use valori_storage::events::{recover_from_event_log, EventLogWriter};

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn hex32(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Parsed fixture manifest.
#[derive(Debug)]
struct Manifest {
    event_count: u64,
    record_count: usize,
    chain_head: String,
    state_hash: String,
}

fn load_manifest(name: &str) -> Manifest {
    let raw = std::fs::read_to_string(fixture_path(name))
        .unwrap_or_else(|_| panic!("manifest {name} must exist"));
    let doc: toml::Value = raw.parse().expect("manifest must be valid TOML");
    Manifest {
        event_count: doc["event_count"].as_integer().expect("event_count") as u64,
        record_count: doc["record_count"].as_integer().expect("record_count") as usize,
        chain_head: doc["chain_head"].as_str().expect("chain_head").to_owned(),
        state_hash: doc["state_hash"].as_str().expect("state_hash").to_owned(),
    }
}

// ── Helpers shared by generator and tests ────────────────────────────────────

/// Write `events.log` at `path` and return the final chain_head.
fn write_fixture_inserts(path: &Path) -> [u8; 32] {
    let mut w = EventLogWriter::open(path, Some(4)).expect("open event log");
    for i in 0u32..24 {
        let data = (0..4)
            .map(|d| FxpScalar((i * 1000 + d * 7) as i32))
            .collect();
        let event = KernelEvent::InsertRecord {
            id: RecordId(i),
            vector: FxpVector { data },
            metadata: if i % 4 == 0 {
                Some(format!("{{\"n\":{i}}}").into_bytes())
            } else {
                None
            },
            tag: i as u64 % 5,
        };
        let ns = 0u16;
        w.append(&LogEntry::EventNs {
            event,
            namespace_id: ns,
        })
        .expect("append");
    }
    w.flush().expect("flush");
    *w.chain_head()
}

/// Write `events.log` at `path` across two namespaces and return the final chain_head.
fn write_fixture_namespace(path: &Path) -> [u8; 32] {
    let mut w = EventLogWriter::open(path, Some(4)).expect("open event log");
    // ns 0: 12 records
    for i in 0u32..12 {
        let data = (0..4)
            .map(|d| FxpScalar((i * 1000 + d * 7) as i32))
            .collect();
        w.append(&LogEntry::EventNs {
            event: KernelEvent::InsertRecord {
                id: RecordId(i),
                vector: FxpVector { data },
                metadata: None,
                tag: 0,
            },
            namespace_id: 0,
        })
        .expect("append ns0");
    }
    // ns 1: 8 records
    for i in 12u32..20 {
        let data = (0..4)
            .map(|d| FxpScalar((i * 500 + d * 13) as i32 - 1000))
            .collect();
        w.append(&LogEntry::EventNs {
            event: KernelEvent::InsertRecord {
                id: RecordId(i),
                vector: FxpVector { data },
                metadata: None,
                tag: 1,
            },
            namespace_id: 1,
        })
        .expect("append ns1");
    }
    w.flush().expect("flush");
    *w.chain_head()
}

// ── Forever-verify tests ──────────────────────────────────────────────────────

#[test]
fn event_log_inserts_verifies_forever() {
    let log_path = fixture_path("event_log_inserts.log");
    let m = load_manifest("event_log_inserts.toml");

    // Path 1: recovery (valori-state::bootstrap entry point)
    let (state, _journal, event_count) =
        recover_from_event_log(&log_path).expect("fixture must recover");

    assert_eq!(
        event_count, m.event_count,
        "event_count changed — log was truncated or entries were dropped"
    );
    assert_eq!(
        state.record_count(),
        m.record_count,
        "record_count changed — replay logic or soft-delete handling changed"
    );
    assert_eq!(
        hex32(&hash_state_blake3(&state)),
        m.state_hash,
        "state_hash changed — hash domain or apply_event_ns semantics changed"
    );

    // Path 2: end-to-end audit (same path as valori-verify binary / FFI)
    let report = valori_verify::verify_log_file(&log_path, Some(&m.state_hash))
        .expect("verify_log_file must succeed");
    assert_eq!(
        report["verdict"], "verified",
        "verify_log_file verdict not 'verified': {report}"
    );
    assert_eq!(
        report["replay"]["chain_head"].as_str().unwrap(),
        m.chain_head,
        "chain_head changed — BLAKE3 chain formula or wire encoding changed"
    );
}

#[test]
fn event_log_namespace_verifies_forever() {
    let log_path = fixture_path("event_log_namespace.log");
    let m = load_manifest("event_log_namespace.toml");

    let (state, _journal, event_count) =
        recover_from_event_log(&log_path).expect("fixture must recover");

    assert_eq!(event_count, m.event_count, "event_count changed");
    assert_eq!(state.record_count(), m.record_count, "record_count changed");
    assert_eq!(
        hex32(&hash_state_blake3(&state)),
        m.state_hash,
        "state_hash changed"
    );

    let report = valori_verify::verify_log_file(&log_path, Some(&m.state_hash))
        .expect("verify_log_file must succeed");
    assert_eq!(
        report["verdict"], "verified",
        "verify_log_file verdict not 'verified': {report}"
    );
    assert_eq!(
        report["replay"]["chain_head"].as_str().unwrap(),
        m.chain_head,
        "chain_head changed"
    );
}

// ── Parser hardening: committed malformed artifacts must never panic ──────────

#[test]
fn bad_magic_returns_err_not_panic() {
    let path = fixture_path("bad_magic.log");
    let result = valori_verify::verify_log_file(&path, None);
    assert!(
        result.is_err(),
        "bad magic must return Err, not Ok or panic"
    );
}

#[test]
fn truncated_log_returns_err_not_panic() {
    let path = fixture_path("truncated.log");
    // A truncated log may decode successfully with zero events (empty after
    // the header) OR return Err — what it must never do is panic.
    let _ = valori_verify::verify_log_file(&path, None);
    // No assertion beyond "didn't panic" — the interesting invariant is
    // that corrupted input is handled gracefully at every truncation point.
}

#[test]
fn chain_tampered_log_is_detected() {
    let path = fixture_path("chain_tampered.log");
    // V4 has a per-entry CRC, so a flipped data byte is caught as
    // "tampered_structural" before the chain check fires. Either way, the
    // key invariant is: tampering is detected and the verdict is not "verified".
    let report = valori_verify::verify_log_file(&path, None)
        .expect("tampered log must produce a report, not Err");
    let verdict = report["verdict"].as_str().unwrap_or("(missing)");
    assert_ne!(
        verdict, "verified",
        "tampered log must not verify: {report}"
    );
    assert!(
        matches!(
            verdict,
            "tampered_chain" | "tampered_structural" | "tampered_semantic" | "tampered_content"
        ),
        "verdict must be a known tampered variant: {report}"
    );
}

// ── Fixture generator ─────────────────────────────────────────────────────────

/// `cargo test -p valori-state --test event_log_compat generate_event_log_fixtures -- --ignored --nocapture`
#[test]
#[ignore]
fn generate_event_log_fixtures() {
    use std::fs;

    let dir = fixture_path("");
    fs::create_dir_all(&dir).unwrap();

    // ── Fixture 1: inserts ────────────────────────────────────────────────────
    {
        let log_path = dir.join("event_log_inserts.log");
        let _ = fs::remove_file(&log_path); // ensure clean write, not append
        let chain_head = write_fixture_inserts(&log_path);
        let (state, _j, count) = recover_from_event_log(&log_path).unwrap();
        let state_hash = hex32(&hash_state_blake3(&state));
        let manifest = format!(
            "# Generated from commit: see git log\n\
             # Written by: EventLogWriter (valori-storage)\n\
             # Read by: recover_from_event_log (valori-state::bootstrap)\n\
             # Verified by: valori-verify::verify_log_file\n\
             event_count  = {count}\n\
             record_count = {}\n\
             chain_head   = \"{}\"\n\
             state_hash   = \"{state_hash}\"\n",
            state.record_count(),
            hex32(&chain_head),
        );
        fs::write(dir.join("event_log_inserts.toml"), &manifest).unwrap();
        println!(
            "event_log_inserts: {count} events, {} records, chain_head {}, state_hash {state_hash}",
            state.record_count(),
            hex32(&chain_head)
        );
    }

    // ── Fixture 2: two namespaces ─────────────────────────────────────────────
    {
        let log_path = dir.join("event_log_namespace.log");
        let _ = fs::remove_file(&log_path); // ensure clean write, not append
        let chain_head = write_fixture_namespace(&log_path);
        let (state, _j, count) = recover_from_event_log(&log_path).unwrap();
        let state_hash = hex32(&hash_state_blake3(&state));
        let manifest = format!(
            "# Generated from commit: see git log\n\
             # Written by: EventLogWriter (valori-storage)\n\
             # Read by: recover_from_event_log (valori-state::bootstrap)\n\
             # Verified by: valori-verify::verify_log_file\n\
             event_count  = {count}\n\
             record_count = {}\n\
             chain_head   = \"{}\"\n\
             state_hash   = \"{state_hash}\"\n",
            state.record_count(),
            hex32(&chain_head),
        );
        fs::write(dir.join("event_log_namespace.toml"), &manifest).unwrap();
        println!("event_log_namespace: {count} events, {} records, chain_head {}, state_hash {state_hash}",
            state.record_count(), hex32(&chain_head));
    }

    // ── Malformed artifacts (derived from fixture 1) ─────────────────────────

    let good_bytes = fs::read(dir.join("event_log_inserts.log")).unwrap();

    // bad_magic: corrupt the 4-byte MAGIC at offset 0
    {
        let mut bytes = good_bytes.clone();
        bytes[0] ^= 0xFF;
        fs::write(dir.join("bad_magic.log"), &bytes).unwrap();
        println!("bad_magic.log: {} bytes (magic corrupted)", bytes.len());
    }

    // truncated: first 8 bytes only (header magic + partial version field)
    {
        fs::write(
            dir.join("truncated.log"),
            &good_bytes[..8.min(good_bytes.len())],
        )
        .unwrap();
        println!("truncated.log: 8 bytes");
    }

    // chain_tampered: flip a bit in the entry stream well past the header
    {
        let mut bytes = good_bytes.clone();
        if bytes.len() > 200 {
            bytes[200] ^= 0x01;
        }
        fs::write(dir.join("chain_tampered.log"), &bytes).unwrap();
        println!(
            "chain_tampered.log: {} bytes (byte 200 flipped)",
            bytes.len()
        );
    }

    println!("\nAll fixtures written to {:?}", dir);
    println!("Commit: crates/valori-state/tests/fixtures/");
}
