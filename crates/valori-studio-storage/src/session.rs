// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Valori Studio **application** sessions.
//!
//! # What a session means here — and what it explicitly does not
//!
//! A session is "this desktop process ran from launch to exit." It is
//! **not**:
//! - a Valori *execution* (a planner/effect-system operation run — owned
//!   by `valori-planner`/`valori-metadata`, persisted in the node/daemon's
//!   own metadata store, not here),
//! - a pipeline execution (same owner as above), or
//! - a Cloud deployment.
//!
//! Conflating these because they all "have timestamps and statuses" was
//! exactly the mistake `docs/architecture/studio-storage-audit.md` §9
//! flags. If Studio ever wants to show "recent executions," it reads them
//! from the node/daemon's execution API — it does not duplicate them into
//! this table.
//!
//! # Relationship to `desktop/src-tauri/src/telemetry.rs`'s `SESSION_ID`
//!
//! Today, `telemetry.rs` mints a `SessionId`-shaped UUID in-process
//! (`OnceLock<String>`) and stamps it onto every telemetry envelope, but
//! never persists a standalone session record. This store is additive: it
//! gives a place to persist that same session as its own row (start/end/
//! crashed), reusing `valori_domain::SessionId`/`InstallationId` so the two
//! sides speak the same identifier — S1 does not wire `telemetry.rs` to
//! call it yet (see `docs/architecture/studio-storage.md` §"Backward
//! compatibility").

use redb::Database;
use serde::{Deserialize, Serialize};
use valori_domain::{InstallationId, SessionId};

use crate::error::StudioStorageResult;
use crate::schema::{self, SESSIONS};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StudioSessionRecord {
    pub id: SessionId,
    #[serde(default)]
    pub installation_id: Option<InstallationId>,
    pub app_version: String,
    pub platform: String,
    pub started_at: i64,
    #[serde(default)]
    pub ended_at: Option<i64>,
    #[serde(default)]
    pub crashed: bool,
}

impl StudioSessionRecord {
    /// `ended_at.is_none()` — the process either is still running, or
    /// exited without calling [`SessionStore::end`] (killed, crashed
    /// before the exit path ran, OS shutdown). A session found in this
    /// state on the *next* launch is exactly the signal
    /// `desktop/src-tauri/src/telemetry.rs`'s crash-marker mechanism uses
    /// a separate file for today; this field is what would let that
    /// inference be made from `studio.redb` instead, once S2 wires it up.
    pub fn is_open(&self) -> bool {
        self.ended_at.is_none()
    }
}

pub struct SessionStore<'a> {
    db: &'a Database,
}

impl<'a> SessionStore<'a> {
    pub(crate) fn new(db: &'a Database) -> Self {
        Self { db }
    }

    fn key(id: SessionId) -> String {
        id.to_string()
    }

    /// Records the start of a new session. Errors are the caller's to
    /// decide how to handle (this crate never panics on IO/serialization
    /// failure — see crate root docs). If `id` was already started (e.g.
    /// redundant lifecycle calls or dev-mode remounts), returns the existing
    /// record idempotently without overwriting `started_at`.
    pub fn start(
        &self,
        id: SessionId,
        installation_id: Option<InstallationId>,
        app_version: &str,
        platform: &str,
        started_at: i64,
    ) -> StudioStorageResult<StudioSessionRecord> {
        let key = Self::key(id);
        if let Some(existing) = schema::get_json::<StudioSessionRecord>(self.db, SESSIONS, &key)? {
            return Ok(existing);
        }
        let record = StudioSessionRecord {
            id,
            installation_id,
            app_version: app_version.to_string(),
            platform: platform.to_string(),
            started_at,
            ended_at: None,
            crashed: false,
        };
        schema::put_json(self.db, SESSIONS, &key, &record)?;
        Ok(record)
    }

    /// Marks a session ended. `NotFound` if `id` was never started —
    /// this store never fabricates a session record on `end`.
    /// Idempotent if called multiple times on the same session.
    pub fn end(
        &self,
        id: SessionId,
        ended_at: i64,
        crashed: bool,
    ) -> StudioStorageResult<StudioSessionRecord> {
        let key = Self::key(id);
        let mut record: StudioSessionRecord = schema::get_json(self.db, SESSIONS, &key)?
            .ok_or_else(|| crate::error::StudioStorageError::NotFound(format!("session {id}")))?;
        record.ended_at = Some(ended_at);
        record.crashed = crashed;
        schema::put_json(self.db, SESSIONS, &key, &record)?;
        Ok(record)
    }

