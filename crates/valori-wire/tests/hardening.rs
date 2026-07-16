// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Verifier-hardening tests (Phase 1.7).
//!
//! Guards that crafted / oversized inputs are rejected before they can cause
//! OOM or infinite loops.

use valori_wire::{
    encode_header_v3, parse_header, WireError, MAX_DIM, MAX_ENTRIES_PER_SEGMENT,
    MAX_ENTRY_DECODE_BYTES, METADATA_CAP,
};

// ── Constants sanity ──────────────────────────────────────────────────────────

#[test]
fn hardening_constants_are_sensible() {
    assert_eq!(MAX_ENTRY_DECODE_BYTES, 1 << 20, "1 MiB per entry");
    assert_eq!(MAX_DIM, 32_768);
    assert!(MAX_ENTRIES_PER_SEGMENT >= 1_000_000);
    assert!(METADATA_CAP >= 1024);
}

// ── Dim validation ────────────────────────────────────────────────────────────

#[test]
fn dim_zero_in_header_is_rejected() {
    let mut header = encode_header_v3(0, 1, 0, &[0u8; 32]);
    // dim is bytes [4..8] in the header
    header[4..8].copy_from_slice(&0u32.to_le_bytes());
    let err = parse_header(&header).unwrap_err();
    assert!(
        matches!(err, WireError::InvalidDim(0)),
        "expected InvalidDim(0), got {err:?}"
    );
}

#[test]
fn dim_exceeds_max_is_rejected() {
    let over = MAX_DIM + 1;
    let mut header = encode_header_v3(16, 1, 0, &[0u8; 32]);
    header[4..8].copy_from_slice(&over.to_le_bytes());
    let err = parse_header(&header).unwrap_err();
    assert!(
        matches!(err, WireError::InvalidDim(d) if d == over),
        "expected InvalidDim({over}), got {err:?}"
    );
}

#[test]
fn dim_at_exactly_max_is_accepted() {
    let header = encode_header_v3(MAX_DIM, 1, 0, &[0u8; 32]);
    let seg = parse_header(&header).unwrap();
    assert_eq!(seg.dim, MAX_DIM);
}

#[test]
fn dim_one_is_accepted() {
    let header = encode_header_v3(1, 1, 0, &[0u8; 32]);
    let seg = parse_header(&header).unwrap();
    assert_eq!(seg.dim, 1);
}

// ── Decode limit ──────────────────────────────────────────────────────────────

#[test]
fn truncated_entry_returns_truncated_error_not_panic() {
    let header = encode_header_v3(4, 1, 0, &[0u8; 32]);
    let seg = parse_header(&header).unwrap();
    // Pass only 3 bytes after the header — not enough for any valid entry.
    let body = [0xAA, 0xBB, 0xCC];
    let err = valori_wire::decode_entry(seg.version, &body).unwrap_err();
    assert!(
        matches!(err, WireError::Truncated),
        "too few bytes for any entry should be Truncated, got {err:?}"
    );
}

#[test]
fn empty_body_returns_truncated_error_not_panic() {
    let header = encode_header_v3(4, 1, 0, &[0u8; 32]);
    let seg = parse_header(&header).unwrap();
    let err = valori_wire::decode_entry(seg.version, &[]).unwrap_err();
    assert!(
        matches!(err, WireError::Truncated),
        "empty body should be Truncated, got {err:?}"
    );
}

// ── Error display ─────────────────────────────────────────────────────────────

#[test]
fn invalid_dim_error_includes_the_value() {
    let msg = WireError::InvalidDim(0).to_string();
    assert!(msg.contains('0'), "error should mention dim value: {msg}");
}

#[test]
fn decode_limit_exceeded_error_is_displayable() {
    let msg = WireError::DecodeLimitExceeded.to_string();
    assert!(!msg.is_empty());
}

// ── V4 per-entry CRC32 ────────────────────────────────────────────────────────

use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;
use valori_wire::{
    chain_advance, decode_entry, encode_entry, encode_header_v4, parse_header as ph4, LogEntry,
    CRC32_SUFFIX_LEN, VERSION_V4,
};

fn v4_entry() -> (LogEntry, Vec<u8>) {
    let entry = LogEntry::Event(KernelEvent::InsertRecord {
        id: RecordId(0),
        vector: FxpVector {
            data: vec![FxpScalar(100), FxpScalar(200)],
        },
        metadata: None,
        tag: 42,
    });
    let bytes = encode_entry(VERSION_V4, &[0u8; 32], 1_700_000_000, None, &entry)
        .expect("encode must succeed");
    (entry, bytes)
}

