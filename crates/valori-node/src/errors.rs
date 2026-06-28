// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use thiserror::Error;
use valori_kernel::error::KernelError;

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
                    "Invalid operation: record ID out of sequence or duplicate insert. \
                     Each record ID must equal the current record count at insert time."
                    .to_string(),
                ),
                KernelError::InvalidInput => (
                    StatusCode::BAD_REQUEST,
                    "Invalid input: vector values are out of the Q16.16 fixed-point range \
                     (−32768.0 to +32767.9999847412)."
                    .to_string(),
                ),
                KernelError::MetadataTooLarge => (
                    StatusCode::BAD_REQUEST,
                    "Metadata too large (max 4 KB per record)".to_string(),
                ),
                KernelError::QueryOutOfRange(v) => (
                    StatusCode::BAD_REQUEST,
                    format!(
                        "Query vector value {v} is out of the Q16.16 fixed-point range \
                         (−32768.0 to +32767.9999847412). Normalise the query vector before sending."
                    ),
                ),
                KernelError::InvalidPayloadLength { expected, found } => (
                    StatusCode::BAD_REQUEST,
                    format!(
                        "Payload length mismatch: expected {expected} bytes, got {found}. \
                         The record may be corrupt."
                    ),
                ),
                KernelError::InvalidCommand(code) => (
                    StatusCode::BAD_REQUEST,
                    format!("Unknown kernel command code {code:#04x} — client/server version mismatch."),
                ),
                KernelError::Overflow => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Numeric overflow in Q16.16 arithmetic — vector values are too large".to_string(),
                ),
                KernelError::DistanceOverflow => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Distance computation overflowed Q16.16 range — vectors are too dissimilar \
                     or contain extreme values."
                    .to_string(),
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

        let body = Json(json!({
            "error": message
        }));

        (status, body).into_response()
    }
}

impl From<valori_kernel::error::KernelError> for EngineError {
    fn from(e: valori_kernel::error::KernelError) -> Self {
        EngineError::Kernel(e)
    }
}
