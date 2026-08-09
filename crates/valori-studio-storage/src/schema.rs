// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Table layout, schema version, and the small JSON-over-redb helpers every
//! store in this crate builds on.
//!
//! # Serialization: JSON, not bincode
//!
//! `valori-consensus`'s Raft log uses `bincode` because it is on a hot,
//! high-frequency path where allocation and encode/decode cost matter and
//! the two ends (writer and reader) are always the same build. Studio's
//! workload is the opposite: low frequency, small values, and the values
//! must stay readable across app updates (see "Schema versioning" below).
//! `valori-metadata::MetadataDb` already made this call for exactly the
//! same reasons — JSON via `serde_json`, `TableDefinition<&str, &[u8]>` —
//! and this crate follows that precedent rather than inventing a second
//! convention. JSON's `#[serde(default)]` support is also what makes a
//! record forward-compatible: a v1 build can add an optional field to a
//! stored struct and older rows still deserialize.
//!
//! # Table layout
//!
//! | Table | Key | Value | Store |
//! |---|---|---|---|
//! | `meta` | `&str` (e.g. `"schema_version"`) | JSON scalar | internal (`db.rs`) |
//! | `preferences` | `"singleton"` | JSON [`crate::preferences::StudioPreferences`] | [`crate::preferences::PreferencesStore`] |
//! | `projects` | [`valori_domain::ProjectId`] as string | JSON [`crate::project::StudioProjectRecord`] | [`crate::project::ProjectRegistry`] |
//! | `project_cache` | [`valori_domain::ProjectId`] as string | JSON [`crate::project_cache::StudioProjectCacheEntry`] | [`crate::project_cache::ProjectCacheStore`] |
//! | `sessions` | [`valori_domain::SessionId`] as string | JSON [`crate::session::StudioSessionRecord`] | [`crate::session::SessionStore`] |
//! | `telemetry_queue` | event id (uuid string) | JSON [`crate::telemetry::StudioTelemetryEvent`] | [`crate::telemetry::TelemetryQueue`] |
//! | `sync_state` | [`valori_domain::ProjectId`] as string | JSON [`crate::sync::StudioSyncState`] | [`crate::sync::SyncStateStore`] |
//! | `update_state` | `"singleton"` | JSON [`crate::update::StudioUpdateState`] | [`crate::update::UpdateStateStore`] |
//!
//! Every table is additive-only at the redb level: `open_table` on a
//! missing table *creates* it (never truncates one that exists), which is
//! what makes `create_all_tables` safe to call on every open, not just the
//! first one — see `crate::db::open`.

use redb::{Database, ReadableTable, TableDefinition, WriteTransaction};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::{StudioStorageError, StudioStorageResult};

// ── Schema version ──────────────────────────────────────────────────────────

/// Bump on every schema change, and add the migration that produces it to
/// `crate::db::MIGRATIONS`. See `docs/architecture/studio-storage.md`
/// §"Schema versioning and migrations".
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

pub const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");
pub const KEY_SCHEMA_VERSION: &str = "schema_version";

// ── S2a legacy-migration flags (see `crate::migration`) ─────────────────────
//
// Stored in `meta` alongside `schema_version` rather than a dedicated table:
// each is a single scalar/small-struct fact about *this database's history*,
// not a collection of records — exactly what `meta` is for. Adding a table
// per flag would be the "blindly create tables that aren't required" the S1
// audit warned against.

/// JSON `i64` (unix ms) — set the moment `migrate_legacy_preferences`
/// commits. Absent means "never run." Presence makes the migration
/// idempotent: a later call is a cheap no-op, not a re-import.
pub const KEY_LEGACY_PREFERENCES_MIGRATED_AT: &str = "legacy_preferences_migrated_at";
/// Same contract as [`KEY_LEGACY_PREFERENCES_MIGRATED_AT`], for
/// `migrate_legacy_telemetry_queue`.
pub const KEY_LEGACY_TELEMETRY_MIGRATED_AT: &str = "legacy_telemetry_migrated_at";
/// JSON `crate::migration::LegacyProjectNames` — the name-only
/// `recentProjects`/`favoriteProjects`/`lastOpenedProject` residue carried
/// over from `preferences.json` by `migrate_legacy_preferences`. Explicitly
/// **not** in the `projects` table — see `crate::migration` module docs.
pub const KEY_LEGACY_PROJECT_NAMES: &str = "legacy_project_names";

// ── Table definitions ────────────────────────────────────────────────────────

pub const PREFERENCES: TableDefinition<&str, &[u8]> = TableDefinition::new("preferences");
pub const PROJECTS: TableDefinition<&str, &[u8]> = TableDefinition::new("projects");
pub const PROJECT_CACHE: TableDefinition<&str, &[u8]> = TableDefinition::new("project_cache");
pub const SESSIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("sessions");
pub const TELEMETRY_QUEUE: TableDefinition<&str, &[u8]> = TableDefinition::new("telemetry_queue");
pub const SYNC_STATE: TableDefinition<&str, &[u8]> = TableDefinition::new("sync_state");
pub const UPDATE_STATE: TableDefinition<&str, &[u8]> = TableDefinition::new("update_state");

