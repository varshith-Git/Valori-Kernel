// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 2.2 — ValoriLogStore behaviour: append/read, conflict truncation,
//! compaction purge, vote persistence, and reader-clone consistency.
//!
//! The full openraft compliance suite (`openraft::testing::Suite`) runs in
//! Phase 2.3 once the state machine exists — the suite requires both halves.

use openraft::storage::{RaftLogStorage, RaftLogStorageExt};
use openraft::testing::log_id;
use openraft::{RaftLogReader, Vote};

use valori_consensus::types::{ClientRequest, Entry, NodeId};
use valori_consensus::ValoriLogStore;
use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;

fn entry(term: u64, node: NodeId, index: u64) -> Entry {
    Entry {
        log_id: log_id(term, node, index),
        payload: openraft::EntryPayload::Normal(ClientRequest {
            event: KernelEvent::DeleteRecord { id: RecordId(index as u32) },
            request_id: None,
        }),
    }
}

/// `blocking_append` (from RaftLogStorageExt) drives the LogFlushed callback
/// for us — the ergonomic path for tests.
async fn append(store: &mut ValoriLogStore, entries: Vec<Entry>) {
    store.blocking_append(entries).await.unwrap();
}

// ── Empty state ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn fresh_store_is_empty() {
    let mut store = ValoriLogStore::new();
    let state = store.get_log_state().await.unwrap();
    assert_eq!(state.last_log_id, None);
    assert_eq!(state.last_purged_log_id, None);
    assert_eq!(store.read_vote().await.unwrap(), None);
    assert_eq!(store.entry_count().await, 0);
}

// ── Append + read ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn append_then_read_roundtrips() {
    let mut store = ValoriLogStore::new();
    append(&mut store, vec![entry(1, 1, 1), entry(1, 1, 2), entry(1, 1, 3)]).await;

    let got = store.try_get_log_entries(1..=3).await.unwrap();
    assert_eq!(got.len(), 3);
    assert_eq!(got[0].log_id.index, 1);
    assert_eq!(got[2].log_id.index, 3);

    let state = store.get_log_state().await.unwrap();
    assert_eq!(state.last_log_id, Some(log_id(1, 1, 3)));
}

#[tokio::test]
async fn range_reads_are_half_open() {
    let mut store = ValoriLogStore::new();
    append(&mut store, (1..=5).map(|i| entry(1, 1, i)).collect()).await;

    let got = store.try_get_log_entries(2..4).await.unwrap();
    assert_eq!(
        got.iter().map(|e| e.log_id.index).collect::<Vec<_>>(),
        vec![2, 3],
        "[start, stop) — stop is exclusive"
    );
}

#[tokio::test]
async fn missing_entries_are_allowed_not_errors() {
    let mut store = ValoriLogStore::new();
    append(&mut store, vec![entry(1, 1, 1)]).await;
    let got = store.try_get_log_entries(5..10).await.unwrap();
    assert!(got.is_empty(), "absent range returns empty, not error");
}

// ── Truncate (conflict resolution) ────────────────────────────────────────────

#[tokio::test]
async fn truncate_removes_at_and_after() {
    let mut store = ValoriLogStore::new();
    append(&mut store, (1..=5).map(|i| entry(1, 1, i)).collect()).await;

    // New leader's log conflicts from index 3 — drop 3, 4, 5.
    store.truncate(log_id(1, 1, 3)).await.unwrap();

    let state = store.get_log_state().await.unwrap();
    assert_eq!(state.last_log_id, Some(log_id(1, 1, 2)));
    assert_eq!(store.entry_count().await, 2);

    // The truncated range can be rewritten by the new leader.
    append(&mut store, vec![entry(2, 2, 3)]).await;
    let got = store.try_get_log_entries(3..4).await.unwrap();
    assert_eq!(got[0].log_id, log_id(2, 2, 3), "rewritten under the new term");
}

// ── Purge (compaction) ────────────────────────────────────────────────────────

#[tokio::test]
async fn purge_removes_up_to_inclusive_and_records_floor() {
    let mut store = ValoriLogStore::new();
    append(&mut store, (1..=5).map(|i| entry(1, 1, i)).collect()).await;

    store.purge(log_id(1, 1, 3)).await.unwrap();

    let state = store.get_log_state().await.unwrap();
    assert_eq!(state.last_purged_log_id, Some(log_id(1, 1, 3)));
    assert_eq!(state.last_log_id, Some(log_id(1, 1, 5)), "entries after the floor survive");
    assert_eq!(store.entry_count().await, 2);
}

#[tokio::test]
async fn purge_of_entire_log_reports_floor_as_last() {
    let mut store = ValoriLogStore::new();
    append(&mut store, (1..=3).map(|i| entry(1, 1, i)).collect()).await;

    store.purge(log_id(1, 1, 3)).await.unwrap();

    let state = store.get_log_state().await.unwrap();
    assert_eq!(state.last_log_id, Some(log_id(1, 1, 3)),
        "empty log: last_log_id falls back to the purge floor");
    assert_eq!(store.entry_count().await, 0);
}

#[tokio::test]
async fn purge_floor_is_monotonic() {
    let mut store = ValoriLogStore::new();
    append(&mut store, (1..=5).map(|i| entry(1, 1, i)).collect()).await;

    store.purge(log_id(1, 1, 4)).await.unwrap();
    // A replayed, older purge must not move the floor backwards.
    store.purge(log_id(1, 1, 2)).await.unwrap();

    let state = store.get_log_state().await.unwrap();
    assert_eq!(state.last_purged_log_id, Some(log_id(1, 1, 4)));
}

// ── Vote persistence ──────────────────────────────────────────────────────────

#[tokio::test]
async fn vote_roundtrips_and_overwrites() {
    let mut store = ValoriLogStore::new();

    let v1 = Vote::new(1, 1);
    store.save_vote(&v1).await.unwrap();
    assert_eq!(store.read_vote().await.unwrap(), Some(v1));

    let v2 = Vote::new_committed(2, 3);
    store.save_vote(&v2).await.unwrap();
    assert_eq!(store.read_vote().await.unwrap(), Some(v2), "newer vote overwrites");
}

#[tokio::test]
async fn committed_log_id_roundtrips() {
    let mut store = ValoriLogStore::new();
    assert_eq!(store.read_committed().await.unwrap(), None);

    store.save_committed(Some(log_id(1, 1, 7))).await.unwrap();
    assert_eq!(store.read_committed().await.unwrap(), Some(log_id(1, 1, 7)));
}

// ── Reader clones share state ─────────────────────────────────────────────────

#[tokio::test]
async fn reader_clone_sees_writes_made_after_cloning() {
    let mut store = ValoriLogStore::new();
    // Replication tasks clone the reader once and hold it long-term —
    // it must observe entries appended later.
    let mut reader = store.get_log_reader().await;

    append(&mut store, vec![entry(1, 1, 1), entry(1, 1, 2)]).await;

    let got = reader.try_get_log_entries(1..=2).await.unwrap();
    assert_eq!(got.len(), 2, "reader shares state with the store, not a snapshot");
}
