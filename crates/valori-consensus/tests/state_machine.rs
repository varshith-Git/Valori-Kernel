// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 2.3 — ValoriStateMachine behaviour: apply pipeline, request-id
//! dedup, deterministic rejection, audit-sink ordering, snapshot round-trip,
//! and the cross-node hash-equality invariant.

use openraft::storage::RaftStateMachine;
use openraft::testing::log_id;
use openraft::{EntryPayload, RaftSnapshotBuilder};

use valori_consensus::types::{ClientRequest, Entry, NodeId};
use valori_consensus::{MemoryAuditSink, ValoriStateMachine};
use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::vector::FxpVector;

fn insert_event(id: u32) -> KernelEvent {
    KernelEvent::InsertRecord {
        id: RecordId(id),
        vector: FxpVector::new_zeros(4),
        metadata: Some(vec![id as u8]),
        tag: id as u64,
    }
}

fn normal(term: u64, node: NodeId, index: u64, event: KernelEvent, rid: Option<[u8; 16]>) -> Entry {
    Entry {
        log_id: log_id(term, node, index),
        payload: EntryPayload::Normal(ClientRequest { event, request_id: rid }),
    }
}

fn rid(n: u8) -> [u8; 16] {
    [n; 16]
}

// ── Apply pipeline ────────────────────────────────────────────────────────────

#[tokio::test]
async fn apply_advances_state_and_returns_hash() {
    let mut sm = ValoriStateMachine::default();

    let replies = sm
        .apply(vec![normal(1, 1, 1, insert_event(0), None)])
        .await
        .unwrap();

    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].log_index, 1);
    assert!(!replies[0].deduplicated);
    assert_eq!(replies[0].state_hash, sm.state_hash().await);
    assert_eq!(sm.with_state(|s| s.record_count()).await, 1);

    let (last, _) = sm.applied_state().await.unwrap();
    assert_eq!(last, Some(log_id(1, 1, 1)));
}

#[tokio::test]
async fn blank_entries_advance_last_applied_without_touching_state() {
    let mut sm = ValoriStateMachine::default();
    let before = sm.state_hash().await;

    sm.apply(vec![Entry {
        log_id: log_id(1, 1, 1),
        payload: EntryPayload::Blank,
    }])
    .await
    .unwrap();

    assert_eq!(sm.state_hash().await, before, "blank entry must not change state");
    let (last, _) = sm.applied_state().await.unwrap();
    assert_eq!(last, Some(log_id(1, 1, 1)));
}

// ── Dedup ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn duplicate_request_id_applies_once() {
    let mut sm = ValoriStateMachine::default();

    let first = sm
        .apply(vec![normal(1, 1, 1, insert_event(0), Some(rid(7)))])
        .await
        .unwrap();
    assert!(!first[0].deduplicated);

    // Leader-failover retry: same request_id, later log index.
    let second = sm
        .apply(vec![normal(2, 2, 2, insert_event(1), Some(rid(7)))])
        .await
        .unwrap();
    assert!(second[0].deduplicated, "same request_id must be recognised");
    assert_eq!(
        sm.with_state(|s| s.record_count()).await,
        1,
        "the duplicate must not double-apply"
    );
    assert_eq!(
        second[0].state_hash, first[0].state_hash,
        "dedup response carries the unchanged state hash"
    );
}

#[tokio::test]
async fn distinct_request_ids_both_apply() {
    let mut sm = ValoriStateMachine::default();
    sm.apply(vec![
        normal(1, 1, 1, insert_event(0), Some(rid(1))),
        normal(1, 1, 2, insert_event(1), Some(rid(2))),
    ])
    .await
    .unwrap();
    assert_eq!(sm.with_state(|s| s.record_count()).await, 2);
}

#[tokio::test]
async fn rejected_event_does_not_poison_dedup() {
    let mut sm = ValoriStateMachine::default();

    // id=5 violates the sequential-id rule — the kernel rejects it.
    sm.apply(vec![normal(1, 1, 1, insert_event(5), Some(rid(9)))])
        .await
        .unwrap();
    assert_eq!(sm.with_state(|s| s.record_count()).await, 0);

    // The same request_id retried with a correct event must NOT be treated
    // as a duplicate — only successful applies enter the dedup table.
    let replies = sm
        .apply(vec![normal(1, 1, 2, insert_event(0), Some(rid(9)))])
        .await
        .unwrap();
    assert!(!replies[0].deduplicated);
    assert_eq!(sm.with_state(|s| s.record_count()).await, 1);
}

// ── Deterministic rejection ───────────────────────────────────────────────────

#[tokio::test]
async fn rejected_event_leaves_state_untouched_but_consumes_the_entry() {
    let mut sm = ValoriStateMachine::default();
    let before = sm.state_hash().await;

    let replies = sm
        .apply(vec![normal(1, 1, 1, insert_event(42), None)])
        .await
        .unwrap();

    assert_eq!(replies[0].state_hash, before, "rejection leaves state untouched");
    let (last, _) = sm.applied_state().await.unwrap();
    assert_eq!(last, Some(log_id(1, 1, 1)), "the entry is still consumed");
}

