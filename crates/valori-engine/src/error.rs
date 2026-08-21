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

// ── ErrorCode ─────────────────────────────────────────────────────────────────

/// Stable, machine-readable error taxonomy for the public HTTP API.
///
/// Phase API-2 promoted this from a reserved contract enum to a field the
/// server actually emits. Every canonical error body carries
/// `{"error": "<human message>", "code": "<ErrorCode>"}`. The `error` string
/// stays for backward compatibility with clients written before codes
/// existed; `code` is the field new clients must branch on.
///
/// The value set is closed and mirrors `components.schemas.ErrorCode` in
/// `api/openapi/valori-v1.yaml` exactly. Adding a variant is a non-breaking
/// change; renaming or removing one is breaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// Malformed or semantically invalid request input.
    ValidationError,
    /// Credentials missing or unparseable.
    Unauthorized,
    /// Credentials valid but the key's scope is insufficient.
    Forbidden,
    /// Generic "the addressed thing does not exist".
    NotFound,
    /// The named Collection is not registered.
    CollectionNotFound,
    /// The record id does not exist in the addressed Collection.
    RecordNotFound,
    /// Vector length disagrees with the Collection's fixed dimension.
    DimensionMismatch,
    /// Unrecognised or unsupported distance metric.
    InvalidMetric,
    /// Unrecognised or unsupported index kind/type.
    InvalidIndex,
    /// An asynchronous index build failed.
    IndexBuildFailed,
    /// The request conflicts with existing immutable state.
    Conflict,
    /// A fixed-capacity pool (records/nodes/edges) is full.
    CapacityExceeded,
    /// Cluster mode: this node is not the Raft leader.
    NotLeader,
    /// The node cannot serve the request right now.
    Unavailable,
    /// The capability is not built into / enabled on this deployment.
    NotImplemented,
    /// Unexpected server-side failure.
    InternalError,
}

