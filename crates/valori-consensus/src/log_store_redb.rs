// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Persistent Raft log on redb — Phase 2.10.
//!
//! Same contract as the in-memory [`crate::log_store::ValoriLogStore`]
//! (both pass the official openraft compliance suite), but the log, vote,
//! committed pointer, and purge floor survive process crashes — the
//! prerequisite for crash-*restart* fault tolerance.
//!
//! ## Layout
//!
//! One redb database file, two tables:
//! - `logs`: `u64 index → bincode(Entry)`
//! - `meta`: `&str key → bincode(value)` for `vote`, `committed`,
//!   `last_purged`
//!
//! ## Durability discipline
//!
//! Every openraft write method commits its redb transaction before
//! returning (redb commits are fsync-backed by default). `append` fires
//! the `LogFlushed` callback only **after** the commit — the exact
//! guarantee openraft's correctness argument needs. A vote that is
//! acknowledged but lost can elect two leaders in one term; a log entry
//! acknowledged but lost breaks the quorum invariant.

use std::fmt::Debug;
use std::ops::{Bound, RangeBounds};
use std::path::Path;
use std::sync::Arc;

use openraft::storage::{LogFlushed, LogState, RaftLogStorage};
use openraft::{AnyError, ErrorSubject, ErrorVerb, LogId, RaftLogReader, StorageError, StorageIOError, Vote};
use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition};

use crate::types::{Entry, NodeId, TypeConfig};

const LOGS: TableDefinition<u64, &[u8]> = TableDefinition::new("logs");
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");

const KEY_VOTE: &str = "vote";
const KEY_COMMITTED: &str = "committed";
const KEY_LAST_PURGED: &str = "last_purged";

fn io_err(e: impl std::error::Error + 'static) -> StorageError<NodeId> {
    StorageError::IO {
        source: StorageIOError::new(ErrorSubject::Store, ErrorVerb::Write, AnyError::new(&e)),
    }
}

