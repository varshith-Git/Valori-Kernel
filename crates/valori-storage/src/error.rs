// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Kernel error: {0:?}")]
    Kernel(valori_kernel::error::KernelError),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type StorageResult<T> = Result<T, StorageError>;