// ── Audit sink ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn audit_sink_sees_successful_applies_in_order_and_nothing_else() {
    let sink = MemoryAuditSink::new();
    let mut sm = ValoriStateMachine::new(Box::new(sink.clone()));

    sm.apply(vec![
        normal(1, 1, 1, insert_event(0), Some(rid(1))),       // applies
        normal(1, 1, 2, insert_event(99), None),              // rejected (bad id)
        normal(1, 1, 3, insert_event(1), Some(rid(1))),       // deduplicated
        normal(1, 1, 4, KernelEvent::DeleteRecord { id: RecordId(0) }, None), // applies
    ])
    .await
    .unwrap();

    let recorded = sink.recorded();
    assert_eq!(
        recorded.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
        vec!["InsertRecord", "DeleteRecord"],
        "audit sees successful applies only, in apply order"
    );
    assert_eq!(recorded[0].1, Some(rid(1)), "request_id travels to the audit record");
}

// ── Snapshot round-trip ───────────────────────────────────────────────────────

#[tokio::test]
async fn snapshot_roundtrip_preserves_state_hash_and_dedup() {
    let mut leader = ValoriStateMachine::default();
    leader
        .apply(vec![
            normal(1, 1, 1, insert_event(0), Some(rid(1))),
            normal(1, 1, 2, insert_event(1), Some(rid(2))),
            normal(1, 1, 3, insert_event(2), None),
        ])
        .await
        .unwrap();
    let leader_hash = leader.state_hash().await;

    let snapshot = leader.get_snapshot_builder().await.build_snapshot().await.unwrap();

    // A brand-new follower installs the leader's snapshot.
    let mut follower = ValoriStateMachine::default();
    follower
        .install_snapshot(&snapshot.meta, snapshot.snapshot)
        .await
        .unwrap();

    assert_eq!(
        follower.state_hash().await,
        leader_hash,
        "cross-node hash equality after snapshot install"
    );
    assert_eq!(follower.with_state(|s| s.record_count()).await, 3);

    let (last, _) = follower.applied_state().await.unwrap();
    assert_eq!(last, Some(log_id(1, 1, 3)), "last_applied restored from meta");

    // The dedup table travelled with the snapshot: a replay of rid(1) on
    // the restored follower is recognised as a duplicate.
    let replies = follower
        .apply(vec![normal(2, 2, 4, insert_event(3), Some(rid(1)))])
        .await
        .unwrap();
    assert!(replies[0].deduplicated, "dedup table survives snapshot transfer");
}

#[tokio::test]
async fn corrupted_snapshot_payload_is_refused_and_state_kept() {
    let mut sm = ValoriStateMachine::default();
    sm.apply(vec![normal(1, 1, 1, insert_event(0), None)]).await.unwrap();
    let before = sm.state_hash().await;

    let snapshot = sm.get_snapshot_builder().await.build_snapshot().await.unwrap();
    let mut bytes = snapshot.snapshot.into_inner();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xFF;

    let result = sm
        .install_snapshot(&snapshot.meta, Box::new(std::io::Cursor::new(bytes)))
        .await;

    assert!(result.is_err(), "tampered snapshot must be refused");
    assert_eq!(sm.state_hash().await, before, "old state kept after refusal");
}

#[tokio::test]
async fn get_current_snapshot_returns_the_last_built() {
    let mut sm = ValoriStateMachine::default();
    assert!(sm.get_current_snapshot().await.unwrap().is_none());

    sm.apply(vec![normal(1, 1, 1, insert_event(0), None)]).await.unwrap();
    let built = sm.get_snapshot_builder().await.build_snapshot().await.unwrap();

    let current = sm.get_current_snapshot().await.unwrap().expect("snapshot stored");
    assert_eq!(current.meta.snapshot_id, built.meta.snapshot_id);
    assert_eq!(current.meta.last_log_id, Some(log_id(1, 1, 1)));
}

// ── Determinism: two nodes, same entries, same hash ───────────────────────────

#[tokio::test]
async fn two_nodes_applying_the_same_entries_converge_to_the_same_hash() {
    let entries = || {
        vec![
            normal(1, 1, 1, insert_event(0), Some(rid(1))),
            normal(1, 1, 2, insert_event(1), None),
            normal(1, 1, 3, KernelEvent::SoftDeleteRecord { id: RecordId(0) }, None),
            normal(1, 1, 4, insert_event(1), Some(rid(1))), // dup — skipped identically
        ]
    };

    let mut a = ValoriStateMachine::default();
    let mut b = ValoriStateMachine::default();
    a.apply(entries()).await.unwrap();
    b.apply(entries()).await.unwrap();

    assert_eq!(
        a.state_hash().await,
        b.state_hash().await,
        "the SMR invariant: same committed entries → same state hash"
    );
}