    /// Scans for any previously open sessions belonging to prior process runs
    /// (`id != current_session_id` where `ended_at.is_none()`) and marks them
    /// as crashed with `ended_at = Some(now)`.
    pub fn reconcile_crashed(
        &self,
        current_session_id: SessionId,
        now: i64,
    ) -> StudioStorageResult<usize> {
        let open = self.open_sessions()?;
        let mut reconciled = 0;
        for s in open {
            if s.id != current_session_id {
                self.end(s.id, now, true)?;
                reconciled += 1;
            }
        }
        Ok(reconciled)
    }

    pub fn get(&self, id: SessionId) -> StudioStorageResult<Option<StudioSessionRecord>> {
        schema::get_json(self.db, SESSIONS, &Self::key(id))
    }

    pub fn list(&self) -> StudioStorageResult<Vec<StudioSessionRecord>> {
        schema::list_json(self.db, SESSIONS)
    }

    /// Sessions with no `ended_at` — see [`StudioSessionRecord::is_open`].
    pub fn open_sessions(&self) -> StudioStorageResult<Vec<StudioSessionRecord>> {
        Ok(self.list()?.into_iter().filter(|s| s.is_open()).collect())
    }

    /// Most recently started sessions first, truncated to `limit`.
    pub fn recent(&self, limit: usize) -> StudioStorageResult<Vec<StudioSessionRecord>> {
        let mut all = self.list()?;
        all.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        all.truncate(limit);
        Ok(all)
    }

    /// Deletes sessions the given `policy` no longer requires kept, relative
    /// to `now` (caller-supplied so pruning stays deterministic and
    /// testable — see [`SessionRetentionPolicy`]'s doc comment for the
    /// exact rule). `current_session_id` is never touched, regardless of
    /// its state — belt-and-suspenders alongside the "never delete an open
    /// session" rule, since the intended call site (S5,
    /// `desktop/src-tauri/src/lib.rs`) runs this before the current
    /// session's row even exists yet.
    ///
    /// Only scans the `sessions` table — no other Studio table (`meta`,
    /// `preferences`, `projects`, `project_cache`, `telemetry_queue`,
    /// `sync_state`, `update_state`) is read or written by this call.
    pub fn prune(
        &self,
        current_session_id: SessionId,
        policy: &SessionRetentionPolicy,
        now: i64,
    ) -> StudioStorageResult<SessionPruneStats> {
        let all = self.list()?;
        let scanned = all.len();

        let mut protected_current = 0usize;
        let mut protected_active = 0usize;
        let mut completed: Vec<StudioSessionRecord> = Vec::new();
        let mut crashed: Vec<StudioSessionRecord> = Vec::new();

        for s in all {
            if s.id == current_session_id {
                protected_current += 1;
                continue;
            }
            if s.is_open() {
                // Should be rare-to-never in practice — reconcile_crashed
                // runs before this in the intended lifecycle, converting
                // every other open session to crashed+ended first. Kept as
                // an explicit, defensive rule rather than an assumption.
                protected_active += 1;
                continue;
            }
            if s.crashed {
                crashed.push(s);
            } else {
                completed.push(s);
            }
        }

        // Newest-first, matching `recent()`'s existing sort key
        // (`started_at`) — age is measured from the same field for
        // consistency, not `ended_at` (a session's "age" is when it
        // happened, not the — usually near-identical — moment it closed).
        completed.sort_by(|a, b| b.started_at.cmp(&a.started_at));

        let mut protected_within_retention = 0usize;
        let mut to_delete: Vec<SessionId> = Vec::new();

        let completed_cutoff = now.saturating_sub(days_to_ms(policy.completed_retention_days));
        for (rank, s) in completed.iter().enumerate() {
            if rank < policy.max_completed_sessions {
                // Always kept — the newest-N floor, independent of age.
                continue;
            }
            // Beyond the newest-N floor: eligible for deletion only once
            // also past the age cutoff. A row-count overflow alone never
            // deletes anything still within the retention window.
            if s.started_at < completed_cutoff {
                to_delete.push(s.id);
            } else {
                protected_within_retention += 1;
            }
        }

        let crashed_cutoff = now.saturating_sub(days_to_ms(policy.crashed_retention_days));
        for s in &crashed {
            if s.started_at < crashed_cutoff {
                to_delete.push(s.id);
            } else {
                protected_within_retention += 1;
            }
        }

        // Oldest-first deletion order — deterministic, and means an
        // interrupted prune (e.g. a future timeout/limit) always drops the
        // oldest eligible rows first, never an arbitrary subset.
        to_delete.sort_by_key(|id| {
            completed
                .iter()
                .chain(crashed.iter())
                .find(|s| s.id == *id)
                .map(|s| s.started_at)
                .unwrap_or(i64::MAX)
        });

        let mut deleted = 0usize;
        for id in &to_delete {
            if schema::delete_key(self.db, SESSIONS, &Self::key(*id))? {
                deleted += 1;
            }
        }

        Ok(SessionPruneStats {
            scanned,
            deleted,
            retained: scanned - deleted,
            protected_active,
            protected_current,
            protected_within_retention,
        })
    }
}

