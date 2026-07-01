// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 2.3 — ValoriStateMachine behaviour: apply pipeline, request-id
//! dedup, deterministic rejection, audit-sink ordering, snapshot round-trip,
//! and the cross-node hash-equality invariant.

use openraft::storage::RaftStateMachine;
use openraft::testing::log_id;
use openraft::{EntryPayload, RaftSnapshotBuilder};

use valori_consensus::types::{ClientRequest, Entry, NodeId, CURRENT_SCHEMA_VERSION};
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
        payload: EntryPayload::Normal(ClientRequest { event, request_id: rid, schema_version: 0, namespace_id: 0 }),
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
    let mut sm = ValoriStateMachine::new(Box::new(sink.clone()), 4);

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
    // Corrupt the very last byte: bincode encodes state_hash (32 bytes) at the
    // tail of the payload, so the last byte is always inside the hash field
    // regardless of kernel format version. This guarantees the hash check fires.
    *bytes.last_mut().unwrap() ^= 0xFF;

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

// ── Schema version gate (Phase 3.2) ──────────────────────────────────────────

fn versioned(term: u64, node: NodeId, index: u64, event: KernelEvent, version: u8) -> Entry {
    Entry {
        log_id: log_id(term, node, index),
        payload: EntryPayload::Normal(ClientRequest {
            event,
            request_id: None,
            schema_version: version,
        namespace_id: 0,
        }),
    }
}

#[tokio::test]
async fn apply_accepts_current_schema_version() {
    let mut sm = ValoriStateMachine::default();
    let result = sm
        .apply(vec![versioned(1, 1, 1, insert_event(0), CURRENT_SCHEMA_VERSION)])
        .await;
    assert!(result.is_ok(), "current schema version must be accepted");
    assert_eq!(sm.with_state(|s| s.record_count()).await, 1);
}

#[tokio::test]
async fn apply_rejects_unknown_schema_version() {
    let mut sm = ValoriStateMachine::default();
    let future_version = CURRENT_SCHEMA_VERSION.saturating_add(1);
    // H-4: a too-new schema version is rejected at the APPLICATION layer
    // (ClientResponse.rejected), not as a StorageError — a StorageError here
    // would permanently halt the node. apply() itself still returns Ok.
    let replies = sm
        .apply(vec![versioned(1, 1, 1, insert_event(0), future_version)])
        .await
        .unwrap();
    assert!(
        replies[0].rejected.is_some(),
        "a schema version newer than CURRENT_SCHEMA_VERSION must be refused"
    );
    // State must be untouched — the node did not apply a partially understood entry.
    assert_eq!(
        sm.with_state(|s| s.record_count()).await,
        0,
        "kernel state must be unchanged after a schema-version rejection"
    );
}

// ── Phase S2: Raft-replicated namespace registry ─────────────────────────────

fn create_ns_event(name: &str) -> KernelEvent {
    KernelEvent::AutoCreateNamespace { name: name.to_string() }
}

fn drop_ns_event(name: &str) -> KernelEvent {
    KernelEvent::DropNamespace { name: name.to_string() }
}

#[tokio::test]
async fn auto_create_namespace_assigns_sequential_ids() {
    let mut sm = ValoriStateMachine::default();

    let r1 = sm.apply(vec![normal(1, 1, 1, create_ns_event("docs"), None)]).await.unwrap();
    let r2 = sm.apply(vec![normal(1, 1, 2, create_ns_event("images"), None)]).await.unwrap();

    assert_eq!(r1[0].allocated_namespace_id, Some(1));
    assert_eq!(r2[0].allocated_namespace_id, Some(2));
    assert_eq!(sm.resolve_namespace(Some("docs")).await, Some(1));
    assert_eq!(sm.resolve_namespace(Some("images")).await, Some(2));
}

