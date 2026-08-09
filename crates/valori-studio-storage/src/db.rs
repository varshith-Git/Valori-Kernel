// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `StudioDatabase` — the single typed owner of `studio.redb`.
//!
//! # Ownership
//!
//! Exactly one `StudioDatabase` should be open per process (enforced today
//! by `desktop/src-tauri`'s `tauri-plugin-single-instance`, which already
//! guarantees only one Studio process runs at a time — see
//! `docs/architecture/studio-storage.md` §"Concurrency"). Share one
//! instance across tasks with `Arc<StudioDatabase>`; do not open a second
//! handle onto the same file from the same process.
//!
//! # No `redb::Database` in the public API
//!
//! Every field and method here that touches `redb` types is private. The
//! rest of the application is expected to go through the typed accessors
//! (`preferences()`, `projects()`, …), each of which hands back a small
//! `*Store<'_>` / `*Registry<'_>` handle scoped to exactly one table.

use std::path::Path;

use redb::{Database, WriteTransaction};

use crate::error::{StudioStorageError, StudioStorageResult};
use crate::project::ProjectRegistry;
use crate::project_cache::ProjectCacheStore;
use crate::schema::{self, CURRENT_SCHEMA_VERSION};
use crate::session::SessionStore;
use crate::sync::SyncStateStore;
use crate::telemetry::TelemetryQueue;
use crate::update::UpdateStateStore;
use crate::{path, preferences::PreferencesStore};

/// One migration step: mutates `tx` to bring the database from the version
/// just below this migration's target up to it. Must be idempotent-safe in
/// the sense that it only ever *adds* structure (redb's `open_table` is
/// already create-if-absent — see `schema::create_all_tables`) — never
/// drops a table or a key outright without first having copied forward
/// whatever it needs to preserve.
///
/// Empty today: schema v1 is the first version this crate has ever shipped,
/// so there is nothing to migrate *from*. This is the scaffold S2+ schema
/// changes hang their migrations on — see
/// `docs/architecture/studio-storage.md` §"Schema versioning and migrations".
type MigrationFn = fn(&WriteTransaction) -> StudioStorageResult<()>;

/// `(version this migration produces, human-readable description, function)`,
/// ordered by ascending target version. `run_migrations` applies every entry
/// whose target version is greater than the database's current version, in
/// order, within one write transaction.
const MIGRATIONS: &[(u32, &str, MigrationFn)] = &[];

pub struct StudioDatabase {
    // `pub(crate)`, not private: `crate::migration`'s import logic needs to
    // open a single write transaction spanning both a data table (e.g.
    // `preferences`) and `meta` (the migration-completed flag) atomically —
    // something the per-store `schema::*_json` helpers can't do since each
    // opens its own transaction. Still never exposed outside this crate —
    // see the module doc's "No `redb::Database` in the public API".
    pub(crate) db: Database,
}

// `redb::Database` does not implement `Debug`, so this is written by hand
// rather than derived — deliberately minimal (no table contents), just
// enough for `{:?}` in a panic message or log line to be useful.
impl std::fmt::Debug for StudioDatabase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StudioDatabase").finish_non_exhaustive()
    }
}

impl StudioDatabase {
    /// Opens (or creates) the database at `path`, running schema creation
    /// or migration as needed. Creates `path`'s parent directory if it does
    /// not exist.
    ///
    /// Behavior by on-disk state:
    /// - **No file / empty file**: `redb::Database::create` initializes a
    ///   fresh database; this then creates every table in
    ///   [`schema::ALL_TABLES`] and stamps `schema_version =
    ///   CURRENT_SCHEMA_VERSION`, in one transaction.
    /// - **Existing file, version == current**: tables are (re-)ensured to
    ///   exist (a cheap no-op if they already do — `open_table` never
    ///   truncates) and nothing else happens.
    /// - **Existing file, version < current**: [`run_migrations`] applies
    ///   every pending step from [`MIGRATIONS`], atomically.
    /// - **Existing file, version > current**: returns
    ///   [`StudioStorageError::UnsupportedSchemaVersion`] *before* opening
    ///   any write transaction — the file is never touched.
    /// - **File exists but is not a valid redb database** (corruption):
    ///   `redb::Database::create` itself returns `Err`, surfaced here as
    ///   [`StudioStorageError::Db`]. This crate never deletes or
    ///   recreates a database it failed to open — see
    ///   `docs/architecture/studio-storage.md` §"Corruption and recovery".
    pub fn open(path: &Path) -> StudioStorageResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db = Database::create(path)?;

