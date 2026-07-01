// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Kernel state-machine semantics: event application, ID sequencing,
//! dimension enforcement, and the record/node/edge lifecycle.

use valori_kernel::event::KernelEvent;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::enums::{EdgeKind, NodeKind};
use valori_kernel::types::id::{EdgeId, NodeId, RecordId};
use valori_kernel::types::vector::FxpVector;

const DIM: usize = 4;

fn insert(id: u32) -> KernelEvent {
    KernelEvent::InsertRecord {
        id: RecordId(id),
        vector: FxpVector::new_zeros(DIM),
        metadata: None,
        tag: 0,
    }
}

#[test]
fn insert_advances_record_count() {
    let mut state = KernelState::new();
    for i in 0..5 {
        state.apply_event(&insert(i)).unwrap();
    }
    assert_eq!(state.record_count(), 5);
}

#[test]
fn insert_with_wrong_sequential_id_is_rejected() {
    let mut state = KernelState::new();
    state.apply_event(&insert(0)).unwrap();
    // The pool allocates sequentially — claiming id 5 next must fail,
    // otherwise replicas replaying the same log could disagree on IDs.
    assert!(state.apply_event(&insert(5)).is_err());
    assert_eq!(state.record_count(), 1);
}

#[test]
fn insert_with_mismatched_dimension_is_rejected() {
    let mut state = KernelState::new();
    state.apply_event(&insert(0)).unwrap();
    let bad = KernelEvent::InsertRecord {
        id: RecordId(1),
        vector: FxpVector::new_zeros(DIM + 1),
        metadata: None,
        tag: 0,
    };
    assert!(state.apply_event(&bad).is_err());
}

#[test]
fn delete_record_reduces_count() {
    let mut state = KernelState::new();
    state.apply_event(&insert(0)).unwrap();
    state.apply_event(&insert(1)).unwrap();
    state
        .apply_event(&KernelEvent::DeleteRecord { id: RecordId(0) })
        .unwrap();
    assert_eq!(state.record_count(), 1);
}

#[test]
fn node_and_edge_lifecycle() {
    let mut state = KernelState::new();
    for i in 0..2 {
        state
            .apply_event(&KernelEvent::CreateNode {
                id: NodeId(i),
                kind: NodeKind::Concept,
                record: None,
            })
            .unwrap();
    }
    state
        .apply_event(&KernelEvent::CreateEdge {
            id: EdgeId(0),
            kind: EdgeKind::Relation,
            from: NodeId(0),
            to: NodeId(1),
        })
        .unwrap();
    assert_eq!(state.node_count(), 2);
    assert_eq!(state.edge_count(), 1);

    // Deleting a node cascades to its edges.
    state
        .apply_event(&KernelEvent::DeleteNode { id: NodeId(0) })
        .unwrap();
    assert_eq!(state.node_count(), 1);
    assert_eq!(state.edge_count(), 0);
}

#[test]
fn node_referencing_missing_record_is_rejected() {
    let mut state = KernelState::new();
    let evt = KernelEvent::CreateNode {
        id: NodeId(0),
        kind: NodeKind::Concept,
        record: Some(RecordId(42)),
    };
    assert!(state.apply_event(&evt).is_err());
    assert_eq!(state.node_count(), 0);
}

#[test]
fn failed_event_leaves_state_unchanged() {
    let mut state = KernelState::new();
    state.apply_event(&insert(0)).unwrap();
    let before = valori_kernel::snapshot::blake3::hash_state_blake3(&state);

    let _ = state.apply_event(&insert(7)); // wrong id — rejected
    let after = valori_kernel::snapshot::blake3::hash_state_blake3(&state);
    assert_eq!(before, after, "rejected events must not mutate state");
}

// ── Phase S2: AutoCreateNamespace / DropNamespace (apply_event_ns) ──────────
//
// Both variants delegate to KernelState's existing Command::CreateNamespace/
// DropNamespace logic — the kernel never sees the name, only the
// already-resolved namespace_id passed as apply_event_ns's second argument
// (the consensus layer resolves/allocates the id one layer up; see
// docs/phases/phase-S2-*.md).

#[test]
fn auto_create_namespace_registers_valid_id() {
    let mut state = KernelState::new();
    let evt = KernelEvent::AutoCreateNamespace { name: "tenant-acme".into() };
    assert!(state.apply_event_ns(&evt, 5).is_ok());
}

#[test]
fn auto_create_namespace_rejects_id_beyond_max() {
    let mut state = KernelState::new();
    let evt = KernelEvent::AutoCreateNamespace { name: "overflow".into() };
    assert!(state.apply_event_ns(&evt, valori_kernel::types::id::MAX_NAMESPACES as u16).is_err());
}

#[test]
fn drop_namespace_cascades_records_in_that_namespace() {
    let mut state = KernelState::new();
    state.apply_event_ns(&insert(0), 5).unwrap();
    state.apply_event_ns(&insert(1), 5).unwrap();
    state.apply_event_ns(&insert(2), 0).unwrap(); // different namespace — must survive
    assert_eq!(state.record_count(), 3);

    let drop_evt = KernelEvent::DropNamespace { name: "tenant-acme".into() };
    state.apply_event_ns(&drop_evt, 5).unwrap();

    assert_eq!(state.record_count(), 1, "only namespace 0's record should survive the cascade");
}

#[test]
fn drop_namespace_zero_is_rejected() {
    let mut state = KernelState::new();
    let evt = KernelEvent::DropNamespace { name: "default".into() };
    assert!(state.apply_event_ns(&evt, 0).is_err(), "the default namespace must never be dropped");
}
