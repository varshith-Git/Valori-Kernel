// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Corruption-safe startup for `studio.redb`: preserve, restore, or start
//! fresh — but never let `studio.redb` prevent Valori Studio from
//! launching, and never touch anything outside `studio.redb` and its own
//! backups.
//!
//! # The one invariant everything else here serves
//!
//! `studio.redb` is **recoverable Studio metadata** — preferences, the
//! project *registry* (a reference/cache layer, not project data itself —
//! see `crate::project` module docs), sessions, the telemetry queue, sync
//! and update state. It is never the authoritative store for a Valori
//! project's vectors, WAL, snapshots, indexes, or collections — those live
//! under `$VALORI_HOME/projects/<name>/`, owned by `valori-kernel`/
//! `valori-wire`/`valori-storage`/the daemon, and nothing in this module
//! (or this crate) has a code path that can reach them. That separation is
//! structural (the dependency firewall — see `crates/valori-node/tests/dependency_direction.rs`),
//! not just documented, which is what makes "recovery of studio.redb can
//! never touch project data" true by construction rather than by promise.
//!
//! # Recovery order
//!
//! ```text
//! 1. Try opening the current studio.redb
//! 2. If it opens        → Healthy, done
//! 3. If it doesn't       → preserve the original (atomic rename aside)
//! 4. Try the newest-to-oldest backup generation
//! 5. First one that opens AND validates → restore it, done
//! 6. None do             → create a fresh studio.redb, done
//! 7. (Rebuilding: see "Rebuild classification" below — mostly a no-op)
//! 8. Return control to the caller either way — recovery never blocks
//!    startup indefinitely and never returns an unrecoverable error for a
//!    condition a fresh database can resolve.
//! ```
//!
//! # Rebuild classification (evidence-based, not aspirational)
//!
//! | Table | On fresh DB | Why |
//! |---|---|---|
//! | `preferences` | Restored from backup if one validates; otherwise `StudioPreferences::default()` | No independent source outside `studio.redb`/its backups exists — "safe defaults" is the only fallback, exactly as specified |
//! | `update_state` | `StudioUpdateState::default()` | Already the existing "absent = default" behavior (see `crate::update`) — trivially rebuildable, no new code needed |
//! | `telemetry_queue` | Empty | Disposable by existing product semantics — undelivered events are already best-effort (see `crate::telemetry` module docs) |
//! | `sessions` | Empty | Disposable — a fresh DB with no session history is fine; the next `SessionStore::start` just creates a new record |
//! | `sync_state` | Empty | Cloud remains authoritative for Cloud projects (see `crate::sync` module docs) — re-derivable once a sync engine exists; nothing is lost that Cloud doesn't already have |
//! | `projects` (registry) / `project_cache` | **Not rebuilt automatically** | This crate has no parser for the daemon's `project.json` and must not gain one — depending on `valori-daemon` to read it would violate the dependency firewall (`SEALED_CRATES`), and hand-rolling a duplicate parser here would violate the single-owner-per-concept rule in `docs/architecture/ownership.md`. A fresh registry is empty; whichever phase wires the app to the daemon's live project list (S2b's own deferred follow-up) is what actually repopulates it, the same way it would on first install. Documented here as a real, current limitation — not invented "rebuild" behavior for something the architecture doesn't yet support. |
//! | `meta.legacy_project_names` | Lost | Only ever existed as one-time S2a migration residue in the same file that just got recreated — there is nothing else to recover it from |
//!
//! **Never rebuilt inside `studio.redb`, under any circumstance:** WAL,
//! snapshots, vectors, indexes, collections, model artifacts — this crate
//! has no code path to them at all (see the firewall note above).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use redb::{Database, ReadableTable};
use serde::{Deserialize, Serialize};

use crate::db::StudioDatabase;
use crate::error::{StudioStorageError, StudioStorageResult};
use crate::schema;

