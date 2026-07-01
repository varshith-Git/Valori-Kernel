// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 2.10 — RedbLogStore: the same behaviour the in-memory store has
//! (mirrored core tests), PLUS what it exists for: everything survives a
//! process restart. Reopen-and-verify is this file's reason to be.

use openraft::storage::{RaftLogStorage, RaftLogStorageExt};
use openraft::testing::log_id;
use openraft::{RaftLogReader, Vote};

use valori_consensus::types::{ClientRequest, Entry, NodeId};
use valori_consensus::RedbLogStore;
use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;

fn entry(term: u64, node: NodeId, index: u64) -> Entry {
    Entry {
        log_id: log_id(term, node, index),
        payload: openraft::EntryPayload::Normal(ClientRequest {
            event: KernelEvent::DeleteRecord { id: RecordId(index as u32) },
            request_id: Some([index as u8; 16]),
            schema_version: 0,
        namespace_id: 0,
        }),
    }
}

async fn append(store: &mut RedbLogStore, entries: Vec<Entry>) {
    store.blocking_append(entries).await.unwrap();
}

// ── Behaviour parity with the in-memory store ─────────────────────────────────

#[tokio::test]
async fn append_read_truncate_purge_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = RedbLogStore::open(dir.path().join("raft.redb")).unwrap();

    append(&mut store, (1..=5).map(|i| entry(1, 1, i)).collect()).await;

    let got = store.try_get_log_entries(2..4).await.unwrap();
    assert_eq!(
        got.iter().map(|e| e.log_id.index).collect::<Vec<_>>(),
        vec![2, 3],
        "[start, stop) half-open range"
    );

    // Truncate (conflict): drop 4, 5; rewrite 4 under a new term.
    store.truncate(log_id(1, 1, 4)).await.unwrap();
    assert_eq!(store.entry_count().unwrap(), 3);
    append(&mut store, vec![entry(2, 2, 4)]).await;
    let got = store.try_get_log_entries(4..5).await.unwrap();
    assert_eq!(got[0].log_id, log_id(2, 2, 4));

    // Purge (compaction): floor recorded, monotonic.
    store.purge(log_id(1, 1, 2)).await.unwrap();
    let state = store.get_log_state().await.unwrap();
    assert_eq!(state.last_purged_log_id, Some(log_id(1, 1, 2)));
    assert_eq!(state.last_log_id, Some(log_id(2, 2, 4)));

    store.purge(log_id(1, 1, 1)).await.unwrap(); // replayed older purge
    let state = store.get_log_state().await.unwrap();
    assert_eq!(state.last_purged_log_id, Some(log_id(1, 1, 2)), "floor is monotonic");
}

#[tokio::test]
async fn purge_of_entire_log_reports_floor_as_last() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = RedbLogStore::open(dir.path().join("raft.redb")).unwrap();
    append(&mut store, (1..=3).map(|i| entry(1, 1, i)).collect()).await;
    store.purge(log_id(1, 1, 3)).await.unwrap();

    let state = store.get_log_state().await.unwrap();
    assert_eq!(state.last_log_id, Some(log_id(1, 1, 3)));
    assert_eq!(store.entry_count().unwrap(), 0);
}

// ── The point of this store: restart survival ─────────────────────────────────

#[tokio::test]
async fn everything_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("raft.redb");

    // Session 1: write log, vote, committed pointer, purge floor — then DROP
    // the store (process death; redb transactions were committed).
    {
        let mut store = RedbLogStore::open(&path).unwrap();
        append(&mut store, (1..=10).map(|i| entry(1, 1, i)).collect()).await;
        store.save_vote(&Vote::new_committed(3, 1)).await.unwrap();
        store.save_committed(Some(log_id(1, 1, 9))).await.unwrap();
        store.purge(log_id(1, 1, 2)).await.unwrap();
    }

    // Session 2: a fresh open of the same file sees the exact same world.
    let mut store = RedbLogStore::open(&path).unwrap();

    assert_eq!(
        store.read_vote().await.unwrap(),
        Some(Vote::new_committed(3, 1)),
        "the vote MUST survive a crash — losing it can elect two leaders in one term"
    );
    assert_eq!(store.read_committed().await.unwrap(), Some(log_id(1, 1, 9)));

    let state = store.get_log_state().await.unwrap();
    assert_eq!(state.last_purged_log_id, Some(log_id(1, 1, 2)));
    assert_eq!(state.last_log_id, Some(log_id(1, 1, 10)));
    assert_eq!(store.entry_count().unwrap(), 8, "entries 3..=10 survive");

    // Entries decode fully — payloads included, not just ids.
    let got = store.try_get_log_entries(3..=3).await.unwrap();
    match &got[0].payload {
        openraft::EntryPayload::Normal(req) => {
            assert_eq!(req.request_id, Some([3u8; 16]), "payload bytes intact after reopen");
        }
        other => panic!("expected Normal payload, got {other:?}"),
    }

    // And the store keeps working after reopen.
    append(&mut store, vec![entry(2, 1, 11)]).await;
    assert_eq!(store.entry_count().unwrap(), 9);
}

#[tokio::test]
async fn fresh_database_is_empty_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = RedbLogStore::open(dir.path().join("raft.redb")).unwrap();
    let state = store.get_log_state().await.unwrap();
    assert_eq!(state.last_log_id, None);
    assert_eq!(state.last_purged_log_id, None);
    assert_eq!(store.read_vote().await.unwrap(), None);
    assert_eq!(store.read_committed().await.unwrap(), None);
}

// ── Official compliance suite over the persistent store ──────────────────────

mod compliance {
    use openraft::testing::{StoreBuilder, Suite};
    use openraft::StorageError;
    use valori_consensus::types::{NodeId, TypeConfig};
    use valori_consensus::{RedbLogStore, ValoriStateMachine};

    struct Builder;

    impl StoreBuilder<TypeConfig, RedbLogStore, ValoriStateMachine, tempfile::TempDir> for Builder {
        async fn build(
            &self,
        ) -> Result<(tempfile::TempDir, RedbLogStore, ValoriStateMachine), StorageError<NodeId>>
        {
            let dir = tempfile::tempdir().unwrap();
            let store = RedbLogStore::open(dir.path().join("raft.redb")).unwrap();
            Ok((dir, store, ValoriStateMachine::default()))
        }
    }

    #[test]
    fn openraft_compliance_suite_over_redb() {
        Suite::test_all(Builder).expect("redb store must pass the same suite the memory store passes");
    }
}
