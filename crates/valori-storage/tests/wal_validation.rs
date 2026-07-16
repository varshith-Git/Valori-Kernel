// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! P7 — WAL / event-log validation.
//!
//! Covers the four scenarios the phase asks for, targeted at BOTH durability
//! layers this crate owns (the legacy bincode `WalWriter`/`WalReader`, and
//! the canonical BLAKE3-chained `EventLogWriter`/event-replay):
//!   - partial / truncated records
//!   - checksum (CRC32, V4 segments) mismatch
//!   - a truncated WAL/log file (shorter than the header)
//!   - duplicate / out-of-order sequence
//!
//! Before these tests, `EventLogWriter::open`'s replay-on-open loop and
//! `event_replay::read_segment_full`'s recovery loop each carried their OWN
//! copy of "decide whether a decode failure means trailing-partial-write or
//! real corruption" — the former tolerated ANY decode error unconditionally,
//! the latter used a fuzzy "within 100 bytes of EOF" heuristic. Both were
//! replaced by a single shared `walk_segment_body` (in `event_log.rs`) that
//! makes the distinction structurally, via `WireError::Truncated`, instead
//! of guessing from a byte offset — these tests exercise that shared path
//! from both call sites, including truncation points a 100-byte heuristic
//! would have gotten wrong.

use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::vector::FxpVector;
use valori_storage::events::event_log::{EventLogWriter, LogEntry};
use valori_storage::events::event_replay::{read_all_segments, recover_from_event_log, replay_events};
use valori_storage::wal_reader::WalReader;
use valori_storage::wal_writer::WalWriter;

const DIM: usize = 8;

fn ev(i: u32) -> KernelEvent {
    KernelEvent::InsertRecord {
        id: RecordId(i),
        vector: FxpVector::new_zeros(DIM),
        metadata: None,
        tag: 0,
    }
}

// ── Simple WAL: read_entry() clean-EOF regression (bug found while ─────
// writing the P5 WAL benchmark: a raw `read_entry()` loop hit
// `Err(Deserialization(Io(UnexpectedEof)))` at EOF instead of the
// documented `Ok(None)`, because bincode 2.0.1's `decode_from_std_read`
// raises `DecodeError::Io`, not `DecodeError::UnexpectedEnd`, on a
// `BufReader<File>`. Existing tests never hit it because they only ever
// drove `WalReader` through its `IntoIterator` impl, which pre-checks EOF
// via `fill_buf()` before calling `read_entry()`.

#[test]
fn wal_reader_read_entry_returns_none_at_clean_eof_not_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clean.wal");
    {
        let mut w = WalWriter::open(&path, DIM as u32).unwrap();
        for i in 0..10 {
            w.append_event(&ev(i), 0).unwrap();
        }
    }

    let mut r = WalReader::open(&path, Some(DIM as u32)).unwrap();
    let mut count = 0;
    loop {
        match r.read_entry() {
            Ok(Some(_)) => count += 1,
            Ok(None) => break,
            Err(e) => panic!("read_entry() must return Ok(None) at clean EOF, got Err({e})"),
        }
    }
    assert_eq!(count, 10);
    // Calling again past EOF must stay clean, not flip to an error.
    assert!(matches!(r.read_entry(), Ok(None)));
}

// ── Simple WAL: partial / truncated records ─────────────────────────────

#[test]
fn wal_truncated_at_various_points_recovers_complete_prefix_without_erroring() {
    let dir = tempfile::tempdir().unwrap();
    let full_path = dir.path().join("full.wal");
    {
        let mut w = WalWriter::open(&full_path, DIM as u32).unwrap();
        for i in 0..20 {
            w.append_event(&ev(i), 0).unwrap();
        }
    }
    let full_bytes = std::fs::read(&full_path).unwrap();

    for cut_pct in [10usize, 30, 50, 70, 95] {
        let cut = full_bytes.len() * cut_pct / 100;
        let path = dir.path().join(format!("trunc_{cut_pct}.wal"));
        std::fs::write(&path, &full_bytes[..cut]).unwrap();

        let mut r = WalReader::open(&path, Some(DIM as u32)).unwrap();
        let mut recovered = Vec::new();
        loop {
            match r.read_entry() {
                Ok(Some((evt, _ns))) => recovered.push(evt),
                Ok(None) => break,
                Err(e) => panic!("cut {cut_pct}%: a truncated tail must not hard-error, got {e}"),
            }
        }
        for (idx, evt) in recovered.iter().enumerate() {
            if let KernelEvent::InsertRecord { id, .. } = evt {
                assert_eq!(id.0, idx as u32, "cut {cut_pct}%: recovered entries must be gap-free and in order");
            }
        }
        assert!(recovered.len() < 20, "cut {cut_pct}%: a real truncation must lose at least the last entry");
    }
}