        match schema::read_schema_version(&db)? {
            None => {
                // Fresh database (or one with no meta table at all, which
                // is the same thing from this crate's perspective — see
                // `schema::read_schema_version`).
                let tx = db.begin_write()?;
                schema::create_all_tables(&tx)?;
                schema::write_schema_version(&tx, CURRENT_SCHEMA_VERSION)?;
                tx.commit()?;
            }
            Some(v) if v == CURRENT_SCHEMA_VERSION => {
                // Defensive: ensure every table this build expects exists,
                // in case a future version ever adds a table without
                // bumping the schema version for it (shouldn't happen, but
                // this keeps `open` idempotent regardless). Cheap: creating
                // an already-existing table is a no-op read of its header.
                let tx = db.begin_write()?;
                schema::create_all_tables(&tx)?;
                tx.commit()?;
            }
            Some(v) if v < CURRENT_SCHEMA_VERSION => {
                run_migrations(&db, v)?;
            }
            Some(v) => {
                return Err(StudioStorageError::UnsupportedSchemaVersion {
                    found: v,
                    supported: CURRENT_SCHEMA_VERSION,
                });
            }
        }

        Ok(Self { db })
    }

    /// Opens the database at the default location
    /// ([`path::default_db_path`] — `$VALORI_HOME/studio.redb`, or
    /// `~/.valori/studio.redb`). Convenience wrapper around [`Self::open`]
    /// for the common case.
    pub fn open_default() -> StudioStorageResult<Self> {
        Self::open(&path::default_db_path())
    }

    /// Recovery-aware open at the default location: preserves a corrupt
    /// `studio.redb`, restores the newest valid backup, or falls back to a
    /// fresh database — but never fails for a condition recovery can
    /// resolve. See `crate::recovery` module docs for the full contract
    /// and `docs/architecture/studio-storage.md` §"Corruption and
    /// recovery". This is what `desktop/src-tauri` calls; [`Self::open`]
    /// / [`Self::open_default`] stay the plain, non-recovering primitive
    /// `crate::recovery` itself (and every existing test) is built on.
    pub fn open_default_with_recovery(
    ) -> StudioStorageResult<(Self, crate::recovery::RecoveryOutcome)> {
        crate::recovery::open_with_recovery(
            &path::default_db_path(),
            &path::default_backups_dir(),
            &path::default_recovery_log_path(),
        )
    }

    /// The schema version currently stored on disk. Always
    /// `schema::CURRENT_SCHEMA_VERSION` immediately after a successful
    /// [`Self::open`] — exposed mainly for tests and diagnostics.
    pub fn schema_version(&self) -> StudioStorageResult<u32> {
        schema::read_schema_version(&self.db)?
            .ok_or_else(|| StudioStorageError::NotFound("schema_version".to_string()))
    }

    // ── Typed store accessors ────────────────────────────────────────────

    pub fn preferences(&self) -> PreferencesStore<'_> {
        PreferencesStore::new(&self.db)
    }

    /// The Studio-local project registry: local projects, cloud project
    /// references, favorites, and (derived, not stored separately) recency.
    /// **Authoritative** for Studio's own bookkeeping — see
    /// `crate::project` module docs.
    pub fn projects(&self) -> ProjectRegistry<'_> {
        ProjectRegistry::new(&self.db)
    }

    /// The disposable, never-authoritative project display cache. Deleting
    /// everything in this store must never affect [`Self::projects`] — see
    /// `crate::project_cache` module docs.
    pub fn project_cache(&self) -> ProjectCacheStore<'_> {
        ProjectCacheStore::new(&self.db)
    }

    pub fn sessions(&self) -> SessionStore<'_> {
        SessionStore::new(&self.db)
    }

    pub fn telemetry(&self) -> TelemetryQueue<'_> {
        TelemetryQueue::new(&self.db)
    }

    pub fn sync(&self) -> SyncStateStore<'_> {
        SyncStateStore::new(&self.db)
    }

    pub fn updates(&self) -> UpdateStateStore<'_> {
        UpdateStateStore::new(&self.db)
    }

    // ── S2a: one-time legacy-data migration ──────────────────────────────
    //
    // Not to be confused with `MIGRATIONS`/`MigrationFn` above, which
    // migrate `studio.redb`'s own *schema* between versions of this crate.
    // These methods migrate *data from a different, older store entirely*
    // (`preferences.json`, `events.jsonl`) into this one, once. See
    // `crate::migration` module docs for the full contract.

    /// Imports `preferences.json`'s bytes. See [`crate::migration::migrate_legacy_preferences`].
    pub fn migrate_legacy_preferences(
        &self,
        json_bytes: &[u8],
        migrated_at: i64,
    ) -> StudioStorageResult<crate::migration::MigrationReport> {
        crate::migration::migrate_legacy_preferences(&self.db, json_bytes, migrated_at)
    }

    /// Reads `path` and imports it, if present. See
    /// [`crate::migration::migrate_legacy_preferences_from_path`].
    pub fn migrate_legacy_preferences_from_path(
        &self,
        path: &Path,
        migrated_at: i64,
    ) -> StudioStorageResult<crate::migration::MigrationReport> {
        crate::migration::migrate_legacy_preferences_from_path(&self.db, path, migrated_at)
    }

    /// Imports `events.jsonl`'s lines. See [`crate::migration::migrate_legacy_telemetry_queue`].
    pub fn migrate_legacy_telemetry_queue(
        &self,
        jsonl_bytes: &[u8],
        migrated_at: i64,
    ) -> StudioStorageResult<crate::migration::MigrationReport> {
        crate::migration::migrate_legacy_telemetry_queue(&self.db, jsonl_bytes, migrated_at)
    }

    /// Reads `path` and imports it, if present. See
    /// [`crate::migration::migrate_legacy_telemetry_queue_from_path`].
    pub fn migrate_legacy_telemetry_queue_from_path(
        &self,
        path: &Path,
        migrated_at: i64,
    ) -> StudioStorageResult<crate::migration::MigrationReport> {
        crate::migration::migrate_legacy_telemetry_queue_from_path(&self.db, path, migrated_at)
    }

    /// Runs both legacy-source migrations against caller-resolved paths
    /// (this crate cannot resolve Tauri's OS-specific app-config directory
    /// itself — see `crate::path` module docs). Either path may be `None`
    /// (nothing to migrate from that source) or point at a file that
    /// doesn't exist (reported via `MigrationReport::source_found`, not an
    /// error). Each source migrates independently — a failure importing
    /// one does not prevent the other from being attempted.
    pub fn run_legacy_migration(
        &self,
        paths: &LegacyStudioPaths,
        migrated_at: i64,
    ) -> LegacyMigrationSummary {
        let preferences = match &paths.preferences_json {
            Some(p) => self.migrate_legacy_preferences_from_path(p, migrated_at),
            None => Ok(crate::migration::MigrationReport::default()),
        };
        let telemetry = match &paths.events_jsonl {
            Some(p) => self.migrate_legacy_telemetry_queue_from_path(p, migrated_at),
            None => Ok(crate::migration::MigrationReport::default()),
        };
        LegacyMigrationSummary {
            preferences,
            telemetry,
        }
    }

    /// The name-only `recentProjects`/`favoriteProjects`/`lastOpenedProject`
    /// residue carried over by [`Self::migrate_legacy_preferences`], if
    /// that migration has run. See `crate::migration` module docs for why
    /// this is not in [`Self::projects`].
    pub fn legacy_project_names(
        &self,
    ) -> StudioStorageResult<Option<crate::migration::LegacyProjectNames>> {
        crate::migration::legacy_project_names(&self.db)
    }
}