/// How many backup generations to keep. `studio.redb.1` is newest,
/// `studio.redb.{BACKUP_GENERATIONS}` is oldest. Bounded on purpose — see
/// module docs' "Backup strategy": this is a small rolling window, not an
/// archive.
pub const BACKUP_GENERATIONS: u32 = 3;

/// A periodic backup is taken at most this often, gated by the *newest*
/// backup's mtime — see [`maybe_periodic_backup`]. Keeps backup creation
/// off the hot path (never triggered by a preference write or a telemetry
/// enqueue) without needing a background scheduler.
const PERIODIC_BACKUP_INTERVAL_SECS: u64 = 24 * 3_600;

/// Explicit recovery states, persisted into each
/// [`RecoveryLogEntry`] so the sequence of what happened is inspectable
/// after the fact, not just inferable from which [`RecoveryOutcome`]
/// variant resulted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryState {
    Healthy,
    RecoveryRequired,
    RestoringBackup,
    Rebuilding,
    Recovered,
    RecoveryFailed,
}

/// What [`open_with_recovery`] actually did. Every non-`Healthy` variant
/// carries enough detail for the caller to build the user-facing notice
/// described in `docs/architecture/studio-storage.md` §"Recovery UI"
/// without needing to inspect the filesystem itself.
#[derive(Clone, Debug, PartialEq)]
pub enum RecoveryOutcome {
    /// The database opened normally. No corruption detected.
    Healthy,
    /// The original database could not be opened; a validated backup was
    /// restored in its place. The original is preserved at
    /// `corrupt_original`, never deleted.
    RestoredFromBackup {
        backup_generation: u32,
        corrupt_original: PathBuf,
    },
    /// The original database could not be opened and no backup validated
    /// (or none existed); a fresh, empty `studio.redb` was created so the
    /// app can still launch. `corrupt_original` is `None` only when there
    /// was no file to preserve at all (e.g. a permission error prevented
    /// even reading it, or this is — despite reaching the recovery path —
    /// somehow a true first run).
    FreshDatabaseCreated { corrupt_original: Option<PathBuf> },
}

impl RecoveryOutcome {
    pub fn is_healthy(&self) -> bool {
        matches!(self, RecoveryOutcome::Healthy)
    }

    /// A short, non-technical sentence suitable for the non-blocking
    /// notice described in the Recovery UI spec. Raw `redb`/IO errors are
    /// deliberately never surfaced here — see [`RecoveryLogEntry::reason`]
    /// for where the technical detail lives instead.
    pub fn user_message(&self) -> Option<&'static str> {
        match self {
            RecoveryOutcome::Healthy => None,
            RecoveryOutcome::RestoredFromBackup { .. } => Some(
                "Studio recovered its local metadata database from a backup. \
                 Your project data was not modified.",
            ),
            RecoveryOutcome::FreshDatabaseCreated { .. } => Some(
                "Studio recreated its local metadata database. Your project \
                 data was not modified; some Studio preferences and recent \
                 activity may need to be set again.",
            ),
        }
    }
}

/// One line of `studio-recovery.jsonl` — see module docs and
/// `path::default_recovery_log_path`. Deliberately excludes anything
/// resembling preference values, telemetry payloads, or credentials —
/// only recovery *mechanics* are recorded, matching
/// `docs/architecture/studio-storage.md` §"Logging".
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoveryLogEntry {
    pub recovery_timestamp: i64,
    pub state: RecoveryState,
    /// A short, technical-but-not-sensitive description (e.g. the `redb`
    /// error's `Display` text) — for diagnostics, never shown to the user
    /// as the primary message. See [`RecoveryOutcome::user_message`].
    pub reason: String,
    pub original_database_path: String,
    pub backup_attempted: bool,
    pub backup_restored: bool,
    pub fresh_database_created: bool,
}

