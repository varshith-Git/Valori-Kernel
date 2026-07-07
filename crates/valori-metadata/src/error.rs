// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("Database error: {0}")]
    Db(#[from] redb::DatabaseError),
    #[error("Database transaction error: {0}")]
    Transaction(#[from] redb::TransactionError),
    #[error("Database table error: {0}")]
    Table(#[from] redb::TableError),
    #[error("Database storage error: {0}")]
    Storage(#[from] redb::StorageError),
    #[error("Database commit error: {0}")]
    Commit(#[from] redb::CommitError),
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

pub type MetadataResult<T> = Result<T, MetadataError>;
