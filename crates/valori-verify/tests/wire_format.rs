// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Wire-format contract tests: the shared `valori-wire` definitions must
//! decode exactly what `EventLogWriter` writes (v3 — and v2 legacy files),
//! and the hash chain must localize tampering.

use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::vector::FxpVector;
use valori_verify::wire::{
    chain_advance, chain_advance_v3, decode_entry, encode_header_v3, hex, parse_header, LogEntry,
    FORMAT_Q16_16, HEADER_SIZE_V3, VERSION_V2, VERSION_V3, VERSION_V4,
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
fn header_roundtrip_v3() {
    let bytes = encode_header_v3(384, FORMAT_Q16_16, 7, &[0xAB; 32]);
    let h = parse_header(&bytes).expect("own header must parse");
    assert_eq!(h.version, VERSION_V3);
    assert_eq!(h.dim, 384);
    assert_eq!(h.format_id, FORMAT_Q16_16);
    assert_eq!(h.segment_seq, 7);
    assert_eq!(h.prev_segment_chain_head, [0xAB; 32]);
    assert_eq!(h.header_len, HEADER_SIZE_V3);
}

#[test]
fn header_rejects_unknown_version() {
    let mut bytes = encode_header_v3(4, FORMAT_Q16_16, 0, &[0u8; 32]);
    bytes[0] = 99;
    assert!(parse_header(&bytes).is_err());
}

#[test]
fn header_rejects_unknown_format_id() {
    let bytes = encode_header_v3(4, 200, 0, &[0u8; 32]);
    assert!(
        parse_header(&bytes).is_err(),
        "unknown arithmetic format must be refused"
    );
}

#[test]
fn header_rejects_short_file() {
    assert!(parse_header(&[0u8; 4]).is_err());
}

#[test]
fn v2_headers_still_parse() {
    let bytes = valori_verify::wire::encode_header_v2(16);
    let h = parse_header(&bytes).expect("legacy v2 header must keep parsing forever");
    assert_eq!(h.version, VERSION_V2);
    assert_eq!(h.dim, 16);
    assert_eq!(h.segment_seq, 0);
    assert_eq!(h.prev_segment_chain_head, [0u8; 32]);
}

#[test]
fn chain_advance_is_deterministic_and_request_id_sensitive() {
    let mut a = [0u8; 32];
    let mut b = [0u8; 32];
    for i in 0..20 {
        a = chain_advance_v3(&a, 1_000 + i as u64, None, &LogEntry::Event(event(i)));
        b = chain_advance_v3(&b, 1_000 + i as u64, None, &LogEntry::Event(event(i)));
    }
    assert_eq!(a, b);
    assert_ne!(a, [0u8; 32]);

    // request_id participates in the chain.
    let with_rid = chain_advance_v3(
        &[0u8; 32],
        1_000,
        Some([1u8; 16]),
        &LogEntry::Event(event(0)),
    );
    let without = chain_advance_v3(&[0u8; 32], 1_000, None, &LogEntry::Event(event(0)));
    assert_ne!(with_rid, without);
}

#[test]
fn chain_detects_entry_substitution() {
    let honest: Vec<[u8; 32]> = {
        let mut head = [0u8; 32];
        (0..10u32)
            .map(|i| {
                head = chain_advance_v3(&head, 1_000, None, &LogEntry::Event(event(i)));
                head
            })
            .collect()
    };

    // Replay with entry #5 swapped — every head from #5 onward must differ,
    // which is exactly how the verifier pinpoints the first tampered entry.
    let mut head = [0u8; 32];
    for i in 0..10u32 {
        let e = if i == 5 { event(99) } else { event(i) };
        head = chain_advance_v3(&head, 1_000, None, &LogEntry::Event(e));
        if i < 5 {
            assert_eq!(
                head, honest[i as usize],
                "heads before the tamper must match"
            );
        } else {
            assert_ne!(
                head, honest[i as usize],
                "heads from the tamper on must differ"
            );
        }
    }
}

#[test]
fn wire_decodes_what_the_node_writes() {
    // The critical cross-crate contract: bytes produced by valori-node's
    // EventLogWriter must decode with the shared wire definitions.
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
    let header = parse_header(&bytes).expect("node header must parse");
    assert_eq!(header.version, VERSION_V4, "new node files are v4");
    assert_eq!(header.dim, 4);
    assert_eq!(header.segment_seq, 0);

    let mut offset = header.header_len;
    let mut head = header.prev_segment_chain_head;
    let mut count = 0u32;
    while offset < bytes.len() {
        let (chained, n) = decode_entry(header.version, &bytes[offset..])
            .expect("node-written entry must decode with the shared wire types");
        assert_eq!(chained.prev_hash, head, "chain must verify");
        head = chain_advance(header.version, &head, &chained).unwrap();
        offset += n;
        count += 1;
    }
    assert_eq!(count, 5);
    assert_eq!(
        hex(&head),
        hex(&node_head),
        "recomputed chain head must equal the writer's"
    );
}

#[test]
fn rotation_splice_is_verifiable_across_segments() {
    // Archive + new segment: the new header's prev_segment_chain_head must
    // equal the archived segment's final chain head, so a verifier can prove
    // no segment was removed or substituted.
    use valori_node::events::event_log::{EventLogWriter, LogEntry as NodeLogEntry};

    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("events.log");
    let archive = dir.path().join("events.log.0");

    let mut w = EventLogWriter::open(&live, Some(4)).unwrap();
    for i in 0..6u32 {
        w.append(&NodeLogEntry::Event(event(i))).unwrap();
    }
    w.rotate(&archive, None).unwrap();
    w.append(&NodeLogEntry::Event(event(6))).unwrap();
    drop(w);

    // Walk the archived segment to its final head.
    let a_bytes = std::fs::read(&archive).unwrap();
    let a_header = parse_header(&a_bytes).unwrap();
    let mut head = a_header.prev_segment_chain_head;
    let mut offset = a_header.header_len;
    while offset < a_bytes.len() {
        let (e, n) = decode_entry(a_header.version, &a_bytes[offset..]).unwrap();
        assert_eq!(e.prev_hash, head);
        head = chain_advance(a_header.version, &head, &e).unwrap();
        offset += n;
    }

    // The live segment must splice exactly there.
    let l_bytes = std::fs::read(&live).unwrap();
    let l_header = parse_header(&l_bytes).unwrap();
    assert_eq!(l_header.segment_seq, a_header.segment_seq + 1);
    assert_eq!(
        l_header.prev_segment_chain_head, head,
        "live segment must bind to the archived segment's final chain head"
    );

    // And its entries must continue the chain without a break.
    let mut head2 = l_header.prev_segment_chain_head;
    let mut offset2 = l_header.header_len;
    let mut live_events = 0;
    while offset2 < l_bytes.len() {
        let (e, n) = decode_entry(l_header.version, &l_bytes[offset2..]).unwrap();
        assert_eq!(e.prev_hash, head2, "chain must continue across the splice");
        head2 = chain_advance(l_header.version, &head2, &e).unwrap();
        offset2 += n;
        live_events += 1;
    }
    assert_eq!(live_events, 1);
}