/// Deliberate exception to the rest of the crate's "take timestamps as
/// arguments, never read the clock" convention (see e.g. `crate::telemetry`
/// module docs): recovery is triggered internally, autonomously, by a
/// database-open failure — there is no caller-supplied "now" available at
/// that point the way there is for an explicit `SessionStore::start` call.
/// Recovery log timestamps and the periodic-backup age check both need a
/// real wall clock.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Appends one entry to `studio-recovery.jsonl`. Best-effort: a failure to
/// write the log must never fail recovery itself — recovery already
/// succeeded or failed on its own terms by the time this is called.
fn append_recovery_log(log_path: &Path, entry: &RecoveryLogEntry) {
    let Ok(line) = serde_json::to_string(entry) else {
        return;
    };
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        let _ = writeln!(f, "{line}");
    }
}

/// Opens `studio.redb` at `db_path`, recovering from corruption if
/// necessary. Never returns `Err` for a condition a fresh database can
/// resolve — the only `Err` case is one where even creating a brand new
/// `studio.redb` failed (disk full, directory unwritable, …), which the
/// caller (`desktop/src-tauri`) already treats as "Studio storage
/// unavailable, continue without it" rather than a fatal error. See
/// `desktop/src-tauri/src/studio_storage.rs::init_studio_storage`.
///
/// `db_path`, `backups_dir`, and `recovery_log_path` are all
/// caller-supplied (not resolved internally) so this function stays
/// testable against a disposable temp directory — production callers pass
/// `path::default_db_path()` / `path::default_backups_dir()` /
/// `path::default_recovery_log_path()`.
pub fn open_with_recovery(
    db_path: &Path,
    backups_dir: &Path,
    recovery_log_path: &Path,
) -> StudioStorageResult<(StudioDatabase, RecoveryOutcome)> {
    // Step 1: try the current database, including the case where a prior
    // crashed recovery attempt already renamed it aside (db_path absent)
    // but never finished restoring or creating a replacement — see module
    // docs and the crash-safety test suite. Both cases funnel into the
    // same recovery path below; there is no separate "resume" branch to
    // get out of sync with the fresh-corruption branch.
    if db_path.exists() {
        // Backup creation trigger: "before schema migration" — see module
        // docs. If this open is about to run a migration, back up the
        // pre-migration file first, so a migration failure below has this
        // exact state to fall back to via the normal backup-restoration
        // path in `recover`. Best-effort and non-mutating (only inspects
        // the version, never opens for write) — a failed pre-check just
        // means no backup was taken, not that the open is blocked.
        if will_need_migration(db_path) {
            backup_before_migration(db_path, backups_dir);
        }
        match StudioDatabase::open(db_path) {
            Ok(db) => {
                maybe_periodic_backup(db_path, backups_dir);
                return Ok((db, RecoveryOutcome::Healthy));
            }
            Err(open_err) => {
                // `DatabaseAlreadyOpen` means exactly what it says — another
                // handle (almost always another process; the single-instance
                // plugin should make that impossible in practice, but this
                // is the defense-in-depth line, not a place to trust that)
                // already has this exact file locked. The database is not
                // corrupt. Treating "locked" the same as "corrupt" would be
                // actively destructive: it would preserve-aside and start
                // rewriting backups for a database that is perfectly fine
                // and in active use — see module docs' "never touch project
                // data" spirit applied to Studio's own database too.
                // Propagate cleanly instead; the caller decides what a
                // "Studio is already running" message looks like.
                if matches!(
                    &open_err,
                    StudioStorageError::Db(redb::DatabaseError::DatabaseAlreadyOpen)
                ) {
                    return Err(open_err);
                }
                return recover(
                    db_path,
                    backups_dir,
                    recovery_log_path,
                    open_err.to_string(),
                );
            }
        }
    }

    // db_path does not exist. Distinguish "genuinely nothing here yet" —
    // the common fresh-install case, which must stay silent and cheap —
    // from "a previous recovery attempt already preserved a corrupt file
    // and got interrupted before finishing" by checking for leftover
    // `studio.redb.corrupt-*` markers. Only the latter re-enters the full
    // recovery path (still safe either way: recovery is idempotent).
    if has_preserved_corrupt_marker(db_path) {
        return recover(
            db_path,
            backups_dir,
            recovery_log_path,
            "resuming interrupted recovery from a previous launch".to_string(),
        );
    }

    let db = StudioDatabase::open(db_path)?;
    Ok((db, RecoveryOutcome::Healthy))
}