#[test]
fn v4_roundtrip_clean() {
    let (_, bytes) = v4_entry();
    let (decoded, consumed) = decode_entry(VERSION_V4, &bytes).expect("clean V4 must decode");
    assert_eq!(
        consumed,
        bytes.len(),
        "must consume all bytes including CRC suffix"
    );
    assert!(matches!(
        decoded.entry,
        LogEntry::Event(KernelEvent::InsertRecord { .. })
    ));
}

#[test]
fn v4_crc_suffix_present() {
    let (_, bytes) = v4_entry();
    // Last 4 bytes are the CRC suffix; the rest is the bincode payload.
    assert!(
        bytes.len() > CRC32_SUFFIX_LEN,
        "encoded V4 must be longer than just the CRC"
    );
}

#[test]
fn v4_bit_flip_in_payload_is_caught() {
    let (_, mut bytes) = v4_entry();
    // Flip a bit in the middle of the payload (well before the CRC suffix).
    let flip_pos = bytes.len() / 2;
    bytes[flip_pos] ^= 0x01;
    let result = decode_entry(VERSION_V4, &bytes);
    assert!(
        result.is_err(),
        "a flipped payload byte must be caught by CRC check"
    );
    let err = format!("{:?}", result.unwrap_err());
    assert!(
        err.contains("CRC32") || err.contains("Decode"),
        "error must mention CRC: {err}"
    );
}

#[test]
fn v4_crc_suffix_tamper_is_caught() {
    let (_, mut bytes) = v4_entry();
    // Flip a bit inside the CRC suffix itself.
    let crc_pos = bytes.len() - CRC32_SUFFIX_LEN;
    bytes[crc_pos] ^= 0xFF;
    let result = decode_entry(VERSION_V4, &bytes);
    assert!(result.is_err(), "a tampered CRC suffix must be caught");
}

#[test]
fn v4_truncated_crc_suffix_is_caught() {
    let (_, mut bytes) = v4_entry();
    // Remove the last byte of the CRC suffix.
    bytes.pop();
    let err = decode_entry(VERSION_V4, &bytes).unwrap_err();
    assert!(
        matches!(err, WireError::Truncated),
        "a missing CRC suffix byte is a truncation, not corruption: got {err:?}"
    );
}

#[test]
fn v4_chain_advance_matches_v3_formula() {
    // The chain hash for V4 must be identical to V3 (CRC is transport-only).
    let entry = LogEntry::Event(KernelEvent::InsertRecord {
        id: RecordId(1),
        vector: FxpVector {
            data: vec![FxpScalar(1), FxpScalar(2)],
        },
        metadata: None,
        tag: 0,
    });
    let prev_hash = [0xABu8; 32];
    let wall_time = 1_700_000_001u64;
    let req_id = None;

    use valori_wire::VERSION_V3;
    let v3_bytes = encode_entry(VERSION_V3, &prev_hash, wall_time, req_id, &entry).unwrap();
    let (v3_decoded, _) = decode_entry(VERSION_V3, &v3_bytes).unwrap();
    let v3_head = chain_advance(VERSION_V3, &prev_hash, &v3_decoded).unwrap();

    let v4_bytes = encode_entry(VERSION_V4, &prev_hash, wall_time, req_id, &entry).unwrap();
    let (v4_decoded, _) = decode_entry(VERSION_V4, &v4_bytes).unwrap();
    let v4_head = chain_advance(VERSION_V4, &prev_hash, &v4_decoded).unwrap();

    assert_eq!(v3_head, v4_head, "V4 chain hash must be identical to V3");
}

#[test]
fn metadata_cap_enforced_on_encode_not_decode() {
    use valori_wire::{WireError, METADATA_CAP};

    let oversized = LogEntry::Event(KernelEvent::InsertRecord {
        id: RecordId(1),
        vector: FxpVector {
            data: vec![FxpScalar(1)],
        },
        metadata: Some(vec![0u8; METADATA_CAP + 1]),
        tag: 0,
    });
    // Writers must refuse to put an oversized blob on disk...
    let err = encode_entry(VERSION_V4, &[0u8; 32], 1_700_000_000, None, &oversized)
        .expect_err("oversized metadata must be rejected at encode time");
    assert!(matches!(err, WireError::MetadataTooLarge(n) if n == METADATA_CAP + 1));

    // ...but at the cap it encodes and decodes fine.
    let at_cap = LogEntry::Event(KernelEvent::InsertRecord {
        id: RecordId(1),
        vector: FxpVector {
            data: vec![FxpScalar(1)],
        },
        metadata: Some(vec![0u8; METADATA_CAP]),
        tag: 0,
    });
    let bytes = encode_entry(VERSION_V4, &[0u8; 32], 1_700_000_000, None, &at_cap).unwrap();
    let (decoded, n) = decode_entry(VERSION_V4, &bytes).unwrap();
    assert_eq!(n, bytes.len());
    assert!(matches!(
        decoded.entry,
        LogEntry::Event(KernelEvent::InsertRecord { .. })
    ));
}
