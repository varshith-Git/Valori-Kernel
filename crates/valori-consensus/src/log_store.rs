// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Raft log storage — Phase 2.2.
//!
//! `ValoriLogStore` implements openraft's [`RaftLogStorage`]: the *internal*
//! Raft log of not-yet-(or recently-)committed entries, plus the persisted
//! vote. This log is truncatable (conflict resolution after leader change)
//! and purgeable (snapshot compaction) — properties the append-only audit
//! log must never have, which is exactly why they are different logs.
//!
//! **The one rule:** nothing here touches `events.log`. The audit log is
//! written by the state machine (Phase 2.3) at APPLY time, after quorum.
//!
//! ## Storage backend
//!
//! Phase 2.2 is in-memory (`BTreeMap` behind an async mutex) — correct
//! semantics first, durability later. Phase 2.10 swaps the maps for an
//! embedded `redb` database behind the same interface; the openraft
//! compliance suite (run in Phase 2.3) re-validates the swap.
//!
//! ## Concurrency model
//!
//! openraft hands `&mut self` to one writer task, and clones `LogReader`s
//! into replication tasks (one per follower). All state therefore lives in
//! one `Arc<Mutex<…>>` shared by the store and every reader clone.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::ops::RangeBounds;
use std::sync::Arc;

use openraft::storage::{LogFlushed, LogState, RaftLogStorage};
use openraft::{LogId, RaftLogReader, StorageError, Vote};
use tokio::sync::Mutex;

use crate::types::{Entry, NodeId, TypeConfig};

/// Everything the Raft log persists, in one lockable unit so vote and log
/// writes are serialized (an openraft correctness requirement).
#[derive(Debug, Default)]
struct LogStoreInner {
    /// index → entry. BTreeMap keeps range reads ordered and cheap.
    log: BTreeMap<u64, Entry>,
    /// Highest log id removed by `purge` (compaction floor).
    last_purged: Option<LogId<NodeId>>,
    /// The persisted vote — MUST survive restarts (Phase 2.10: disk).
    /// A lost vote can elect two leaders in one term.
    vote: Option<Vote<NodeId>>,
    /// Last known committed log id (optional persistence, we keep it).
    committed: Option<LogId<NodeId>>,
}

/// In-memory Raft log store. Cheap to clone — all clones share state.
#[derive(Debug, Default, Clone)]
pub struct ValoriLogStore {
    inner: Arc<Mutex<LogStoreInner>>,
}

impl ValoriLogStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of entries currently held (purged entries excluded).
    /// Test/metrics helper, not part of the openraft contract.
    pub async fn entry_count(&self) -> usize {
        self.inner.lock().await.log.len()
    }
}

impl RaftLogReader<TypeConfig> for ValoriLogStore {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry>, StorageError<NodeId>> {
        let inner = self.inner.lock().await;
        Ok(inner.log.range(range).map(|(_, e)| e.clone()).collect())
    }
}

impl RaftLogStorage<TypeConfig> for ValoriLogStore {
    type LogReader = Self;

    async fn get_log_state(&mut self) -> Result<LogState<TypeConfig>, StorageError<NodeId>> {
        let inner = self.inner.lock().await;
        let last = inner
            .log
            .values()
            .next_back()
            .map(|e| e.log_id)
            .or(inner.last_purged);
        Ok(LogState {
            last_purged_log_id: inner.last_purged,
            last_log_id: last,
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }

    async fn save_vote(&mut self, vote: &Vote<NodeId>) -> Result<(), StorageError<NodeId>> {
        self.inner.lock().await.vote = Some(*vote);
        Ok(())
    }

    async fn read_vote(&mut self) -> Result<Option<Vote<NodeId>>, StorageError<NodeId>> {
        Ok(self.inner.lock().await.vote)
    }

    async fn save_committed(
        &mut self,
        committed: Option<LogId<NodeId>>,
    ) -> Result<(), StorageError<NodeId>> {
        self.inner.lock().await.committed = committed;
        Ok(())
    }

    async fn read_committed(&mut self) -> Result<Option<LogId<NodeId>>, StorageError<NodeId>> {
        Ok(self.inner.lock().await.committed)
    }

    async fn append<I>(
        &mut self,
        entries: I,
        callback: LogFlushed<TypeConfig>,
    ) -> Result<(), StorageError<NodeId>>
    where
        I: IntoIterator<Item = Entry> + Send,
        I::IntoIter: Send,
    {
        {
            let mut inner = self.inner.lock().await;
            for entry in entries {
                inner.log.insert(entry.log_id.index, entry);
            }
        }
        // In-memory store: "persisted" the moment the map insert lands.
        // Phase 2.10 (redb) calls this only after the fsync of the batch.
        callback.log_io_completed(Ok(()));
        Ok(())
    }

    async fn truncate(&mut self, log_id: LogId<NodeId>) -> Result<(), StorageError<NodeId>> {
        // Conflict resolution: delete everything AT AND AFTER log_id.
        let mut inner = self.inner.lock().await;
        inner.log.split_off(&log_id.index);
        Ok(())
    }

    async fn purge(&mut self, log_id: LogId<NodeId>) -> Result<(), StorageError<NodeId>> {
        // Compaction: delete everything UP TO AND INCLUDING log_id.
        let mut inner = self.inner.lock().await;
        // last_purged must be monotonic — openraft may replay purges.
        if inner.last_purged.map_or(true, |p| p < log_id) {
            inner.last_purged = Some(log_id);
        }
        let keep = inner.log.split_off(&(log_id.index + 1));
        inner.log = keep;
        Ok(())
    }
}