#[tokio::test]
async fn auto_create_namespace_is_idempotent_by_name() {
    let mut sm = ValoriStateMachine::default();

    // Two different request_ids (or none) — idempotency here comes from the
    // NAME, not the dedup table, unlike record inserts.
    let first = sm.apply(vec![normal(1, 1, 1, create_ns_event("docs"), None)]).await.unwrap();
    let second = sm.apply(vec![normal(1, 1, 2, create_ns_event("docs"), None)]).await.unwrap();

    assert_eq!(first[0].allocated_namespace_id, second[0].allocated_namespace_id);
    assert_eq!(first[0].allocated_namespace_id, Some(1));
}

#[tokio::test]
async fn default_namespace_resolves_without_being_created() {
    let sm = ValoriStateMachine::default();
    assert_eq!(sm.resolve_namespace(None).await, Some(0));
    assert_eq!(sm.resolve_namespace(Some("default")).await, Some(0));
    assert_eq!(sm.resolve_namespace(Some("unregistered")).await, None);
}

#[tokio::test]
async fn drop_namespace_removes_from_registry() {
    // The kernel-side cascade-delete (records/nodes/edges) is proven
    // independently in crates/valori-kernel/tests/state_machine.rs's
    // drop_namespace_cascades_records_in_that_namespace — this test is
    // scoped to the consensus-layer registry mutation itself.
    let mut sm = ValoriStateMachine::default();

    sm.apply(vec![normal(1, 1, 1, create_ns_event("docs"), None)]).await.unwrap();
    assert_eq!(sm.resolve_namespace(Some("docs")).await, Some(1));

    sm.apply(vec![normal(1, 1, 2, drop_ns_event("docs"), None)]).await.unwrap();
    assert_eq!(sm.resolve_namespace(Some("docs")).await, None, "dropped namespace must no longer resolve");
}

#[tokio::test]
async fn drop_unknown_namespace_is_rejected_registry_unchanged() {
    let mut sm = ValoriStateMachine::default();

    let replies = sm.apply(vec![normal(1, 1, 1, drop_ns_event("never-created"), None)]).await.unwrap();
    assert!(replies[0].rejected.is_some(), "dropping an unknown namespace must be rejected");
    assert_eq!(sm.resolve_namespace(Some("never-created")).await, None);
}

#[tokio::test]
async fn snapshot_roundtrip_preserves_namespace_registry() {
    let mut leader = ValoriStateMachine::default();
    leader.apply(vec![
        normal(1, 1, 1, create_ns_event("docs"), None),
        normal(1, 1, 2, create_ns_event("images"), None),
    ]).await.unwrap();

    let snapshot = leader.get_snapshot_builder().await.build_snapshot().await.unwrap();

    let mut follower = ValoriStateMachine::default();
    follower.install_snapshot(&snapshot.meta, snapshot.snapshot).await.unwrap();

    assert_eq!(follower.resolve_namespace(Some("docs")).await, Some(1));
    assert_eq!(follower.resolve_namespace(Some("images")).await, Some(2));

    // A namespace created AFTER the snapshot must continue the SAME sequence
    // — proves next_id survived the round-trip too, not just the map.
    follower.apply(vec![normal(2, 2, 3, create_ns_event("videos"), None)]).await.unwrap();
    assert_eq!(follower.resolve_namespace(Some("videos")).await, Some(3));
}

#[tokio::test]
async fn two_nodes_applying_the_same_entries_converge_on_namespace_ids() {
    let entries = || {
        vec![
            normal(1, 1, 1, create_ns_event("docs"), None),
            normal(1, 1, 2, create_ns_event("images"), None),
            normal(1, 1, 3, create_ns_event("docs"), None), // idempotent repeat
        ]
    };

    let mut a = ValoriStateMachine::default();
    let mut b = ValoriStateMachine::default();
    a.apply(entries()).await.unwrap();
    b.apply(entries()).await.unwrap();

    assert_eq!(a.resolve_namespace(Some("docs")).await, b.resolve_namespace(Some("docs")).await);
    assert_eq!(a.resolve_namespace(Some("images")).await, b.resolve_namespace(Some("images")).await);
    assert_eq!(
        a.state_hash().await,
        b.state_hash().await,
        "namespace events must not desync the BLAKE3 state hash between replicas"
    );
}
