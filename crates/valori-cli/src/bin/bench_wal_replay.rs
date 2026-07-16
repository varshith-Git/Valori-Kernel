// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! P5 — WAL append + replay throughput benchmark.
//!
//! Measures events/sec for both directions of the durable write path:
//! `WalWriter::append_event` (the crash-safe write every mutation takes)
//! and `WalReader::read_entry` (the recovery path a restart replays through
//! before the node accepts traffic — its speed is the node's recovery-time
//! floor). Self-contained: generates synthetic vectors in-process, no
//! external dataset required.
//!
//! Run: `cargo run --release -p valori-cli --bin bench_wal_replay`

use std::time::Instant;
use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;
use valori_storage::wal_reader::WalReader;
use valori_storage::wal_writer::WalWriter;

const DIM: usize = 128;
const SCALES: &[usize] = &[10_000, 100_000];

fn make_event(id: u32, dim: usize) -> KernelEvent {
    // Deterministic pseudo-random-looking values — not actually random, just
    // varied enough to avoid the WAL/bincode path degenerating on an
    // all-zeros vector.
    let data = (0..dim)
        .map(|d| FxpScalar((((id as i64) * 2654435761 + d as i64 * 97) % 65536 - 32768) as i32))
        .collect();
    KernelEvent::InsertRecord {
        id: RecordId(id),
        vector: FxpVector { data },
        metadata: None,
        tag: id as u64 % 8,
    }
}

fn main() -> anyhow::Result<()> {
    println!("WAL append + replay throughput  (dim={DIM}, release build recommended)\n");
    println!(
        "{:>10} | {:>12} | {:>14} | {:>10} | {:>12} | {:>14}",
        "events", "append time", "append ev/s", "WAL size", "replay time", "replay ev/s"
    );
    println!("{}", "-".repeat(88));

    for &n in SCALES {
        let dir = std::env::temp_dir().join(format!("valori_bench_wal_{}_{n}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let wal_path = dir.join("bench.wal");

        // ── Append: the durable write path every InsertRecord goes through ──
        let mut writer = WalWriter::open(&wal_path, DIM as u32)?;
        let t_append = Instant::now();
        for i in 0..n as u32 {
            writer.append_event(&make_event(i, DIM), 0)?;
        }
        writer.sync()?;
        let append_elapsed = t_append.elapsed();
        let bytes_written = writer.bytes_written();
        drop(writer);

        // ── Replay: the crash-recovery path, read start to finish ──────────
        // Iterate via `WalReader`'s `IntoIterator` impl, not a raw
        // `read_entry()` loop: calling `read_entry()` directly at clean EOF
        // surfaces as `Err(Deserialization(Io(UnexpectedEof)))` rather than
        // `Ok(None)` (bincode 2.0.1 raises `DecodeError::Io`, not the
        // `DecodeError::UnexpectedEnd` variant `read_entry()` special-cases)
        // — only the iterator's `fill_buf()` pre-check avoids ever calling
        // `read_entry()` past the last entry. That's a real bug in the
        // direct API (P7's WAL-hardening territory, not this benchmark's),
        // flagged separately; this is the supported call pattern today.
        let reader = WalReader::open(&wal_path, Some(DIM as u32))?;
        let t_replay = Instant::now();
        let mut replayed = 0usize;
        for entry in reader {
            entry?;
            replayed += 1;
        }
        let replay_elapsed = t_replay.elapsed();

        assert_eq!(replayed, n, "replay must recover every appended event — WAL is lossy or truncated");

        println!(
            "{:>10} | {:>10.3?} | {:>10} ev/s | {:>7.2} MB | {:>10.3?} | {:>10} ev/s",
            n,
            append_elapsed,
            (n as f64 / append_elapsed.as_secs_f64()) as u64,
            bytes_written as f64 / 1_048_576.0,
            replay_elapsed,
            (n as f64 / replay_elapsed.as_secs_f64()) as u64,
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    println!("\nNote: append_event() flushes to the OS on every call (matches the real write path —");
    println!("not batched here for realism), so append throughput is fsync/flush-bound, not CPU-bound.");

    Ok(())
}
