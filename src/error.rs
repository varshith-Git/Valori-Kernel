//! Error types.

#[derive(Debug)]
pub enum KernelError {
    /// Generic overflow error for arithmetic operations.
    Overflow,
    /// Storage is full.
    CapacityExceeded,
    /// Item not found.
    NotFound,
    /// Invalid operation.
    InvalidOperation,
}

pub type KernelResult<T> = core::result::Result<T, KernelError>;
pub type Result<T> = KernelResult<T>; // Keep Result for backward compat within crate, or deprecate? User asked for KernelResult.