/// Read-only inspection of whether opening `db_path` would trigger a
/// schema migration — never opens for write, never mutates the file.
/// Returns `false` (not an error) if the file can't even be inspected;
/// the real open right after this call will surface that failure properly
/// through the normal recovery path.
fn will_need_migration(db_path: &Path) -> bool {
    let Ok(db) = Database::open(db_path) else {
        return false;
    };
    matches!(
        schema::read_schema_version(&db),
        Ok(Some(v)) if v < schema::CURRENT_SCHEMA_VERSION
    )
}

fn has_preserved_corrupt_marker(db_path: &Path) -> bool {
    let Some(dir) = db_path.parent() else {
        return false;
    };
    let Some(stem) = db_path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let prefix = format!("{stem}.corrupt-");
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
}

/// Steps 3–8 of the recovery order: preserve, try backups oldest-newest
/// (newest generation first), and fall back to fresh. Always succeeds
/// unless even fresh creation fails.
fn recover(
    db_path: &Path,
    backups_dir: &Path,
    recovery_log_path: &Path,
    reason: String,
) -> StudioStorageResult<(StudioDatabase, RecoveryOutcome)> {
    tracing::warn!("studio database open failed: {reason}");
    log_state(
        recovery_log_path,
        db_path,
        RecoveryState::RecoveryRequired,
        &reason,
        false,
        false,
        false,
    );

    // Step 3: preserve the original — never delete, never overwrite an
    // existing preserved copy (the timestamp in the name makes a
    // collision astronomically unlikely, but a collision is handled by
    // appending a counter rather than silently overwriting).
    let corrupt_original = preserve_corrupt(db_path);
    if let Some(preserved) = &corrupt_original {
        tracing::warn!("studio database preserved at {}", preserved.display());
    }

    // Steps 4–5: newest generation first (".1" is newest — see
    // `rotate_backups`), skip any that don't validate.
    tracing::info!("studio database recovery: attempting backups");
    log_state(
        recovery_log_path,
        db_path,
        RecoveryState::RestoringBackup,
        &reason,
        true,
        false,
        false,
    );
    for generation in 1..=BACKUP_GENERATIONS {
        let candidate = backups_dir.join(format!("studio.redb.{generation}"));
        if !candidate.exists() {
            continue;
        }
        if !validate_database_file(&candidate) {
            tracing::warn!("backup generation {generation} failed validation, skipping");
            continue;
        }
        match restore_backup(&candidate, db_path) {
            Ok(db) => {
                tracing::info!("studio database recovered from backup generation {generation}");
                log_state(
                    recovery_log_path,
                    db_path,
                    RecoveryState::Recovered,
                    &reason,
                    true,
                    true,
                    false,
                );
                return Ok((
                    db,
                    RecoveryOutcome::RestoredFromBackup {
                        backup_generation: generation,
                        corrupt_original: corrupt_original.unwrap_or_else(|| db_path.to_path_buf()),
                    },
                ));
            }
            Err(e) => {
                tracing::warn!("backup generation {generation} restored but failed to open: {e}");
                let _ = std::fs::remove_file(db_path);
                continue;
            }
        }
    }

    // Step 6: no valid backup — start fresh. `db_path` is guaranteed
    // absent at this point (preserved away, or never existed), so this is
    // a genuinely new database, not an overwrite of anything.
    tracing::warn!("studio database recovery: no valid backup, creating a fresh database");
    log_state(
        recovery_log_path,
        db_path,
        RecoveryState::Rebuilding,
        &reason,
        true,
        false,
        true,
    );
    match StudioDatabase::open(db_path) {
        Ok(db) => {
            log_state(
                recovery_log_path,
                db_path,
                RecoveryState::Recovered,
                &reason,
                true,
                false,
                true,
            );
            Ok((
                db,
                RecoveryOutcome::FreshDatabaseCreated { corrupt_original },
            ))
        }
        Err(e) => {
            log_state(
                recovery_log_path,
                db_path,
                RecoveryState::RecoveryFailed,
                &format!("{reason}; fresh database creation also failed: {e}"),
                true,
                false,
                false,
            );
            Err(e)
        }
    }
}

