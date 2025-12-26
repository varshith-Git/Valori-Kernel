use thiserror::Error;

#[derive(Error, Debug)]
pub enum KernelError {
    #[error("Invalid command: {0}")]
    InvalidCommand(u8),

    #[error("Dimension mismatch: expected {expected}, found {found}")]
    DimensionMismatch { expected: usize, found: usize },

    #[error("Invalid payload length: expected {expected}, found {found}")]
    InvalidPayloadLength { expected: usize, found: usize },

    #[error("IO Error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Distance calculation overflow")]
    DistanceOverflow,

    #[error("Query value out of Q16.16 range: {0}")]
    QueryOutOfRange(i32),
}

pub type Result<T> = std::result::Result<T, KernelError>;
