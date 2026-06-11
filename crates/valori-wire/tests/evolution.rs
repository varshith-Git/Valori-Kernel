// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Evolution-policy enforcement.
//!
//! The fixture files under `tests/fixtures/` are COMMITTED BYTES written by
//! historical versions of the wire format. They must decode, chain-verify,
//! and produce the recorded chain heads forever. If a refactor reorders an
//! enum variant, changes a field, or touches the chain formula, these tests
//! fail — that is their entire purpose. Do not regenerate fixtures to make
//! a failure go away; that defeats the policy (see crate README).
//!
//! `generate_fixtures` (ignored) writes the files; it was run once per
//! format version and should only ever run again to ADD a new version.

use valori_wire::{
    chain_advance, decode_entry, encode_entry, encode_header_v2, encode_header_v3, hex,
    parse_header, LogEntry, FORMAT_Q16_16, VERSION_V2, VERSION_V3,
};

use valori_kernel::event::KernelEvent;
use valori_kernel::types::enums::NodeKind;
use valori_kernel::types::id::{NodeId, RecordId};
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

const BASE_TIME: u64 = 1_750_000_000;

/// Deterministic fixture payload: 8 inserts + 1 node + 1 checkpoint.
fn fixture_entries() -> Vec<LogEntry> {
    let mut entries = Vec::new();
    for i in 0..8u32 {
        let data = (0..4).map(|d| FxpScalar((i * 1000 + d * 7) as i32)).collect();
        entries.push(LogEntry::Event(KernelEvent::InsertRecord {
            id: RecordId(i),
            vector: FxpVector { data },
            metadata: if i % 2 == 0 { Some(vec![i as u8; 4]) } else { None },
            tag: i as u64,
        }));
    }
    entries.push(LogEntry::Event(KernelEvent::CreateNode {
        id: NodeId(0),
        kind: NodeKind::Concept,
        record: Some(RecordId(0)),
    }));
    entries.push(LogEntry::Checkpoint {
        event_count: 9,
        snapshot_hash: [0x42; 32],
        timestamp: BASE_TIME,
    });
    entries
}

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn walk(bytes: &[u8]) -> (u64, u64, [u8; 32]) {
    let header = parse_header(bytes).expect("fixture header must parse forever");
    let mut head = header.prev_segment_chain_head;
    let mut offset = header.header_len;
    let mut events = 0u64;
    let mut checkpoints = 0u64;
    while offset < bytes.len() {
        let (e, n) = decode_entry(header.version, &bytes[offset..])
            .expect("fixture entry must decode forever");
        assert_eq!(e.prev_hash, head, "fixture chain must verify forever");
        head = chain_advance(header.version, &head, &e).unwrap();
        match e.entry {
            LogEntry::Event(_) => events += 1,
            LogEntry::Checkpoint { .. } => checkpoints += 1,
        }
        offset += n;
    }
    (events, checkpoints, head)
}

#[test]
fn v2_fixture_decodes_forever() {
    let bytes = std::fs::read(fixture_path("segment_v2.bin"))
        .expect("committed v2 fixture must exist");
    let (events, checkpoints, head) = walk(&bytes);
    assert_eq!(events, 9);
    assert_eq!(checkpoints, 1);
    assert_eq!(
        hex(&head),
        "481dd017606e125d921552d183c6a678d382b618caeb9150155f981d6b308d07",
        "v2 fixture chain head changed — the wire format or chain formula broke compatibility"
    );
}

#[test]
fn v3_fixture_decodes_forever() {
    let bytes = std::fs::read(fixture_path("segment_v3.bin"))
        .expect("committed v3 fixture must exist");
    let header = parse_header(&bytes).unwrap();
    assert_eq!(header.version, VERSION_V3);
    assert_eq!(header.format_id, FORMAT_Q16_16);
    assert_eq!(header.segment_seq, 3);

    let (events, checkpoints, head) = walk(&bytes);
    assert_eq!(events, 9);
    assert_eq!(checkpoints, 1);
    assert_eq!(
        hex(&head),
        "221c42b81e15578399e036035739314f5889a1bf5007449280a8b8465b56e0b9",
        "v3 fixture chain head changed — the wire format or chain formula broke compatibility"
    );
}

/// One-time fixture generator. Run manually:
/// `cargo test -p valori-wire --test evolution generate_fixtures -- --ignored --nocapture`
#[test]
#[ignore]
fn generate_fixtures() {
    std::fs::create_dir_all(fixture_path("")).unwrap();

    // v2 segment (genesis, zero-seeded chain).
    let mut bytes = encode_header_v2(4).to_vec();
    let mut head = [0u8; 32];
    for (i, entry) in fixture_entries().iter().enumerate() {
        let t = BASE_TIME + i as u64;
        bytes.extend(encode_entry(VERSION_V2, &head, t, None, entry).unwrap());
        head = valori_wire::chain_advance_v2(&head, t, entry);
    }
    std::fs::write(fixture_path("segment_v2.bin"), &bytes).unwrap();
    println!("v2 final chain head: {}", hex(&head));

    // v3 segment (seq 3, spliced to a fixed previous head, request ids on
    // even entries so the Option=Some path is pinned too).
    let prev = [0x11u8; 32];
    let mut bytes = encode_header_v3(4, FORMAT_Q16_16, 3, &prev).to_vec();
    let mut head = prev;
    for (i, entry) in fixture_entries().iter().enumerate() {
        let t = BASE_TIME + i as u64;
        let rid = if i % 2 == 0 { Some([i as u8; 16]) } else { None };
        bytes.extend(encode_entry(VERSION_V3, &head, t, rid, entry).unwrap());
        head = valori_wire::chain_advance_v3(&head, t, rid, entry);
    }
    std::fs::write(fixture_path("segment_v3.bin"), &bytes).unwrap();
    println!("v3 final chain head: {}", hex(&head));
}
