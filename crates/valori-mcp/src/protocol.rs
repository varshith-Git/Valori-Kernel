// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! JSON-RPC 2.0 envelope types — the wire format MCP rides on.
//!
//! This module is deliberately transport-agnostic and free of any Valori or
//! HTTP concern, so the framing logic can be unit-tested in isolation.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 standard error codes (plus the MCP convention that server
/// errors live in the -32000..-32099 implementation-defined band).
pub mod codes {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
    /// Backend (Valori node) unreachable or returned an error.
    pub const BACKEND_ERROR: i64 = -32000;
}

/// An incoming JSON-RPC request or notification.
///
/// A *notification* is a request with no `id` — the spec forbids replying to
/// it. We model that by making `id` optional and checking it before sending.
#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    #[allow(dead_code)]
    pub jsonrpc: String,
    /// Absent for notifications (e.g. `notifications/initialized`).
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl Request {
    /// True when this is a notification (no `id`) and therefore must not be
    /// answered.
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorObject {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// A JSON-RPC 2.0 response. Exactly one of `result`/`error` is `Some`.
#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

impl Response {
    pub fn success(id: Value, result: Value) -> Self {
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }

    pub fn error(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(ErrorObject { code, message: message.into(), data: None }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_request_with_id() {
        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
        let req: Request = serde_json::from_str(raw).unwrap();
        assert_eq!(req.method, "tools/list");
        assert!(!req.is_notification());
        assert_eq!(req.id, Some(Value::from(1)));
    }

    #[test]
    fn request_without_id_is_a_notification() {
        let raw = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let req: Request = serde_json::from_str(raw).unwrap();
        assert!(req.is_notification());
    }

    #[test]
    fn success_response_omits_error_field() {
        let r = Response::success(Value::from(7), serde_json::json!({"ok": true}));
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"result\""));
        assert!(!s.contains("\"error\""));
        assert!(s.contains("\"id\":7"));
    }

    #[test]
    fn error_response_omits_result_field() {
        let r = Response::error(Value::from(7), codes::METHOD_NOT_FOUND, "no such method");
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"error\""));
        assert!(!s.contains("\"result\""));
        assert!(s.contains("-32601"));
    }
}