/// Every table this schema version owns, in creation order. Used by
/// `create_all_tables` — additive-only, safe to call on every open.
pub const ALL_TABLES: &[TableDefinition<&str, &[u8]>] = &[
    META,
    PREFERENCES,
    PROJECTS,
    PROJECT_CACHE,
    SESSIONS,
    TELEMETRY_QUEUE,
    SYNC_STATE,
    UPDATE_STATE,
];

/// The key used for every singleton (non-keyed) table value, e.g.
/// `preferences` and `update_state`, which hold exactly one logical record.
pub const SINGLETON_KEY: &str = "singleton";

/// Opens every table in [`ALL_TABLES`] within `tx`, creating any that don't
/// exist yet. Never truncates or drops a table that already has rows —
/// `WriteTransaction::open_table` in redb 2.x is create-if-absent, not
/// create-or-replace.
pub(crate) fn create_all_tables(tx: &WriteTransaction) -> StudioStorageResult<()> {
    for table in ALL_TABLES {
        tx.open_table(*table)?;
    }
    Ok(())
}

// ── JSON-over-redb helpers ───────────────────────────────────────────────────
//
// Every store (`preferences.rs`, `project.rs`, …) is built on these four
// functions rather than calling `redb` directly — this is what keeps table
// access confined to this crate's internals instead of scattered across
// every call site (the audit's requirement: "one owner of the Studio
// database, typed access to logical stores"). Each call opens and commits
// its own transaction, matching `valori_metadata::MetadataDb`'s existing
// convention — no long-lived transaction handle crosses a store method
// boundary.

pub(crate) fn get_json<T: DeserializeOwned>(
    db: &Database,
    table: TableDefinition<&str, &[u8]>,
    key: &str,
) -> StudioStorageResult<Option<T>> {
    let tx = db.begin_read()?;
    let t = match tx.open_table(table) {
        Ok(t) => t,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    match t.get(key)? {
        None => Ok(None),
        Some(v) => Ok(Some(serde_json::from_slice(v.value())?)),
    }
}

pub(crate) fn put_json<T: Serialize>(
    db: &Database,
    table: TableDefinition<&str, &[u8]>,
    key: &str,
    value: &T,
) -> StudioStorageResult<()> {
    let bytes = serde_json::to_vec(value)?;
    let tx = db.begin_write()?;
    {
        let mut t = tx.open_table(table)?;
        t.insert(key, bytes.as_slice())?;
    }
    tx.commit()?;
    Ok(())
}

pub(crate) fn delete_key(
    db: &Database,
    table: TableDefinition<&str, &[u8]>,
    key: &str,
) -> StudioStorageResult<bool> {
    let tx = db.begin_write()?;
    let removed;
    {
        let mut t = tx.open_table(table)?;
        removed = t.remove(key)?.is_some();
    }
    tx.commit()?;
    Ok(removed)
}

pub(crate) fn list_json<T: DeserializeOwned>(
    db: &Database,
    table: TableDefinition<&str, &[u8]>,
) -> StudioStorageResult<Vec<T>> {
    let tx = db.begin_read()?;
    let t = match tx.open_table(table) {
        Ok(t) => t,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut out = Vec::new();
    for entry in t.iter()? {
        let (_, v) = entry?;
        out.push(serde_json::from_slice::<T>(v.value())?);
    }
    Ok(out)
}

/// Read-modify-write a single JSON row in one write transaction — the
/// primitive `PreferencesStore::update` and `UpdateStateStore::update` are
/// built on. Atomic: no reader observes a half-applied update, and no two
/// concurrent `update` calls interleave (redb serializes writers).
pub(crate) fn update_json<T, F>(
    db: &Database,
    table: TableDefinition<&str, &[u8]>,
    key: &str,
    default: impl FnOnce() -> T,
    f: F,
) -> StudioStorageResult<T>
where
    T: Serialize + DeserializeOwned + Clone,
    F: FnOnce(&mut T),
{
    let tx = db.begin_write()?;
    let existing_bytes: Option<Vec<u8>> = {
        let t = tx.open_table(table)?;
        let value = t.get(key)?.map(|v| v.value().to_vec());
        value
    };
    let mut current: T = match existing_bytes {
        Some(bytes) => serde_json::from_slice(&bytes)?,
        None => default(),
    };
    f(&mut current);
    let bytes = serde_json::to_vec(&current)?;
    {
        let mut t = tx.open_table(table)?;
        t.insert(key, bytes.as_slice())?;
    }
    tx.commit()?;
    Ok(current)
}

/// Reads `meta.schema_version`, distinguishing "no meta table at all" (a
/// database this crate has never opened) from "meta table exists but the
/// key is absent" (shouldn't happen once `db::open` has run once, but
/// treated the same — `None`) from an actual stored version.
pub(crate) fn read_schema_version(db: &Database) -> StudioStorageResult<Option<u32>> {
    let tx = db.begin_read()?;
    let t = match tx.open_table(META) {
        Ok(t) => t,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    match t.get(KEY_SCHEMA_VERSION)? {
        None => Ok(None),
        Some(v) => {
            let version: u32 =
                serde_json::from_slice(v.value()).map_err(StudioStorageError::from)?;
            Ok(Some(version))
        }
    }
}

pub(crate) fn write_schema_version(tx: &WriteTransaction, version: u32) -> StudioStorageResult<()> {
    let bytes = serde_json::to_vec(&version)?;
    let mut t = tx.open_table(META)?;
    t.insert(KEY_SCHEMA_VERSION, bytes.as_slice())?;
    Ok(())
}
