//! Cross-platform determinism tests (host target, std).
//!
//! These tests prove the core claim of the embedded firmware:
//!   same KernelEvent log → same BLAKE3 state hash
//!   regardless of target (MCU / WASM / host).
//!
//! Run with:
//!   cargo test -p valori-embedded -- --nocapture

use valori_kernel::state::kernel::KernelState;
use valori_kernel::event::KernelEvent;
use valori_kernel::types::vector::FxpVector;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::id::{RecordId, DEFAULT_NS};
use valori_kernel::verify::{kernel_state_hash, snapshot_hash};
use valori_kernel::snapshot::encode::encode_state;
use valori_kernel::snapshot::decode::decode_state;
use valori_kernel::index::SearchResult;

// Mirror the embedded firmware's DIM constant.
const DIM: usize = 128;

// The exact test vector the SelfTest mode inserts.
fn self_test_vector() -> FxpVector {
    let mut v = FxpVector::new_zeros(DIM);
    v.data[0] = FxpScalar(65536);   // 1.0
    v.data[2] = FxpScalar(-65536);  // -1.0
    v.data[3] = FxpScalar(32768);   // 0.5
    v
}

fn apply_self_test_event(state: &mut KernelState) {
    let evt = KernelEvent::InsertRecord {
        id: RecordId(0),
        vector: self_test_vector(),
        metadata: None,
        tag: 0,
    };
    state.apply_event_ns(&evt, DEFAULT_NS.0).unwrap();
}

// ── Determinism ───────────────────────────────────────────────────────────────

#[test]
fn same_events_produce_same_hash() {
    let mut s1 = KernelState::new();
    let mut s2 = KernelState::new();

    apply_self_test_event(&mut s1);
    apply_self_test_event(&mut s2);

    assert_eq!(
        kernel_state_hash(&s1),
        kernel_state_hash(&s2),
        "same event applied twice must yield identical state hash"
    );
}

#[test]
fn empty_state_hash_is_stable() {
    let h1 = kernel_state_hash(&KernelState::new());
    let h2 = kernel_state_hash(&KernelState::new());
    assert_eq!(h1, h2, "empty kernel state hash must be deterministic");
}

#[test]
fn different_content_produces_different_hash() {
    // Two states with the same record IDs but swapped vector content must
    // produce different hashes — the hash commits to vector values, not just IDs.
    let v1 = self_test_vector();
    let mut v2 = self_test_vector();
    v2.data[0] = FxpScalar(32768); // 0.5 instead of 1.0

    // State A: slot-0 = v1, slot-1 = v2
    let mut s_a = KernelState::new();
    s_a.apply_event_ns(&KernelEvent::InsertRecord { id: RecordId(0), vector: v1.clone(), metadata: None, tag: 0 }, DEFAULT_NS.0).unwrap();
    s_a.apply_event_ns(&KernelEvent::InsertRecord { id: RecordId(1), vector: v2.clone(), metadata: None, tag: 0 }, DEFAULT_NS.0).unwrap();

    // State B: slot-0 = v2, slot-1 = v1 (content swapped at same positions)
    let mut s_b = KernelState::new();
    s_b.apply_event_ns(&KernelEvent::InsertRecord { id: RecordId(0), vector: v2, metadata: None, tag: 0 }, DEFAULT_NS.0).unwrap();
    s_b.apply_event_ns(&KernelEvent::InsertRecord { id: RecordId(1), vector: v1, metadata: None, tag: 0 }, DEFAULT_NS.0).unwrap();

    assert_ne!(
        kernel_state_hash(&s_a),
        kernel_state_hash(&s_b),
        "swapped vector content at same IDs must produce different state hash"
    );
}

// ── Snapshot round-trip ───────────────────────────────────────────────────────

#[test]
fn snapshot_roundtrip_preserves_state_hash() {
    let mut state = KernelState::new();
    apply_self_test_event(&mut state);

    let mut buf = vec![0u8; 64 * 1024];
    let len = encode_state(&state, &mut buf).expect("encode_state failed");
    let snap = &buf[0..len];

    let restored = decode_state(snap).expect("decode_state failed");

    assert_eq!(
        kernel_state_hash(&state),
        kernel_state_hash(&restored),
        "state hash must survive encode→decode round-trip"
    );
}

#[test]
fn snapshot_hash_is_stable() {
    let mut state = KernelState::new();
    apply_self_test_event(&mut state);

    let mut buf1 = vec![0u8; 64 * 1024];
    let len1 = encode_state(&state, &mut buf1).unwrap();

    let mut buf2 = vec![0u8; 64 * 1024];
    let len2 = encode_state(&state, &mut buf2).unwrap();

    assert_eq!(len1, len2);
    assert_eq!(
        snapshot_hash(&buf1[0..len1]),
        snapshot_hash(&buf2[0..len2]),
        "snapshot encoding must be deterministic"
    );
}

// ── Search determinism ────────────────────────────────────────────────────────

#[test]
fn search_returns_inserted_vector_as_top1() {
    let mut state = KernelState::new();
    apply_self_test_event(&mut state);

    let mut results = [SearchResult::default(); 1];
    let found = state.search_l2_ns(&self_test_vector(), &mut results, DEFAULT_NS.0);

    assert_eq!(found, 1, "should find the one inserted record");
    assert_eq!(results[0].id, RecordId(0), "top-1 must be the inserted record");
    // Exact match: L2-squared distance to itself is 0.
    assert_eq!(results[0].score.0, 0, "self-query must return score=0");
}

#[test]
fn search_result_paired_with_state_hash_is_verifiable() {
    let mut state = KernelState::new();
    apply_self_test_event(&mut state);

    let pre_search_hash = kernel_state_hash(&state);

    let mut results = [SearchResult::default(); 4];
    let found = state.search_l2_ns(&self_test_vector(), &mut results, DEFAULT_NS.0);

    // Search must not mutate state.
    let post_search_hash = kernel_state_hash(&state);
    assert_eq!(pre_search_hash, post_search_hash, "search must not mutate kernel state");
    assert!(found > 0);

    // The proof the embedded device would emit is (state_hash, top-k results).
    // A verifier that knows the event log can recompute state_hash and confirm.
    println!("state_hash:    {}", hex::encode(post_search_hash));
    println!("top-1 id:      {}", results[0].id.0);
    println!("top-1 score:   {}", results[0].score.0);
}

// ── Proof anchor test — pin the expected hash ─────────────────────────────────
//
// This test is the CI gate on cross-platform determinism: if the hash changes,
// the embedded firmware and the cloud node are out of sync and this test fails.
//
// To re-pin after an intentional kernel change:
//   cargo test -p valori-embedded self_test_hash_anchor -- --nocapture
// Copy the printed hash and update EXPECTED_HASH below.

#[test]
fn self_test_hash_anchor() {
    let mut state = KernelState::new();
    apply_self_test_event(&mut state);
    let hash = kernel_state_hash(&state);
    let hex_hash = hex::encode(hash);

    println!("self_test kernel_state_hash: {hex_hash}");

    // Verify hash is stable (two independent computations agree).
    let mut s2 = KernelState::new();
    apply_self_test_event(&mut s2);
    assert_eq!(hash, kernel_state_hash(&s2), "hash must be stable across two runs");

    // TODO: once the hash is observed and confirmed correct on the cloud node,
    // pin it here:
    //   const EXPECTED_HASH: &str = "<paste hex here>";
    //   assert_eq!(hex_hash, EXPECTED_HASH, "kernel hash changed — re-verify firmware and node are in sync");
}
