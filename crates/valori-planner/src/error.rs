// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlannerError {
    #[error("Capability unavailable: {0}")]
    CapabilityUnavailable(&'static str),
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
    #[error("Planning failed: {0}")]
    PlanningFailed(String),
    #[error("Metadata error: {0}")]
    Metadata(#[from] valori_metadata::MetadataError),
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type PlannerResult<T> = Result<T, PlannerError>;