/// Caller-resolved paths to the two legacy Studio persistence sources.
/// Both are optional — pass `None` for a source that either doesn't apply
/// or whose location the caller hasn't determined.
#[derive(Clone, Debug, Default)]
pub struct LegacyStudioPaths {
    pub preferences_json: Option<std::path::PathBuf>,
    pub events_jsonl: Option<std::path::PathBuf>,
}

/// Result of [`StudioDatabase::run_legacy_migration`]. Each field is its
/// own `Result` — one source failing (e.g. a corrupt `preferences.json`)
/// does not prevent the other from being reported.
#[derive(Debug)]
pub struct LegacyMigrationSummary {
    pub preferences: StudioStorageResult<crate::migration::MigrationReport>,
    pub telemetry: StudioStorageResult<crate::migration::MigrationReport>,
}

/// Applies every migration in [`MIGRATIONS`] whose target version is above
/// `from_version`, in a single write transaction, then stamps the final
/// version. If any step returns `Err`, the transaction is dropped without
/// committing — redb write transactions are atomic, so the database is left
/// exactly at `from_version` on disk, never partially migrated. This is
/// what satisfies "never partially claim migration success": either every
/// pending step applied and the version bump committed together, or none of
/// it did.
fn run_migrations(db: &Database, from_version: u32) -> StudioStorageResult<()> {
    let pending: Vec<_> = MIGRATIONS
        .iter()
        .filter(|(target, _, _)| *target > from_version && *target <= CURRENT_SCHEMA_VERSION)
        .collect();

    if pending.is_empty() {
        // Nothing registered between `from_version` and current — the
        // database is behind but this build has no path forward for it.
        // This should not currently be reachable (CURRENT_SCHEMA_VERSION is
        // 1 and there is no version below 1), but fail loudly rather than
        // silently leaving the stored version stale if it ever is.
        return Err(StudioStorageError::MigrationFailed {
            from: from_version,
            to: CURRENT_SCHEMA_VERSION,
            reason: "no migration path registered for this version gap".to_string(),
        });
    }

    let tx = db.begin_write()?;
    let mut applied_to = from_version;
    for (target, description, migrate) in pending {
        migrate(&tx).map_err(|e| StudioStorageError::MigrationFailed {
            from: applied_to,
            to: *target,
            reason: format!("{description}: {e}"),
        })?;
        applied_to = *target;
    }
    schema::write_schema_version(&tx, applied_to)?;
    tx.commit()?;
    Ok(())
}
