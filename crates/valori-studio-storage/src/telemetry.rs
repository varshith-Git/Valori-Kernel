// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! A durable, bounded local telemetry queue — **storage layer only**.
//!
//! This does not send anything anywhere. `desktop/src-tauri/src/telemetry.rs`
//! keeps its own producer (`enqueue_telemetry_event`) and sender
//! (`spawn_sender`/`drain_queue`), backed by a flat `events.jsonl` file — S1
//! does not change that. This store exists so a future phase can point the
//! same producer/sender at `enqueue`/`peek_batch`/`mark_delivered` instead
//! of hand-rolled file I/O, gaining per-event atomicity (today's
//! `events.jsonl` does a whole-file read-modify-rewrite under one mutex on
//! every enqueue and every drain tick — see
//! `docs/architecture/studio-storage-audit.md` §6) without changing what
//! gets sent or when.
//!
//! # Bounded, always
//!
//! [`MAX_QUEUE_LEN`] mirrors `telemetry.rs`'s existing `MAX_QUEUE_LINES`
//! (500) — a queue that can grow forever is exactly the "Studio redb
//! becomes a log database" failure mode the audit warns against.
//! [`TelemetryQueue::enqueue`] evicts the oldest event(s) *in the same
//! transaction* as the insert whenever the count would exceed the cap, so
//! the table is never observed over its limit by any concurrent reader.
//!
//! # Delete on success, don't accumulate `delivered = true` rows
//!
//! [`TelemetryQueue::mark_delivered`] removes the row outright. There is no
//! "delivered" flag and no history kept here — an unbounded
//! `delivered = true` history is the audit's explicit warning (§6/§11:
//! "prefer deleting successfully acknowledged events rather than keeping
//! an unbounded uploaded=true history").

use redb::{Database, ReadableTable, ReadableTableMetadata};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use valori_domain::SessionId;

use crate::error::StudioStorageResult;
use crate::schema::TELEMETRY_QUEUE;

/// Matches `desktop/src-tauri/src/telemetry.rs`'s `MAX_QUEUE_LINES`. Kept
/// as a distinct constant (not shared across the crate boundary) because
/// the two queues are independent implementations of the same policy, not
/// one queue two callers agree on — see module docs.
pub const MAX_QUEUE_LEN: usize = 500;

/// Which consent field gates an event — see `crate::preferences::TelemetryConsent`.
/// This is deliberately **not** a third consent field: `TelemetryConsent`
/// still has exactly the two booleans it always has
/// (`analytics`/`crash`) — nothing in this crate or its callers found
/// evidence of a third category (e.g. "performance") ever having existed,
/// so none was added. `TelemetryCategory` is the *event's* tag, answering
/// "which of the two existing consent fields controls this one row" —
/// necessary because a single flat queue with no way to tell an
/// analytics-consented row from a crash-consented row makes it impossible
/// to revoke one without also silently discarding (or leaving deliverable)
/// the other. See module docs' "Consent boundary" section.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryCategory {
    Analytics,
    Crash,
}