// ── Simple WAL: truncated WAL (shorter than the header) ─────────────────

#[test]
fn wal_shorter_than_header_errors_on_first_read_not_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("short.wal");
    std::fs::write(&path, [0u8; 10]).unwrap(); // header is 16 bytes

    // `WalReader::open` is lazy — it doesn't read the header until the
    // first `read_entry()` call, so opening itself must still succeed.
    let mut r = WalReader::open(&path, Some(DIM as u32)).expect("open is lazy, must not fail here");
    assert!(r.read_entry().is_err(), "a header shorter than 16 bytes must fail on first read");
}

// ── Event log: partial / truncated trailing entry (byte-exact) ─────────

#[test]
fn event_log_truncated_trailing_entry_recovers_complete_prefix_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.log");
    {
        let mut w = EventLogWriter::open(&path, Some(DIM as u32)).unwrap();
        for i in 0..15 {
            w.append(&LogEntry::Event(ev(i))).unwrap();
        }
    }
    let full_bytes = std::fs::read(&path).unwrap();

    // Truncation points a fuzzy "within 100 bytes of EOF" heuristic could
    // get wrong on a segment this size — byte-exact truncation via
    // WireError::Truncated must tolerate every one of them.
    for cut_pct in [20usize, 50, 80, 99] {
        let cut = full_bytes.len() * cut_pct / 100;
        std::fs::write(&path, &full_bytes[..cut]).unwrap();

        let w = EventLogWriter::open(&path, Some(DIM as u32))
            .unwrap_or_else(|e| panic!("cut {cut_pct}%: a trailing partial write must be tolerated, got {e}"));
        assert!(w.event_count() < 15, "cut {cut_pct}%: truncation must lose at least the last event");
        drop(w);
    }
}

#[test]
fn recover_from_event_log_tolerates_trailing_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.log");
    {
        let mut w = EventLogWriter::open(&path, Some(DIM as u32)).unwrap();
        for i in 0..15 {
            w.append(&LogEntry::Event(ev(i))).unwrap();
        }
    }
    let full_bytes = std::fs::read(&path).unwrap();
    let cut = full_bytes.len() * 60 / 100;
    std::fs::write(&path, &full_bytes[..cut]).unwrap();

    let (state, _journal, count) =
        recover_from_event_log(&path).expect("a truncated tail must be tolerated, not hard-error");
    assert!(count < 15);
    for i in 0..count as u32 {
        assert!(state.get_record(RecordId(i)).is_some(), "record {i} should have recovered");
    }
}

// ── Event log: checksum mismatch mid-file is a hard error ──────────────
// (never tolerated as "trailing partial", regardless of segment length)

#[test]
fn event_log_mid_file_corruption_is_rejected_not_silently_truncated() {
    // Flip a byte inside the FIRST entry's payload of a large-enough
    // segment that the corrupted offset is NOT within the old fuzzy
    // "within 100 bytes of EOF" heuristic's tolerance window — this must
    // be a hard error under either version of the code, exercised here as
    // a baseline before the sharper same-entry-as-EOF case below.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.log");
    {
        let mut w = EventLogWriter::open(&path, Some(DIM as u32)).unwrap();
        for i in 0..30 {
            w.append(&LogEntry::Event(ev(i))).unwrap();
        }
    }
    let mut bytes = std::fs::read(&path).unwrap();
    let header_len = 48; // V4 header size
    bytes[header_len + 5] ^= 0xFF; // inside the first entry's prev_hash field
    std::fs::write(&path, &bytes).unwrap();

    assert!(
        EventLogWriter::open(&path, Some(DIM as u32)).is_err(),
        "a corrupted early entry must be rejected on reopen, not silently truncated"
    );
    assert!(
        recover_from_event_log(&path).is_err(),
        "a corrupted early entry must be rejected on recovery, not silently truncated"
    );
}

