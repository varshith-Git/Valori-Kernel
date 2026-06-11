// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! FxpFormat seam: format identifiers, hash-domain separation, and the
//! snapshot V5 format byte.

use valori_kernel::event::KernelEvent;
use valori_kernel::fxp::format::{
    format_name, parse_format, FxpFormat, Q16_16, Q32_32, Q8_8, ACTIVE_FORMAT_ID,
};
use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::snapshot::decode::decode_state;
use valori_kernel::snapshot::encode::encode_state;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::vector::FxpVector;

#[test]
fn format_ids_are_distinct_and_stable() {
    // IDs are written into headers and the hash domain — append-only,
    // never reused, never renumbered.
    assert_eq!(Q16_16::FORMAT_ID, 1);
    assert_eq!(Q8_8::FORMAT_ID, 2);
    assert_eq!(Q32_32::FORMAT_ID, 3);
    assert_eq!(ACTIVE_FORMAT_ID, Q16_16::FORMAT_ID);
}

#[test]
fn accumulators_are_wider_than_reprs() {
    assert_eq!(core::mem::size_of::<<Q16_16 as FxpFormat>::Wide>(), 2 * core::mem::size_of::<<Q16_16 as FxpFormat>::Repr>());
    assert_eq!(core::mem::size_of::<<Q8_8 as FxpFormat>::Wide>(), 2 * core::mem::size_of::<<Q8_8 as FxpFormat>::Repr>());
    assert_eq!(core::mem::size_of::<<Q32_32 as FxpFormat>::Wide>(), 2 * core::mem::size_of::<<Q32_32 as FxpFormat>::Repr>());
}

#[test]
fn parse_and_name_roundtrip() {
    for (name, id) in [("q16.16", 1u8), ("q8.8", 2), ("q32.32", 3)] {
        assert_eq!(parse_format(name), Some(id));
        assert_eq!(format_name(id), Some(name));
    }
    assert_eq!(parse_format("Q16.16"), Some(1), "parsing is case-insensitive");
    assert_eq!(parse_format(" q16.16 "), Some(1), "parsing trims whitespace");
    assert_eq!(parse_format("float32"), None);
    assert_eq!(format_name(200), None);
}

/// Pins the hash domain. If this test fails, the state-hash input schema
/// changed — that must be a DELIBERATE, versioned event (bump
/// STATE_HASH_DOMAIN_VERSION, update this constant in the same commit,
/// and document the break in the phase report). Never an accident.
#[test]
fn empty_state_hash_is_pinned() {
    let h = hash_state_blake3(&KernelState::new());
    assert_eq!(
        h.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        "4eeaa41d0b2eb66651bdbb252f4b91a7fa191d3f1cee4d311b6056966fba4d4a",
        "state-hash domain changed — see test doc comment before touching this"
    );
}

#[test]
fn snapshot_v5_roundtrip_preserves_hash() {
    let mut state = KernelState::new();
    for i in 0..5u32 {
        state
            .apply_event(&KernelEvent::InsertRecord {
                id: RecordId(i),
                vector: FxpVector::new_zeros(4),
                metadata: Some(vec![i as u8]),
                tag: i as u64,
            })
            .unwrap();
    }
    let before = hash_state_blake3(&state);

    let mut buf = vec![0u8; 1 << 16];
    let len = encode_state(&state, &mut buf).unwrap();
    buf.truncate(len);

    let restored = decode_state(&buf).unwrap();
    assert_eq!(hash_state_blake3(&restored), before);
}

#[test]
fn snapshot_with_foreign_format_is_refused() {
    let state = KernelState::new();
    let mut buf = vec![0u8; 4096];
    let len = encode_state(&state, &mut buf).unwrap();
    buf.truncate(len);

    // Format byte position: MAGIC(4) + schema_ver(4) + state_version(8)
    // + 4 capacity u32s(16) = offset 32.
    assert_eq!(buf[32], ACTIVE_FORMAT_ID, "format byte location moved — update this test");
    buf[32] = Q8_8::FORMAT_ID;
    assert!(
        decode_state(&buf).is_err(),
        "restoring a snapshot from a different arithmetic format must be refused"
    );
}