fn log_state(
    log_path: &Path,
    db_path: &Path,
    state: RecoveryState,
    reason: &str,
    backup_attempted: bool,
    backup_restored: bool,
    fresh_database_created: bool,
) {
    append_recovery_log(
        log_path,
        &RecoveryLogEntry {
            recovery_timestamp: now_ms(),
            state,
            reason: reason.to_string(),
            original_database_path: db_path.display().to_string(),
            backup_attempted,
            backup_restored,
            fresh_database_created,
        },
    );
}

/// Renames `db_path` to a deterministic, collision-safe
/// `studio.redb.corrupt-<unix_ms>` name in the same directory (same
/// filesystem — `fs::rename` is atomic there on macOS, Windows, and
/// Linux; it never partially applies). Returns `None` (not an error) if
/// `db_path` didn't exist to begin with, or if the rename itself fails
/// (e.g. a permission error) — either way, recovery continues rather than
/// aborting, since the goal is "the app must still launch," not "the
/// preserve step must always succeed."
fn preserve_corrupt(db_path: &Path) -> Option<PathBuf> {
    if !db_path.exists() {
        return None;
    }
    let dir = db_path.parent()?;
    let stem = db_path.file_name()?.to_str()?;
    let mut target = dir.join(format!("{stem}.corrupt-{}", now_ms()));
    // Collision guard: astronomically unlikely at millisecond resolution,
    // but never silently overwrite an existing preserved artifact — see
    // module docs' "Preserve corrupted databases".
    let mut counter = 1;
    while target.exists() {
        target = dir.join(format!("{stem}.corrupt-{}-{counter}", now_ms()));
        counter += 1;
    }
    match std::fs::rename(db_path, &target) {
        Ok(()) => Some(target),
        Err(e) => {
            tracing::error!("failed to preserve corrupt studio database: {e}");
            None
        }
    }
}

/// Opens `path` read-existing-only (never creates) and checks that
/// `meta.schema_version` is present, within the version range this build
/// understands, and that every table in [`schema::ALL_TABLES`] can be
/// opened. Does not mutate `path` — a backup validated this way stays a
/// pristine archive copy; the live database at its final location is what
/// actually gets migrated, by the normal [`StudioDatabase::open`] call
/// `restore_backup` makes after copying it into place.
fn validate_database_file(path: &Path) -> bool {
    let Ok(db) = Database::open(path) else {
        return false;
    };
    let Ok(version) = schema::read_schema_version(&db) else {
        return false;
    };
    let Some(version) = version else { return false };
    if version == 0 || version > schema::CURRENT_SCHEMA_VERSION {
        return false;
    }
    let Ok(tx) = db.begin_read() else {
        return false;
    };
    for table in schema::ALL_TABLES {
        if tx.open_table(*table).is_err() {
            return false;
        }
    }
    true
}

