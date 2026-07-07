// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EffectError {
    #[error("Capability unavailable: {0}")]
    CapabilityUnavailable(&'static str),
    #[error("Effect dispatch failed: {0}")]
    Dispatch(String),
    #[error("Task execution failed: {0}")]
    TaskFailed(String),
    #[error("Resource budget exceeded: {0}")]
    BudgetExceeded(&'static str),
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Effect already dispatched (dedup): {0}")]
    Duplicate(String),
    #[error("Capacity limit reached: {0}")]
    Capacity(String),
}

pub type EffectResult<T> = Result<T, EffectError>;
