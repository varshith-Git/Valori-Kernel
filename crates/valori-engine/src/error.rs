// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Error types for the engine and persistence layers.
//!
//! [`CommitError`] is the persistence-layer error: every durability variant
//! (capacity exceeded, kernel rejection, I/O failure) maps to one variant.
//! [`EngineError`] is the engine-layer error: wraps kernel errors and adds
//! HTTP-facing context; implements `IntoResponse` so axum handlers can use `?`.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use thiserror::Error;

// ── CommitError ───────────────────────────────────────────────────────────────

/// All errors that can occur during a durability commit.
///
/// Returned by [`super::Persistence`] methods and by the `Committer` trait
/// implementations in `valori-node`.
#[derive(Debug, Error)]
pub enum CommitError {
    #[error("capacity exceeded: {pool} pool is full ({used}/{cap})")]
    Capacity {
        pool: &'static str,
        used: usize,
        cap: usize,
    },

    #[error("shadow application rejected event: {0:?}")]
    Apply(valori_kernel::error::KernelError),

    #[error("persistence layer error: {0}")]
    Io(String),

    #[error("batch was empty — nothing to commit")]
    EmptyBatch,

    /// The replicated state machine deterministically rejected the event.
    /// Every node rejected identically; state is untouched.
    #[error("event rejected by the replicated state machine: {0}")]
    Rejected(String),

    /// This node is a Raft follower. The HTTP layer should answer 307 with
    /// the leader's API address.
    #[error("not the leader{}", leader_api_addr.as_deref().map(|a| format!(" — leader API at {a}")).unwrap_or_default())]
    NotLeader { leader_api_addr: Option<String> },
}

// ── EngineError ───────────────────────────────────────────────────────────────

/// Engine-layer error, returned by all `Engine` methods.
///
/// Implements [`IntoResponse`] so axum handlers can propagate engine errors
/// directly with `?`.
#[derive(Error, Debug)]
pub enum EngineError {
    #[error("Kernel error: {0:?}")]
    Kernel(valori_kernel::error::KernelError),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Internal server error")]
    Internal,
    #[error("Network error: {0}")]
    Network(String),
    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl IntoResponse for EngineError {
    fn into_response(self) -> Response {
        use valori_kernel::error::KernelError;
        let (status, message) = match self {
            EngineError::Kernel(k_err) => match k_err {
                KernelError::NotFound => (
                    StatusCode::NOT_FOUND,
                    "Record, node, or edge not found".to_string(),
                ),
                KernelError::CapacityExceeded => (
                    StatusCode::INSUFFICIENT_STORAGE,
                    "Record pool is full — increase VALORI_MAX_RECORDS and restart".to_string(),
                ),
                KernelError::DimensionMismatch { expected, found } => (
                    StatusCode::BAD_REQUEST,
                    format!(
                        "Dimension mismatch: node expects {expected}-element vectors, got {found}. \
                         Check GET /health for the locked dimension, or set VALORI_DIM={expected}."
                    ),
                ),
                KernelError::InvalidOperation => (
                    StatusCode::BAD_REQUEST,
                    "Invalid operation: record ID out of sequence or duplicate insert.".to_string(),
                ),
                KernelError::InvalidInput => (
                    StatusCode::BAD_REQUEST,
                    "Invalid input: vector values are out of the Q16.16 fixed-point range.".to_string(),
                ),
                KernelError::MetadataTooLarge => (
                    StatusCode::BAD_REQUEST,
                    "Metadata too large (max 4 KB per record)".to_string(),
                ),
                KernelError::QueryOutOfRange(v) => (
                    StatusCode::BAD_REQUEST,
                    format!(
                        "Query vector value {v} is out of the Q16.16 fixed-point range \
                         (−32768.0 to +32767.9999847412)."
                    ),
                ),
                KernelError::InvalidPayloadLength { expected, found } => (
                    StatusCode::BAD_REQUEST,
                    format!("Payload length mismatch: expected {expected} bytes, got {found}."),
                ),
                KernelError::InvalidCommand(code) => (
                    StatusCode::BAD_REQUEST,
                    format!("Unknown kernel command code {code:#04x}."),
                ),
                KernelError::Overflow => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Numeric overflow in Q16.16 arithmetic".to_string(),
                ),
                KernelError::DistanceOverflow => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Distance computation overflowed Q16.16 range".to_string(),
                ),
                KernelError::NotImplemented => (
                    StatusCode::NOT_IMPLEMENTED,
                    "This operation is not implemented in the current kernel version".to_string(),
                ),
                KernelError::IoError(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Kernel I/O error: {e}"),
                ),
            },
            EngineError::InvalidInput(msg) => (StatusCode::BAD_REQUEST, msg),
            EngineError::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string()),
            EngineError::Network(msg) => (StatusCode::BAD_GATEWAY, format!("Upstream error: {}", msg)),
            EngineError::Unknown(msg) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Unknown error: {}", msg)),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

impl From<valori_kernel::error::KernelError> for EngineError {
    fn from(e: valori_kernel::error::KernelError) -> Self { EngineError::Kernel(e) }
}

impl From<super::CommitError> for EngineError {
    fn from(e: super::CommitError) -> Self {
        use valori_kernel::error::KernelError;
        match e {
            // Kernel rejected the event (e.g. capacity, dimension mismatch) — preserve
            // the full KernelError so IntoResponse returns the correct HTTP status code.
            super::CommitError::Apply(k) => EngineError::Kernel(k),
            // Pool full — map to the same KernelError the kernel would surface directly.
            super::CommitError::Capacity { .. } => EngineError::Kernel(KernelError::CapacityExceeded),
            // Persistence/IO failure — internal server error, not the client's fault.
            super::CommitError::Io(_) => EngineError::Internal,
            // Empty batch is a caller bug.
            super::CommitError::EmptyBatch => EngineError::InvalidInput("batch was empty".into()),
            // Raft rejection (e.g. duplicate event) — client can retry with a new ID.
            super::CommitError::Rejected(s) => EngineError::InvalidInput(s),
            // Only reachable in cluster mode; never surfaces in standalone commit_and_apply_ns.
            super::CommitError::NotLeader { .. } => EngineError::Internal,
        }
    }
}

impl From<valori_state::StateError> for EngineError {
    fn from(e: valori_state::StateError) -> Self {
        match e {
            valori_state::StateError::Kernel(k) => EngineError::Kernel(k),
            valori_state::StateError::InvalidInput(s) => EngineError::InvalidInput(s),
            valori_state::StateError::Io(io) => EngineError::InvalidInput(io.to_string()),
        }
    }
}

impl From<valori_storage::StorageError> for EngineError {
    fn from(e: valori_storage::StorageError) -> Self {
        match e {
            valori_storage::StorageError::Kernel(k) => EngineError::Kernel(k),
            valori_storage::StorageError::InvalidInput(s) => EngineError::InvalidInput(s),
            valori_storage::StorageError::Io(io) => EngineError::InvalidInput(io.to_string()),
        }
    }
}
