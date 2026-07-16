// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StateError {
    #[error("Kernel error: {0:?}")]
    Kernel(valori_kernel::error::KernelError),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type StateResult<T> = Result<T, StateError>;

impl From<valori_storage::StorageError> for StateError {
    fn from(e: valori_storage::StorageError) -> Self {
        match e {
            valori_storage::StorageError::Kernel(k) => StateError::Kernel(k),
            valori_storage::StorageError::InvalidInput(s) => StateError::InvalidInput(s),
            valori_storage::StorageError::Io(io) => StateError::Io(io),
        }
    }
}

impl From<valori_kernel::error::KernelError> for StateError {
    fn from(e: valori_kernel::error::KernelError) -> Self {
        StateError::Kernel(e)
    }
}