impl Default for TelemetryCategory {
    /// Rows written before this field existed (any `studio.redb` from
    /// before this phase) deserialize via `#[serde(default)]` on
    /// [`StudioTelemetryEvent::category`] below. Every event enqueued
    /// prior to this phase was, in practice, gated by the enqueue-time
    /// `analytics_consent()` check regardless of its event name — so
    /// `Analytics` is not just the safer default (discarded on the next
    /// analytics revocation, rather than left permanently undiscardable),
    /// it is also the factually accurate one for what actually gated
    /// those rows' creation.
    fn default() -> Self {
        TelemetryCategory::Analytics
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StudioTelemetryEvent {
    pub event_id: String,
    pub created_at: i64,
    pub event_name: String,
    #[serde(default)]
    pub session_id: Option<SessionId>,
    #[serde(default)]
    pub payload: serde_json::Value,
    #[serde(default)]
    pub attempt_count: u32,
    #[serde(default)]
    pub last_attempt_at: Option<i64>,
    /// Which consent field gates this event's delivery. See
    /// [`TelemetryCategory`]'s doc comment for the backward-compatibility
    /// story on rows written before this field existed.
    #[serde(default)]
    pub category: TelemetryCategory,
}

impl StudioTelemetryEvent {
    /// Builds a new event with a fresh `event_id`, zero attempts. The
    /// caller supplies `created_at` (Studio's producer already has a wall
    /// clock; this crate does not read the clock itself, matching
    /// `valori-metadata`'s existing convention of taking timestamps as
    /// arguments rather than calling `SystemTime::now()` internally).
    pub fn new(
        event_name: impl Into<String>,
        session_id: Option<SessionId>,
        payload: serde_json::Value,
        created_at: i64,
        category: TelemetryCategory,
    ) -> Self {
        Self {
            event_id: Uuid::new_v4().to_string(),
            created_at,
            event_name: event_name.into(),
            session_id,
            payload,
            attempt_count: 0,
            last_attempt_at: None,
            category,
        }
    }
}

pub struct TelemetryQueue<'a> {
    db: &'a Database,
}

impl<'a> TelemetryQueue<'a> {
    pub(crate) fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// Inserts `event`, then evicts the oldest (by `created_at`, ties
    /// broken by `event_id`) events until the table is at or under
    /// [`MAX_QUEUE_LEN`] — all within one write transaction, so the cap is
    /// never violated even momentarily from another transaction's point of
    /// view.
    pub fn enqueue(&self, event: &StudioTelemetryEvent) -> StudioStorageResult<()> {
        let bytes = serde_json::to_vec(event)?;
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(TELEMETRY_QUEUE)?;
            t.insert(event.event_id.as_str(), bytes.as_slice())?;

            let len = t.len()? as usize;
            if len > MAX_QUEUE_LEN {
                let mut all: Vec<StudioTelemetryEvent> = Vec::with_capacity(len);
                for entry in t.iter()? {
                    let (_, v) = entry?;
                    all.push(serde_json::from_slice(v.value())?);
                }
                all.sort_by(|a, b| {
                    a.created_at
                        .cmp(&b.created_at)
                        .then(a.event_id.cmp(&b.event_id))
                });
                let drop_count = len - MAX_QUEUE_LEN;
                for stale in all.into_iter().take(drop_count) {
                    t.remove(stale.event_id.as_str())?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Up to `limit` events, oldest first (`created_at` then `event_id`) —
    /// the order a sender should attempt delivery in.
    pub fn peek_batch(&self, limit: usize) -> StudioStorageResult<Vec<StudioTelemetryEvent>> {
        let mut all = crate::schema::list_json::<StudioTelemetryEvent>(self.db, TELEMETRY_QUEUE)?;
        all.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then(a.event_id.cmp(&b.event_id))
        });
        all.truncate(limit);
        Ok(all)
    }

    /// Removes the event outright — call after the receiving server
    /// acknowledges it. Returns `true` if it was present.
    pub fn mark_delivered(&self, event_id: &str) -> StudioStorageResult<bool> {
        crate::schema::delete_key(self.db, TELEMETRY_QUEUE, event_id)
    }

    /// Bumps `attempt_count` and `last_attempt_at` after a failed delivery
    /// attempt. `NotFound` if the event isn't queued (already delivered or
    /// pruned by a racing call — callers should treat that as "nothing to
    /// retry," not an error worth surfacing to the user).
    pub fn increment_retry(
        &self,
        event_id: &str,
        attempted_at: i64,
    ) -> StudioStorageResult<StudioTelemetryEvent> {
        let tx = self.db.begin_write()?;
        let updated = {
            let mut t = tx.open_table(TELEMETRY_QUEUE)?;
            let mut event: StudioTelemetryEvent = match t.get(event_id)? {
                Some(v) => serde_json::from_slice(v.value())?,
                None => {
                    return Err(crate::error::StudioStorageError::NotFound(format!(
                        "telemetry event {event_id}"
                    )))
                }
            };
            event.attempt_count += 1;
            event.last_attempt_at = Some(attempted_at);
            let bytes = serde_json::to_vec(&event)?;
            t.insert(event_id, bytes.as_slice())?;
            event
        };
        tx.commit()?;
        Ok(updated)
    }

    pub fn count(&self) -> StudioStorageResult<usize> {
        let tx = self.db.begin_read()?;
        match tx.open_table(TELEMETRY_QUEUE) {
            Ok(t) => Ok(t.len()? as usize),
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(0),
            Err(e) => Err(e.into()),
        }
    }

    /// Deletes every queued event whose [`TelemetryCategory`] matches
    /// `category`, regardless of age or attempt count. This is the
    /// "revocation invalidates already-queued events" primitive: call it
    /// with `Analytics` the moment analytics consent turns off (or
    /// `Crash` the moment crash consent turns off) and every event that
    /// consent no longer covers is gone, atomically, in one transaction —
    /// the same "scan, collect matches, delete each, one commit" shape as
    /// [`Self::prune_older_than`], applied to a different predicate.
    /// Idempotent: calling it again (or when nothing of that category is
    /// queued) is a no-op that returns `0`. Returns the number removed.
    pub fn discard_category(&self, category: TelemetryCategory) -> StudioStorageResult<usize> {
        let tx = self.db.begin_write()?;
        let removed;
        {
            let mut t = tx.open_table(TELEMETRY_QUEUE)?;
            let mut matching = Vec::new();
            for entry in t.iter()? {
                let (k, v) = entry?;
                let event: StudioTelemetryEvent = serde_json::from_slice(v.value())?;
                if event.category == category {
                    matching.push(k.value().to_string());
                }
            }
            for key in &matching {
                t.remove(key.as_str())?;
            }
            removed = matching.len();
        }
        tx.commit()?;
        Ok(removed)
    }

    /// Deletes every event with `created_at < cutoff`, regardless of
    /// `attempt_count` — the bounded-retention backstop for events that
    /// keep failing to deliver forever (today's `events.jsonl` sender has
    /// no such backstop at all; see
    /// `docs/architecture/studio-storage-audit.md` §6/§18's flagged gap).
    /// Returns the number removed.
    pub fn prune_older_than(&self, cutoff: i64) -> StudioStorageResult<usize> {
        let tx = self.db.begin_write()?;
        let removed;
        {
            let mut t = tx.open_table(TELEMETRY_QUEUE)?;
            let mut stale = Vec::new();
            for entry in t.iter()? {
                let (k, v) = entry?;
                let event: StudioTelemetryEvent = serde_json::from_slice(v.value())?;
                if event.created_at < cutoff {
                    stale.push(k.value().to_string());
                }
            }
            for key in &stale {
                t.remove(key.as_str())?;
            }
            removed = stale.len();
        }
        tx.commit()?;
        Ok(removed)
    }
}
