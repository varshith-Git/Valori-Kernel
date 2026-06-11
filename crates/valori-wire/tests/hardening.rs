// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Verifier-hardening tests (Phase 1.7).
//!
//! Guards that crafted / oversized inputs are rejected before they can cause
//! OOM or infinite loops.

use valori_wire::{
    encode_header_v3, parse_header, WireError, MAX_DIM, MAX_ENTRY_DECODE_BYTES,
    MAX_ENTRIES_PER_SEGMENT, METADATA_CAP,
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
fn truncated_entry_returns_decode_error_not_panic() {
    let header = encode_header_v3(4, 1, 0, &[0u8; 32]);
    let seg = parse_header(&header).unwrap();
    // Pass only 3 bytes after the header — not enough for any valid entry.
    let body = [0xAA, 0xBB, 0xCC];
    let err = valori_wire::decode_entry(seg.version, &body).unwrap_err();
    assert!(
        matches!(err, WireError::Decode(_)),
        "truncated body should be Decode error, got {err:?}"
    );
}

#[test]
fn empty_body_returns_decode_error_not_panic() {
    let header = encode_header_v3(4, 1, 0, &[0u8; 32]);
    let seg = parse_header(&header).unwrap();
    let err = valori_wire::decode_entry(seg.version, &[]).unwrap_err();
    assert!(
        matches!(err, WireError::Decode(_)),
        "empty body should be Decode error, got {err:?}"
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
