// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Wire-format contract tests: the verifier's mirror of the node's on-disk
//! format must decode exactly what `EventLogWriter` writes, and the hash
//! chain must localize tampering.

use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::vector::FxpVector;
use valori_verify::wire::{
    chain_advance, encode_header, hex, parse_header, ChainedEntry, LogEntry, HEADER_SIZE,
    SUPPORTED_VERSION,
};

fn event(i: u32) -> KernelEvent {
    KernelEvent::InsertRecord {
        id: RecordId(i),
        vector: FxpVector::new_zeros(4),
        metadata: None,
        tag: 0,
    }
}

#[test]
fn header_roundtrip() {
    let bytes = encode_header(384);
    let h = parse_header(&bytes).expect("own header must parse");
    assert_eq!(h.version, SUPPORTED_VERSION);
    assert_eq!(h.dim, 384);
}

#[test]
fn header_rejects_wrong_version() {
    let mut bytes = encode_header(4);
    bytes[0] = 99;
    assert!(parse_header(&bytes).is_err());
}

#[test]
fn header_rejects_short_file() {
    assert!(parse_header(&[0u8; HEADER_SIZE - 1]).is_err());
}

#[test]
fn chain_advance_is_deterministic() {
    let mut a = [0u8; 32];
    let mut b = [0u8; 32];
    for i in 0..20 {
        a = chain_advance(&a, 1_000 + i as u64, &LogEntry::Event(event(i)));
        b = chain_advance(&b, 1_000 + i as u64, &LogEntry::Event(event(i)));
    }
    assert_eq!(a, b);
    assert_ne!(a, [0u8; 32]);
}

#[test]
fn chain_detects_entry_substitution() {
    let honest: Vec<[u8; 32]> = {
        let mut head = [0u8; 32];
        (0..10u32)
            .map(|i| {
                head = chain_advance(&head, 1_000, &LogEntry::Event(event(i)));
                head
            })
            .collect()
    };

    // Replay with entry #5 swapped — every head from #5 onward must differ,
    // which is exactly how the verifier pinpoints the first tampered entry.
    let mut head = [0u8; 32];
    for i in 0..10u32 {
        let e = if i == 5 { event(99) } else { event(i) };
        // (a substituted id will also fail kernel replay, but the chain
        //  catches it without replaying at all)
        head = chain_advance(&head, 1_000, &LogEntry::Event(e));
        if i < 5 {
            assert_eq!(head, honest[i as usize], "heads before the tamper must match");
        } else {
            assert_ne!(head, honest[i as usize], "heads from the tamper on must differ");
        }
    }
}

#[test]
fn wire_decodes_what_the_node_writes() {
    // The critical cross-crate contract: bytes produced by valori-node's
    // EventLogWriter must decode with valori-verify's mirrored types.
    // This is the test that would have caught the v1→v2 drift.
    use valori_node::events::event_log::{EventLogWriter, LogEntry as NodeLogEntry};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.log");

    let mut w = EventLogWriter::open(&path, Some(4)).unwrap();
    for i in 0..5u32 {
        w.append(&NodeLogEntry::Event(event(i))).unwrap();
    }
    let node_head = *w.chain_head();
    drop(w);

    let bytes = std::fs::read(&path).unwrap();
    let header = parse_header(&bytes).expect("node header must parse with verify's parser");
    assert_eq!(header.dim, 4);

    let mut offset = HEADER_SIZE;
    let mut head = [0u8; 32];
    let mut count = 0u32;
    while offset < bytes.len() {
        let (chained, n): (ChainedEntry, usize) = bincode::serde::decode_from_slice(
            &bytes[offset..],
            bincode::config::standard(),
        )
        .expect("node-written entry must decode with verify's mirror");
        assert_eq!(chained.prev_hash, head, "chain must verify");
        head = chain_advance(&head, chained.wall_time_secs, &chained.entry);
        offset += n;
        count += 1;
    }
    assert_eq!(count, 5);
    assert_eq!(
        hex(&head),
        hex(&node_head),
        "verifier-recomputed chain head must equal the writer's"
    );
}
