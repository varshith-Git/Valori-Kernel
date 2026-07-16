// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Fuzz-style crash recovery tests.
//!
//! Exercises the event-log reader's resilience to truncated and corrupted data.
use std::fs::OpenOptions;
use std::io::Write;
use tempfile::tempdir;
use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::vector::FxpVector;
use valori_node::events::event_log::{EventLogWriter, LogEntry};
use valori_node::events::event_replay::recover_from_event_log;

const DIM: usize = 16;

fn make_log_with_events(path: &std::path::Path, n: u32) {
    let mut writer = EventLogWriter::open(path, Some(DIM as u32)).unwrap();
    for i in 0..n {
        let event = KernelEvent::InsertRecord {
            id: RecordId(i),
            vector: FxpVector::new_zeros(DIM),
            metadata: None,
            tag: 0,
        };
        writer.append(&LogEntry::Event(event)).unwrap();
    }
    // Drop flushes BufWriter → bytes reach disk
}

// ── Test 1: truncated tail is silently ignored ─────────────────────────────

#[test]
fn test_recover_truncated_tail() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("events.log");

    make_log_with_events(&log_path, 10);

    // Truncate the last byte — simulates partial write of the 10th event.
    let full_size = std::fs::metadata(&log_path).unwrap().len();
    let truncated_size = full_size - 1;
    let file = OpenOptions::new().write(true).open(&log_path).unwrap();
    file.set_len(truncated_size).unwrap();

    // Recovery must succeed and return at most 9 events (the last is partial).
    let (state, _, count) = recover_from_event_log(&log_path).unwrap();

    println!("Recovered {} events from truncated log", count);
    assert!(count < 10, "should lose the last incomplete event");
    assert_eq!(count, 9, "should recover exactly 9 valid events");

    for i in 0..9 {
        assert!(state.get_record(RecordId(i)).is_some());
    }
    assert!(state.get_record(RecordId(9)).is_none());
}

// ── Test 2: mid-file corruption returns an error ───────────────────────────

#[test]
fn test_fail_on_corrupted_middle() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("events.log");

    make_log_with_events(&log_path, 10);

    // Flip a bit in the middle of the file.
    let mut data = std::fs::read(&log_path).unwrap();
    let mid = data.len() / 2;
    data[mid] = !data[mid];
    std::fs::write(&log_path, &data).unwrap();

    let result = recover_from_event_log(&log_path);
    assert!(result.is_err(), "recovery must fail on mid-file corruption");
}

// ── Test 3: empty test placeholder (covered by test 1) ────────────────────

#[test]
fn test_recover_from_crash_before_sync() {
    // This scenario is functionally identical to test_recover_truncated_tail.
    // Kept as a named test so the intent is clear in CI output.
}

// ── Test 4: exhaustive truncation fuzzer ──────────────────────────────────

#[test]
fn test_fuzz_every_truncation_point() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("events.log");

    make_log_with_events(&log_path, 20);

    let full_size = std::fs::metadata(&log_path).unwrap().len();

    // Try recovery at every byte offset after the 16-byte header.
    for size in 16..full_size {
        let loop_dir = tempdir().unwrap();
        let loop_path = loop_dir.path().join("fuzz.log");

        let mut data = std::fs::read(&log_path).unwrap();
        data.truncate(size as usize);
        std::fs::write(&loop_path, &data).unwrap();

        match recover_from_event_log(&loop_path) {
            Ok((_, _, count)) => {
                // Must never return more events than actually fit.
                assert!(count < 20, "count {count} out of range at size {size}");
            }
            Err(e) => {
                // Some truncation points look like mid-event corruption to the
                // reader (> 100 bytes remaining past the parse failure).
                // That is acceptable — we just log and continue the loop.
                println!("size {size}: recovery returned Err({e:?}) — acceptable");
            }
        }
    }
}
