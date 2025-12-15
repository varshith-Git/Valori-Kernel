// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
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
}

impl IntoResponse for EngineError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            EngineError::Kernel(k_err) => match k_err {
                KernelError::NotFound => (StatusCode::NOT_FOUND, "Resource not found".to_string()),
                KernelError::CapacityExceeded => (StatusCode::INSUFFICIENT_STORAGE, "Capacity exceeded".to_string()),
                KernelError::InvalidOperation => (StatusCode::BAD_REQUEST, "Invalid operation".to_string()),
                KernelError::Overflow => (StatusCode::INTERNAL_SERVER_ERROR, "Numeric overflow".to_string()),
            },
            EngineError::InvalidInput(msg) => (StatusCode::BAD_REQUEST, msg),
            EngineError::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string()),
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
