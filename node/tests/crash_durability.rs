// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Crash durability test for the event log.
//!
//! Every event acknowledged by `EventLogWriter::append()` must survive an
//! abrupt process death — no graceful shutdown, no `Drop`, no flush. The
//! child process appends events and then `abort()`s, which is equivalent to
//! `kill -9` as far as application-level buffers are concerned: anything not
//! yet handed to the OS is gone.
//!
//! This guards against the regression where `append()` wrote into a
//! `BufWriter` without flushing, so acknowledged events lived only in
//! application memory and a crash lost all of them.

use std::process::Command;

use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::vector::FxpVector;
use valori_node::events::event_log::{EventLogWriter, LogEntry};

const CHILD_LOG_ENV: &str = "CRASH_DURABILITY_CHILD_LOG";
const N_EVENTS: u64 = 50;
const DIM: u32 = 16;

/// Child half of the crash test. A no-op in normal runs; when re-executed by
/// `events_survive_abrupt_death` with CRASH_DURABILITY_CHILD_LOG set, it
/// appends N_EVENTS and dies without unwinding or running destructors.
#[test]
fn crash_child_appends_then_dies() {
    let Ok(path) = std::env::var(CHILD_LOG_ENV) else {
        return;
    };

    let mut writer = EventLogWriter::open(&path, Some(DIM)).unwrap();
    for i in 0..N_EVENTS {
        let event = KernelEvent::InsertRecord {
            id: RecordId(i as u32),
            vector: FxpVector::new_zeros(DIM as usize),
            metadata: None,
            tag: 0,
        };
        writer.append(&LogEntry::Event(event)).unwrap();
    }

    std::process::abort();
}

/// Not a correctness test — prints single-event append (fsync-per-call)
/// throughput. Run with: cargo test ... -- --ignored --nocapture
#[test]
#[ignore]
fn measure_append_fsync_throughput() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.log");
    let mut writer = EventLogWriter::open(&path, Some(DIM)).unwrap();

    let n = 1000u32;
    let start = std::time::Instant::now();
    for i in 0..n {
        let event = KernelEvent::InsertRecord {
            id: RecordId(i),
            vector: FxpVector::new_zeros(DIM as usize),
            metadata: None,
            tag: 0,
        };
        writer.append(&LogEntry::Event(event)).unwrap();
    }
    let elapsed = start.elapsed();
    println!(
        "append with fsync: {} events in {:?} ({:.0} events/s, {:.3} ms/event)",
        n,
        elapsed,
        n as f64 / elapsed.as_secs_f64(),
        elapsed.as_secs_f64() * 1000.0 / n as f64
    );
}

#[test]
fn events_survive_abrupt_death() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.log");

    let status = Command::new(std::env::current_exe().unwrap())
        .args(["crash_child_appends_then_dies", "--exact", "--nocapture"])
        .env(CHILD_LOG_ENV, &path)
        .status()
        .unwrap();
    assert!(
        !status.success(),
        "child process must die via abort, not exit cleanly"
    );

    let writer = EventLogWriter::open(&path, Some(DIM)).unwrap();
    assert_eq!(
        writer.event_count(),
        N_EVENTS,
        "every event acknowledged by append() must be on disk after a crash"
    );
}
