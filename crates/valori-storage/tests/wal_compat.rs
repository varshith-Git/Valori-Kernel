// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! WAL format compatibility corpus.
//!
//! The `.wal` files under `tests/fixtures/` are COMMITTED BYTES written by
//! a specific WAL version. Every test here must pass forever, unchanged.
//! If a test fails it means the WAL format or replay logic changed in a way
//! that breaks backward compatibility.
//!
//! `generate_wal_fixtures` (ignored) writes the files. Run it once per
//! format version, then commit the new files.

use valori_kernel::event::KernelEvent;
use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;
use valori_storage::wal_reader::WalReader;
use valori_storage::wal_writer::WalWriter;

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn hex(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn replay_wal(path: &std::path::Path) -> (KernelState, usize) {
    let mut state = KernelState::new();
    let reader = WalReader::open(path, None).expect("WAL must open");
    let mut count = 0;
    for result in reader {
        let (evt, ns) = result.expect("WAL entry must decode");
        state.apply_event_ns(&evt, ns).expect("WAL event must apply");
        count += 1;
    }
    (state, count)
}

// ── Forever-replay tests ──────────────────────────────────────────────────────

#[test]
fn wal_v1_inserts_replays_forever() {
    let (state, count) = replay_wal(&fixture_path("wal_v1_inserts.wal"));
    let expected = std::fs::read_to_string(fixture_path("wal_v1_inserts.hash"))
        .expect("wal_v1_inserts.hash must exist");
    assert_eq!(count, 20, "fixture must contain exactly 20 events");
    assert_eq!(state.record_count(), 20);
    assert_eq!(
        hex(&hash_state_blake3(&state)),
        expected.trim(),
        "WAL replay hash changed — WAL format or replay logic broke compatibility"
    );
}

#[test]
fn wal_v1_namespace_replays_forever() {
    let (state, count) = replay_wal(&fixture_path("wal_v1_namespace.wal"));
    let expected = std::fs::read_to_string(fixture_path("wal_v1_namespace.hash"))
        .expect("wal_v1_namespace.hash must exist");
    assert_eq!(count, 16, "fixture must contain exactly 16 events");
    assert_eq!(state.record_count(), 16);
    assert_eq!(
        hex(&hash_state_blake3(&state)),
        expected.trim(),
        "namespace-WAL replay hash changed — WAL format or replay logic broke compatibility"
    );
}

// ── Fixture generator ─────────────────────────────────────────────────────────

/// `cargo test -p valori-storage --test wal_compat generate_wal_fixtures -- --ignored --nocapture`
#[test]
#[ignore]
fn generate_wal_fixtures() {
    use std::fs;

    let dir = fixture_path("");
    fs::create_dir_all(&dir).unwrap();

    // Fixture 1: 20 inserts in namespace 0
    {
        let path = dir.join("wal_v1_inserts.wal");
        let mut w = WalWriter::open(&path, 4).unwrap();
        for i in 0u32..20 {
            let data = (0..4).map(|d| FxpScalar((i * 1000 + d * 7) as i32)).collect();
            w.append_event(&KernelEvent::InsertRecord {
                id: RecordId(i),
                vector: FxpVector { data },
                metadata: if i % 4 == 0 { Some(format!("{{\"n\":{i}}}").into_bytes()) } else { None },
                tag: i as u64 % 5,
            }, 0).unwrap();
        }
        drop(w);

        let (state, _) = replay_wal(&path);
        let hash = hex(&hash_state_blake3(&state));
        fs::write(dir.join("wal_v1_inserts.hash"), &hash).unwrap();
        println!("wal_v1_inserts.wal: 20 events, hash {hash}");
    }

    // Fixture 2: 16 inserts spread across 2 namespaces (ns 0 and ns 1)
    {
        let path = dir.join("wal_v1_namespace.wal");
        let mut w = WalWriter::open(&path, 4).unwrap();
        for i in 0u32..8 {
            let data = (0..4).map(|d| FxpScalar((i * 1000 + d * 7) as i32)).collect();
            w.append_event(&KernelEvent::InsertRecord {
                id: RecordId(i),
                vector: FxpVector { data },
                metadata: None,
                tag: 0,
            }, 0).unwrap();
        }
        for i in 8u32..16 {
            let data = (0..4).map(|d| FxpScalar((i * 500 + d * 13) as i32 - 1000)).collect();
            w.append_event(&KernelEvent::InsertRecord {
                id: RecordId(i),
                vector: FxpVector { data },
                metadata: None,
                tag: 1,
            }, 1).unwrap();
        }
        drop(w);

        let (state, _) = replay_wal(&path);
        let hash = hex(&hash_state_blake3(&state));
        fs::write(dir.join("wal_v1_namespace.hash"), &hash).unwrap();
        println!("wal_v1_namespace.wal: 16 events, hash {hash}");
    }
}