/// Copies `backup_path` to a temporary file beside `db_path` and
/// atomically renames it into place — `db_path` is never seen in a
/// half-written state, satisfying the "atomic filesystem operations"
/// requirement even though the *copy* itself isn't atomic (copies never
/// are; the rename that publishes the result is what matters). `db_path`
/// must already be absent (the caller preserves it first) — this never
/// overwrites an existing file.
fn restore_backup(backup_path: &Path, db_path: &Path) -> StudioStorageResult<StudioDatabase> {
    let tmp_path = db_path.with_extension("redb.restoring");
    std::fs::copy(backup_path, &tmp_path)?;
    std::fs::rename(&tmp_path, db_path)?;
    StudioDatabase::open(db_path)
}

/// Copies the currently-open, healthy `db_path` to a fresh backup
/// generation, rotating older generations down and dropping anything past
/// [`BACKUP_GENERATIONS`]. Must only be called when no write transaction
/// is in flight — `redb`'s COW B-tree keeps the on-disk file in a fully
/// consistent, previously-committed state between transactions, which is
/// exactly what a plain `std::fs::copy` needs to produce a valid archive
/// copy without redb's own APIs for it. Every step (the fresh copy landing
/// via rename, and each rotation step) is an atomic `fs::rename`, so a
/// crash mid-rotation loses at most one generation of history — it can
/// never corrupt an existing valid backup file.
fn create_backup(db_path: &Path, backups_dir: &Path) -> StudioStorageResult<()> {
    std::fs::create_dir_all(backups_dir)?;
    let tmp_path = backups_dir.join(format!("studio.redb.new-{}", now_ms()));
    std::fs::copy(db_path, &tmp_path)?;

    // Rotate existing generations down (oldest dropped), then land the new
    // copy as generation 1. Walking from oldest to newest means each
    // rename's destination is guaranteed free before it runs.
    let oldest = backups_dir.join(format!("studio.redb.{BACKUP_GENERATIONS}"));
    if oldest.exists() {
        let _ = std::fs::remove_file(&oldest);
    }
    for generation in (1..BACKUP_GENERATIONS).rev() {
        let from = backups_dir.join(format!("studio.redb.{generation}"));
        let to = backups_dir.join(format!("studio.redb.{}", generation + 1));
        if from.exists() {
            std::fs::rename(&from, &to)?;
        }
    }
    std::fs::rename(&tmp_path, backups_dir.join("studio.redb.1"))?;
    Ok(())
}

/// Public entry point for the "before schema migration" backup trigger —
/// see module docs' "Backup creation triggers". Best-effort: a failure to
/// back up must not block a healthy open or a migration that would
/// otherwise succeed; it only means a *future* corruption has one fewer
/// generation to fall back on.
pub(crate) fn backup_before_migration(db_path: &Path, backups_dir: &Path) {
    if let Err(e) = create_backup(db_path, backups_dir) {
        tracing::warn!("pre-migration studio database backup failed (continuing anyway): {e}");
    }
}

/// The "controlled periodic/maintenance backup" trigger. Gated by the
/// newest backup's mtime so this never runs on every launch — only when
/// the newest backup is missing or older than
/// [`PERIODIC_BACKUP_INTERVAL_SECS`]. Never runs on a preference write, a
/// telemetry enqueue, or any other hot-path operation — only from
/// [`open_with_recovery`]'s healthy-open branch, once per process start at
/// most.
fn maybe_periodic_backup(db_path: &Path, backups_dir: &Path) {
    let newest = backups_dir.join("studio.redb.1");
    let stale = match std::fs::metadata(&newest).and_then(|m| m.modified()) {
        Ok(modified) => SystemTime::now()
            .duration_since(modified)
            .map(|age| age.as_secs() >= PERIODIC_BACKUP_INTERVAL_SECS)
            .unwrap_or(true),
        Err(_) => true, // no backup yet — take the first one.
    };
    if stale {
        if let Err(e) = create_backup(db_path, backups_dir) {
            tracing::warn!("periodic studio database backup failed (continuing anyway): {e}");
        }
    }
}
