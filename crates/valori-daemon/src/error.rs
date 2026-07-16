// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Daemon error type + HTTP mapping.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use thiserror::Error;

pub type DaemonResult<T> = Result<T, DaemonError>;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("project '{0}' not found")]
    NotFound(String),

    #[error("project '{0}' already exists")]
    AlreadyExists(String),

    #[error("project '{0}' is running — stop it first")]
    Running(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("illegal runtime state transition: {from:?} → {to:?}")]
    InvalidState {
        from: crate::runtime::state::RuntimeState,
        to: crate::runtime::state::RuntimeState,
    },

    #[error("could not locate the valori-node binary: {0}")]
    NodeBinaryMissing(String),

    #[error("failed to start node: {0}")]
    StartFailed(String),

    #[error("no free port in the daemon's allocation range")]
    NoFreePort,

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("model error: {0}")]
    Model(#[from] valori_models::ModelError),
}

impl IntoResponse for DaemonError {
    fn into_response(self) -> Response {
        let status = match &self {
            DaemonError::NotFound(_) => StatusCode::NOT_FOUND,
            DaemonError::AlreadyExists(_) | DaemonError::Running(_) => StatusCode::CONFLICT,
            DaemonError::InvalidInput(_) | DaemonError::InvalidState { .. } => {
                StatusCode::BAD_REQUEST
            }
            DaemonError::NoFreePort
            | DaemonError::NodeBinaryMissing(_)
            | DaemonError::StartFailed(_) => StatusCode::SERVICE_UNAVAILABLE,
            DaemonError::Io(_) | DaemonError::Serde(_) | DaemonError::Model(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        (status, Json(json!({ "error": self.to_string() }))).into_response()
    }
}