fn encode<T: serde::Serialize>(v: &T) -> Result<Vec<u8>, StorageError<NodeId>> {
    bincode::serde::encode_to_vec(v, bincode::config::standard()).map_err(io_err)
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, StorageError<NodeId>> {
    bincode::serde::decode_from_slice(bytes, bincode::config::standard())
        .map(|(v, _)| v)
        .map_err(io_err)
}

/// State machine metadata lives in this table alongside the log metadata.
/// Key space is prefixed "sm_" to avoid collision with log store keys.
pub const SM_META: TableDefinition<'static, &'static str, &'static [u8]> =
    TableDefinition::new("sm_meta");

/// redb-backed Raft log store. Cheap to clone — clones share the database.
#[derive(Clone)]
pub struct RedbLogStore {
    db: Arc<Database>,
}

impl RedbLogStore {
    /// Open (or create) the Raft log database at `path`. Reopening after a
    /// crash recovers everything the last committed transaction held.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, redb::Error> {
        let db = Database::create(path)?;
        // Ensure tables exist so first reads don't special-case.
        let txn = db.begin_write()?;
        {
            txn.open_table(LOGS)?;
            txn.open_table(META)?;
            txn.open_table(SM_META)?;
        }
        txn.commit()?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Shared database handle — passed to `ValoriStateMachine::with_db` so
    /// both the log store and the state machine use the same redb file.
    pub fn db(&self) -> Arc<Database> {
        self.db.clone()
    }

    fn read_meta<T: serde::de::DeserializeOwned>(
        &self,
        key: &str,
    ) -> Result<Option<T>, StorageError<NodeId>> {
        let txn = self.db.begin_read().map_err(io_err)?;
        let table = txn.open_table(META).map_err(io_err)?;
        match table.get(key).map_err(io_err)? {
            Some(v) => Ok(Some(decode(v.value())?)),
            None => Ok(None),
        }
    }

    fn write_meta<T: serde::Serialize>(
        &self,
        key: &str,
        value: &T,
    ) -> Result<(), StorageError<NodeId>> {
        let bytes = encode(value)?;
        let txn = self.db.begin_write().map_err(io_err)?;
        {
            let mut table = txn.open_table(META).map_err(io_err)?;
            table.insert(key, bytes.as_slice()).map_err(io_err)?;
        }
        txn.commit().map_err(io_err)?;
        Ok(())
    }

    /// Test/metrics helper — number of entries currently stored.
    pub fn entry_count(&self) -> Result<u64, StorageError<NodeId>> {
        let txn = self.db.begin_read().map_err(io_err)?;
        let table = txn.open_table(LOGS).map_err(io_err)?;
        table.len().map_err(io_err)
    }
}

fn to_index_bounds<RB: RangeBounds<u64>>(range: &RB) -> (Bound<u64>, Bound<u64>) {
    (range.start_bound().cloned(), range.end_bound().cloned())
}

impl RaftLogReader<TypeConfig> for RedbLogStore {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry>, StorageError<NodeId>> {
        let txn = self.db.begin_read().map_err(io_err)?;
        let table = txn.open_table(LOGS).map_err(io_err)?;
        let mut out = Vec::new();
        for item in table.range(to_index_bounds(&range)).map_err(io_err)? {
            let (_, v) = item.map_err(io_err)?;
            out.push(decode(v.value())?);
        }
        Ok(out)
    }
}

impl RaftLogStorage<TypeConfig> for RedbLogStore {
    type LogReader = Self;

    async fn get_log_state(&mut self) -> Result<LogState<TypeConfig>, StorageError<NodeId>> {
        let last_purged: Option<LogId<NodeId>> = self.read_meta(KEY_LAST_PURGED)?;

        let txn = self.db.begin_read().map_err(io_err)?;
        let table = txn.open_table(LOGS).map_err(io_err)?;
        let last = match table.last().map_err(io_err)? {
            Some((_, v)) => {
                let entry: Entry = decode(v.value())?;
                Some(entry.log_id)
            }
            None => last_purged,
        };

        Ok(LogState {
            last_purged_log_id: last_purged,
            last_log_id: last,
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }

    async fn save_vote(&mut self, vote: &Vote<NodeId>) -> Result<(), StorageError<NodeId>> {
        // Committed (fsynced) before returning — the openraft requirement.
        self.write_meta(KEY_VOTE, vote)
    }

    async fn read_vote(&mut self) -> Result<Option<Vote<NodeId>>, StorageError<NodeId>> {
        self.read_meta(KEY_VOTE)
    }

    async fn save_committed(
        &mut self,
        committed: Option<LogId<NodeId>>,
    ) -> Result<(), StorageError<NodeId>> {
        self.write_meta(KEY_COMMITTED, &committed)
    }

    async fn read_committed(&mut self) -> Result<Option<LogId<NodeId>>, StorageError<NodeId>> {
        Ok(self.read_meta::<Option<LogId<NodeId>>>(KEY_COMMITTED)?.flatten())
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
        let txn = self.db.begin_write().map_err(io_err)?;
        {
            let mut table = txn.open_table(LOGS).map_err(io_err)?;
            for entry in entries {
                let bytes = encode(&entry)?;
                table.insert(entry.log_id.index, bytes.as_slice()).map_err(io_err)?;
            }
        }
        txn.commit().map_err(io_err)?;
        // Only after the durable commit: tell openraft the IO landed.
        callback.log_io_completed(Ok(()));
        Ok(())
    }

    async fn truncate(&mut self, log_id: LogId<NodeId>) -> Result<(), StorageError<NodeId>> {
        // Conflict resolution: delete at and after log_id.
        let txn = self.db.begin_write().map_err(io_err)?;
        {
            let mut table = txn.open_table(LOGS).map_err(io_err)?;
            table.retain(|k, _| k < log_id.index).map_err(io_err)?;
        }
        txn.commit().map_err(io_err)?;
        Ok(())
    }

    async fn purge(&mut self, log_id: LogId<NodeId>) -> Result<(), StorageError<NodeId>> {
        // Compaction: delete up to and including; floor is monotonic.
        let current: Option<LogId<NodeId>> = self.read_meta(KEY_LAST_PURGED)?;
        let floor = if current.map_or(true, |p| p < log_id) {
            log_id
        } else {
            current.unwrap()
        };

        let floor_bytes = encode(&floor)?;
        let txn = self.db.begin_write().map_err(io_err)?;
        {
            let mut meta = txn.open_table(META).map_err(io_err)?;
            meta.insert(KEY_LAST_PURGED, floor_bytes.as_slice()).map_err(io_err)?;
            let mut table = txn.open_table(LOGS).map_err(io_err)?;
            table.retain(|k, _| k > log_id.index).map_err(io_err)?;
        }
        txn.commit().map_err(io_err)?;
        Ok(())
    }
}