impl ErrorCode {
    /// The exact wire string emitted in the `code` field.
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCode::ValidationError => "validation_error",
            ErrorCode::Unauthorized => "unauthorized",
            ErrorCode::Forbidden => "forbidden",
            ErrorCode::NotFound => "not_found",
            ErrorCode::CollectionNotFound => "collection_not_found",
            ErrorCode::RecordNotFound => "record_not_found",
            ErrorCode::DimensionMismatch => "dimension_mismatch",
            ErrorCode::InvalidMetric => "invalid_metric",
            ErrorCode::InvalidIndex => "invalid_index",
            ErrorCode::IndexBuildFailed => "index_build_failed",
            ErrorCode::Conflict => "conflict",
            ErrorCode::CapacityExceeded => "capacity_exceeded",
            ErrorCode::NotLeader => "not_leader",
            ErrorCode::Unavailable => "unavailable",
            ErrorCode::NotImplemented => "not_implemented",
            ErrorCode::InternalError => "internal_error",
        }
    }

    /// Every variant, in contract order. Used by the conformance test that
    /// diffs this enum against the OpenAPI `ErrorCode` enum.
    pub const ALL: [ErrorCode; 16] = [
        ErrorCode::ValidationError,
        ErrorCode::Unauthorized,
        ErrorCode::Forbidden,
        ErrorCode::NotFound,
        ErrorCode::CollectionNotFound,
        ErrorCode::RecordNotFound,
        ErrorCode::DimensionMismatch,
        ErrorCode::InvalidMetric,
        ErrorCode::InvalidIndex,
        ErrorCode::IndexBuildFailed,
        ErrorCode::Conflict,
        ErrorCode::CapacityExceeded,
        ErrorCode::NotLeader,
        ErrorCode::Unavailable,
        ErrorCode::NotImplemented,
        ErrorCode::InternalError,
    ];
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl serde::Serialize for ErrorCode {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

/// Build the one canonical error body: `{"error": …, "code": …}`.
///
/// Every hand-rolled `json!({"error": …})` site in the HTTP layer should call
/// this instead so no response can carry a message without a code.
pub fn error_body(code: ErrorCode, message: impl Into<String>) -> serde_json::Value {
    json!({ "error": message.into(), "code": code.as_str() })
}

/// Build a complete axum error response with the canonical body.
pub fn error_response(status: StatusCode, code: ErrorCode, message: impl Into<String>) -> Response {
    (status, Json(error_body(code, message))).into_response()
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
    /// The named Collection is not registered.
    ///
    /// Phase API-2: split out of [`EngineError::InvalidInput`] so that an
    /// unknown Collection is a **404 / `collection_not_found`** on every code
    /// path and in both execution modes. Before this split, standalone
    /// answered 400 while cluster answered 404 for the same request — the
    /// single most-hit status-code fork in the audit.
    #[error("{0}")]
    CollectionNotFound(String),
    /// The request conflicts with existing state that cannot be changed to
    /// satisfy it. Phase API-2: introduced so "collection exists but has no
    /// vector configuration" is a 409 on **both** paths instead of 400
    /// standalone / 500 cluster.
    #[error("{0}")]
    Conflict(String),
    #[error("Internal server error")]
    Internal,
    #[error("Network error: {0}")]
    Network(String),
    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl EngineError {
    /// Decompose into the exact `(HTTP status, machine code, human message)`
    /// triple the wire carries. Kept separate from [`IntoResponse`] so tests
    /// (and the cluster adapter, which builds its own responses) can assert
    /// the mapping without going through axum.
    pub fn parts(self) -> (StatusCode, ErrorCode, String) {
        use valori_kernel::error::KernelError;
        match self {
            EngineError::Kernel(k_err) => match k_err {
                KernelError::NotFound => (
                    StatusCode::NOT_FOUND,
                    ErrorCode::NotFound,
                    "Record, node, or edge not found".to_string(),
                ),
                KernelError::CapacityExceeded => (
                    StatusCode::INSUFFICIENT_STORAGE,
                    ErrorCode::CapacityExceeded,
                    "Record pool is full — increase VALORI_MAX_RECORDS and restart".to_string(),
                ),
                KernelError::DimensionMismatch { expected, found } => (
                    StatusCode::BAD_REQUEST,
                    ErrorCode::DimensionMismatch,
                    format!(
                        "Dimension mismatch: this collection expects {expected}-element vectors, \
                         got {found}. A collection's dimension is fixed when it is created \
                         (POST /v1/namespaces) and cannot be changed — create a new collection \
                         with the correct dimension instead."
                    ),
                ),
                KernelError::InvalidOperation => (
                    StatusCode::BAD_REQUEST,
                    ErrorCode::ValidationError,
                    "Invalid operation: record ID out of sequence or duplicate insert.".to_string(),
                ),
                KernelError::InvalidInput => (
                    StatusCode::BAD_REQUEST,
                    ErrorCode::ValidationError,
                    "Invalid input: vector values are out of the Q16.16 fixed-point range."
                        .to_string(),
                ),
                KernelError::MetadataTooLarge => (
                    StatusCode::BAD_REQUEST,
                    ErrorCode::ValidationError,
                    "Metadata too large (max 4 KB per record)".to_string(),
                ),
                KernelError::QueryOutOfRange(v) => (
                    StatusCode::BAD_REQUEST,
                    ErrorCode::ValidationError,
                    format!(
                        "Query vector value {v} is out of the Q16.16 fixed-point range \
                         (−32768.0 to +32767.9999847412)."
                    ),
                ),
                KernelError::InvalidPayloadLength { expected, found } => (
                    StatusCode::BAD_REQUEST,
                    ErrorCode::ValidationError,
                    format!("Payload length mismatch: expected {expected} bytes, got {found}."),
                ),
                KernelError::InvalidCommand(code) => (
                    StatusCode::BAD_REQUEST,
                    ErrorCode::ValidationError,
                    format!("Unknown kernel command code {code:#04x}."),
                ),
                KernelError::Overflow => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorCode::InternalError,
                    "Numeric overflow in Q16.16 arithmetic".to_string(),
                ),
                KernelError::DistanceOverflow => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorCode::InternalError,
                    "Distance computation overflowed Q16.16 range".to_string(),
                ),
                KernelError::NotImplemented => (
                    StatusCode::NOT_IMPLEMENTED,
                    ErrorCode::NotImplemented,
                    "This operation is not implemented in the current kernel version".to_string(),
                ),
                KernelError::IoError(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorCode::InternalError,
                    format!("Kernel I/O error: {e}"),
                ),
                KernelError::NamespaceAlreadyConfigured {
                    namespace_id,
                    existing_dim,
                } => (
                    StatusCode::CONFLICT,
                    ErrorCode::Conflict,
                    format!(
                        "Collection (namespace {namespace_id}) is already configured with \
                         dimension {existing_dim} — dimension is immutable after creation."
                    ),
                ),
            },
            EngineError::InvalidInput(msg) => {
                (StatusCode::BAD_REQUEST, ErrorCode::ValidationError, msg)
            }
            EngineError::CollectionNotFound(msg) => {
                (StatusCode::NOT_FOUND, ErrorCode::CollectionNotFound, msg)
            }
            EngineError::Conflict(msg) => (StatusCode::CONFLICT, ErrorCode::Conflict, msg),
            EngineError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::InternalError,
                "Internal server error".to_string(),
            ),
            EngineError::Network(msg) => (
                StatusCode::BAD_GATEWAY,
                ErrorCode::Unavailable,
                format!("Upstream error: {}", msg),
            ),
            EngineError::Unknown(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::InternalError,
                format!("Unknown error: {}", msg),
            ),
        }
    }
}

impl IntoResponse for EngineError {
    fn into_response(self) -> Response {
        let (status, code, message) = self.parts();
        error_response(status, code, message)
    }
}

impl From<valori_kernel::error::KernelError> for EngineError {
    fn from(e: valori_kernel::error::KernelError) -> Self {
        EngineError::Kernel(e)
    }
}

impl From<super::CommitError> for EngineError {
    fn from(e: super::CommitError) -> Self {
        use valori_kernel::error::KernelError;
        match e {
            // Kernel rejected the event (e.g. capacity, dimension mismatch) — preserve
            // the full KernelError so IntoResponse returns the correct HTTP status code.
            super::CommitError::Apply(k) => EngineError::Kernel(k),
            // Pool full — map to the same KernelError the kernel would surface directly.
            super::CommitError::Capacity { .. } => {
                EngineError::Kernel(KernelError::CapacityExceeded)
            }
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