fn days_to_ms(days: i64) -> i64 {
    days.saturating_mul(24 * 60 * 60 * 1000)
}

/// The Studio session retention policy — see
/// `docs/architecture/studio-storage.md` §"Session retention (S5)" and
/// `docs/phases/phase-studio-S5-session-retention.md` for the full
/// rationale.
///
/// # The exact rule
///
/// - An **open** session (`ended_at.is_none()`) is never pruned, under any
///   circumstance. Neither is the current session, explicitly, regardless
///   of its own state.
/// - A **completed** session (`ended_at.is_some() && !crashed`) is pruned
///   only when **both**: (a) it is not among the
///   [`max_completed_sessions`](Self::max_completed_sessions) most recent
///   completed sessions by `started_at`, **and** (b) its `started_at` is
///   older than [`completed_retention_days`](Self::completed_retention_days).
///   A completed-session count over the cap never triggers deletion on its
///   own — a session that's technically "excess" by rank but still within
///   the age window survives until it ages out on a later prune.
/// - A **crashed** session (`ended_at.is_some() && crashed`) is pruned only
///   once its `started_at` is older than
///   [`crashed_retention_days`](Self::crashed_retention_days). No count cap
///   applies to crashed sessions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionRetentionPolicy {
    /// Always keep at least this many of the most recent completed
    /// sessions, regardless of age. Default: 100.
    pub max_completed_sessions: usize,
    /// A completed session beyond the `max_completed_sessions` floor
    /// becomes eligible for pruning once older than this many days.
    /// Default: 90.
    pub completed_retention_days: i64,
    /// A crashed session becomes eligible for pruning once older than this
    /// many days. No count cap applies to crashed sessions. Default: 180.
    pub crashed_retention_days: i64,
}

impl Default for SessionRetentionPolicy {
    fn default() -> Self {
        Self {
            max_completed_sessions: 100,
            completed_retention_days: 90,
            crashed_retention_days: 180,
        }
    }
}

/// Result of a [`SessionStore::prune`] call. `scanned == retained +
/// deleted` always; `protected_*` fields explain *why* the retained rows
/// (beyond simple recency) were kept, for logging/diagnostics — see
/// `desktop/src-tauri/src/lib.rs`'s startup integration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SessionPruneStats {
    /// Total session rows examined.
    pub scanned: usize,
    /// Rows actually removed.
    pub deleted: usize,
    /// Rows left in the table after this call (`scanned - deleted`).
    pub retained: usize,
    /// Open sessions other than the current one — never eligible, kept
    /// unconditionally.
    pub protected_active: usize,
    /// The current session — excluded unconditionally, regardless of state.
    pub protected_current: usize,
    /// Completed/crashed sessions kept because they were still within
    /// their respective retention window (the newest-N floor, or the
    /// age cutoff).
    pub protected_within_retention: usize,
}