#[test]
fn event_log_corruption_in_last_entry_is_rejected_not_mistaken_for_truncation() {
    // Flip a byte inside the LAST entry's payload WITHOUT changing the
    // file's length — this is exactly the case the old "within 100 bytes
    // of EOF" heuristic could get wrong: a corrupted-but-complete-length
    // final entry sits near EOF by definition, so the heuristic would
    // silently drop it and report success with one fewer event, instead of
    // surfacing the corruption. `WireError::Truncated` is never raised for
    // a right-length-wrong-content entry (only for too-few-bytes), so the
    // structural check must still hard-error here.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.log");
    {
        let mut w = EventLogWriter::open(&path, Some(DIM as u32)).unwrap();
        for i in 0..5 {
            w.append(&LogEntry::Event(ev(i))).unwrap();
        }
    }
    let mut bytes = std::fs::read(&path).unwrap();
    let corrupt_at = bytes.len() - 6; // inside the last entry's CRC-covered payload
    bytes[corrupt_at] ^= 0xFF;
    std::fs::write(&path, &bytes).unwrap();

    let reopen_result = EventLogWriter::open(&path, Some(DIM as u32));
    assert!(
        reopen_result.is_err(),
        "a corrupted last entry must be rejected on reopen, not mistaken for a trailing partial write"
    );
    assert!(
        recover_from_event_log(&path).is_err(),
        "a corrupted last entry must be rejected on recovery, not mistaken for a trailing partial write"
    );
}

// ── Event log: truncated WAL (shorter than the header) ──────────────────

#[test]
fn event_log_shorter_than_header_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.log");
    std::fs::write(&path, [0u8; 10]).unwrap(); // header is 16 (v2) / 48 (v3/v4) bytes

    assert!(EventLogWriter::open(&path, Some(DIM as u32)).is_err());
    assert!(read_all_segments(&path, Some(DIM as u32)).is_err());
}

// ── Replay: duplicate / out-of-order record ids must be rejected ───────

#[test]
fn replay_events_rejects_duplicate_record_id() {
    let events: Vec<(u16, KernelEvent)> = vec![(0, ev(0)), (0, ev(0))];
    assert!(
        replay_events(&events).is_err(),
        "a duplicate record id in the event stream must be rejected during replay"
    );
}

#[test]
fn replay_events_rejects_out_of_order_record_id() {
    // The kernel requires sequential ids (next_id() == id); id 5 arriving
    // right after id 0 skips ahead and must be rejected, not silently
    // accepted with a gap in the record pool.
    let events: Vec<(u16, KernelEvent)> = vec![(0, ev(0)), (0, ev(5))];
    assert!(
        replay_events(&events).is_err(),
        "an out-of-order record id must be rejected during replay"
    );
}

#[test]
fn recover_from_event_log_rejects_a_hand_crafted_duplicate_id_log() {
    // End to end: `EventCommitter`'s shadow-apply gate normally prevents a
    // duplicate id from ever reaching the log at write time (see
    // event_commit.rs::test_commit_rejects_invalid_event). This test drives
    // `EventLogWriter` directly, bypassing that gate — simulating a
    // hand-edited or corrupted-but-well-formed log — and checks that
    // RECOVERY is the second line of defense: it must fail loudly rather
    // than silently dropping or overwriting the earlier record.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.log");
    let mut w = EventLogWriter::open(&path, Some(DIM as u32)).unwrap();
    w.append(&LogEntry::Event(ev(0))).unwrap();
    w.append(&LogEntry::Event(ev(0))).unwrap();
    drop(w);

    assert!(
        recover_from_event_log(&path).is_err(),
        "recovery must reject a log containing a duplicate record id"
    );
}
