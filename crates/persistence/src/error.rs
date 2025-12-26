use thiserror::Error;
use std::io;

#[derive(Error, Debug)]
pub enum PersistenceError {
    #[error("Invalid magic bytes in header")]
    InvalidMagic,
    #[error("Checksum mismatch: expected {expected}, found {found}")]
    ChecksumMismatch {
        expected: u64,
        found: u64,
    },
    #[error("IO error: {0}")]
    IoError(#[from] io::Error),
    #[error("Invalid data format: {0}")]
    InvalidFormat(String),
}

pub type Result<T> = std::result::Result<T, PersistenceError>;
