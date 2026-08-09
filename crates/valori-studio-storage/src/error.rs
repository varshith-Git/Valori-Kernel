// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Typed errors for `valori-studio-storage`.
//!
//! Mirrors `valori_metadata::error` — same shape, same `redb` error variants
//! — plus two variants specific to this crate's schema-versioning contract
//! (`UnsupportedSchemaVersion`, `MigrationFailed`; see `crate::db`).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StudioStorageError {
    #[error("database error: {0}")]
    Db(#[from] redb::DatabaseError),
    #[error("database transaction error: {0}")]
    Transaction(#[from] redb::TransactionError),
    #[error("database table error: {0}")]
    Table(#[from] redb::TableError),
    #[error("database storage error: {0}")]
    Storage(#[from] redb::StorageError),
    #[error("database commit error: {0}")]
    Commit(#[from] redb::CommitError),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// The on-disk `studio.redb` reports a schema version newer than this
    /// build understands. Refuse to open rather than silently downgrading
    /// or truncating unfamiliar tables — see `docs/architecture/studio-storage.md`
    /// §"Schema versioning".
    #[error(
        "studio.redb schema version {found} is newer than this build supports \
         (max {supported}). Refusing to open — upgrade Valori Studio to open \
         this database. The file was not modified."
    )]
    UnsupportedSchemaVersion { found: u32, supported: u32 },

    /// A migration step failed partway through. The write transaction that
    /// carried it is dropped without committing (redb transactions are
    /// atomic — see `crate::db::run_migrations`), so the database is left at
    /// its pre-migration version, never a half-migrated state.
    #[error("migration from schema v{from} to v{to} failed: {reason}")]
    MigrationFailed { from: u32, to: u32, reason: String },
}

pub type StudioStorageResult<T> = Result<T, StudioStorageError>;
