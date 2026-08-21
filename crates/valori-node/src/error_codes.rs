// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase API-2 — the last mile of the error contract.
//!
//! [`EngineError`](crate::errors::EngineError) already emits the canonical
//! `{"error": …, "code": …}` body for everything that flows through `?`. But
//! both routers also build error responses by hand — `(StatusCode::NOT_FOUND,
//! Json(json!({"error": …})))` appears ~100 times across `server.rs` and
//! `cluster_server.rs` — and the auth guards used to answer 401/403 with **no
//! body at all**, which no client can parse.
//!
//! Hand-editing every one of those call sites would leave the same class of
//! bug open for the next one somebody writes. Instead, [`attach_error_code`]
//! is a response middleware on **both** routers that guarantees the invariant
//! structurally:
//!
//! * status < 400 and not 307 → untouched.
//! * JSON body that already carries `code` → untouched (a handler's more
//!   specific code, e.g. `collection_not_found` or `dimension_mismatch`, always
//!   wins over the status-derived default).
//! * JSON object without `code` → `code` inserted, derived from the status.
//! * empty body → a full `{"error", "code"}` object synthesised.
//! * anything else (non-JSON content type, non-object JSON) → untouched, so
//!   `text/plain` responses such as `GET /v1/version` and Prometheus
//!   `GET /metrics` are never rewritten.
//!
//! The status → code mapping is deliberately coarse; it is the *floor*, not
//! the ceiling. A handler that knows better should say so by emitting its own
//! `code` (via [`crate::errors::error_response`]), which this pass preserves.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::errors::ErrorCode;

/// Largest error body this middleware will buffer. Error bodies are small;
/// anything larger is passed through untouched rather than risk buffering a
/// streaming response.
const MAX_ERROR_BODY: usize = 64 * 1024;

/// The code every response with this status carries unless the handler
/// supplied a more specific one.
fn default_code_for(status: StatusCode) -> Option<ErrorCode> {
    Some(match status {
        StatusCode::TEMPORARY_REDIRECT => ErrorCode::NotLeader,
        StatusCode::UNAUTHORIZED => ErrorCode::Unauthorized,
        StatusCode::FORBIDDEN => ErrorCode::Forbidden,
        StatusCode::NOT_FOUND => ErrorCode::NotFound,
        StatusCode::CONFLICT => ErrorCode::Conflict,
        StatusCode::INSUFFICIENT_STORAGE => ErrorCode::CapacityExceeded,
        StatusCode::NOT_IMPLEMENTED => ErrorCode::NotImplemented,
        StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT => {
            ErrorCode::Unavailable
        }
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => ErrorCode::ValidationError,
        s if s.is_server_error() => ErrorCode::InternalError,
        s if s.is_client_error() => ErrorCode::ValidationError,
        _ => return None,
    })
}

/// Human-readable fallback for a response that carried no body at all.
fn default_message_for(status: StatusCode) -> &'static str {
    match status {
        StatusCode::UNAUTHORIZED => {
            "missing or invalid credentials — send Authorization: Bearer <token>"
        }
        StatusCode::FORBIDDEN => "the supplied credentials do not have the required scope",
        _ => status.canonical_reason().unwrap_or("request failed"),
    }
}

/// Paths whose `>= 400` responses are **status reports, not errors**.
///
/// Phase API-3.3 found this by auditing the contract against the runtime.
/// `GET /health` answers `503` with a complete [`crate::api::HealthResponse`]
/// when a pool is at 100 %: the status code is a signal to the load balancer,
/// not a failure to describe. `GET /v1/cluster/health` does the same with
/// `ClusterHealthResponse`.
///
/// Without this exemption the pass below would see "503, JSON object, no
/// `code`" and splice `error` and `code` into a documented DTO — so the bytes
/// on the wire would not match the schema the contract advertises, and a
/// generated SDK deserialising strictly would reject its own health probe.
///
/// These are the only two operations in the public surface whose `>= 400`
/// response is a typed non-`ApiError` document; `scripts/audit-public-api-operations.py`
/// reports any third one as a finding rather than letting it pass silently.
const STATUS_REPORT_PATHS: &[&str] = &["/health", "/v1/cluster/health"];

pub async fn attach_error_code(req: Request, next: Next) -> Response {
    let is_status_report = STATUS_REPORT_PATHS.contains(&req.uri().path());
    let resp = next.run(req).await;
    let status = resp.status();

    if is_status_report {
        return resp;
    }

    let Some(code) = default_code_for(status) else {
        return resp;
    };

    let is_json = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.starts_with("application/json"))
        .unwrap_or(false);

    let (mut parts, body) = resp.into_parts();

    // Non-JSON with a body: leave it alone. An absent content-type with an
    // empty body is the bare-status 401/403 case, which we do fill in.
    let bytes = match axum::body::to_bytes(body, MAX_ERROR_BODY).await {
        Ok(b) => b,
        // Too large or unreadable — we already consumed the body and cannot
        // put it back, so answer with the canonical error instead of a
        // truncated one. Only reachable for a >64 KB error body.
        Err(_) => {
            parts.headers.remove(header::CONTENT_LENGTH);
            return (
                parts,
                axum::Json(crate::errors::error_body(code, default_message_for(status))),
            )
                .into_response();
        }
    };

    if !bytes.is_empty() && !is_json {
        return Response::from_parts(parts, Body::from(bytes));
    }

    let mut value: serde_json::Value = if bytes.is_empty() {
        serde_json::json!({ "error": default_message_for(status) })
    } else {
        match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            // Declared JSON but isn't — pass through rather than corrupt it.
            Err(_) => return Response::from_parts(parts, Body::from(bytes)),
        }
    };

    let Some(obj) = value.as_object_mut() else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    if obj.contains_key("code") {
        return Response::from_parts(parts, Body::from(bytes));
    }
    obj.entry("error")
        .or_insert_with(|| serde_json::Value::String(default_message_for(status).into()));
    obj.insert(
        "code".into(),
        serde_json::Value::String(code.as_str().into()),
    );

    // Content-Length no longer matches; axum recomputes it for a sized body.
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    (parts, axum::Json(value)).into_response()
}

/// The one canonical "this Collection does not exist" response.
///
/// Phase API-2: six handlers used to build this by hand with three different
/// messages and two different status codes (400 standalone / 404 cluster).
/// `None` means the caller omitted `collection` entirely — which is an error
/// in its own right, since there is no implicit default Collection.
pub fn collection_not_found(name: Option<&str>) -> Response {
    let message = match name {
        Some(n) => format!("unknown collection '{n}' — create it first with POST /v1/namespaces"),
        None => "no collection specified — `collection` is required on this request; \
                 there is no implicit default collection (create one with \
                 POST /v1/namespaces and name it explicitly)"
            .to_string(),
    };
    crate::errors::error_response(
        StatusCode::NOT_FOUND,
        ErrorCode::CollectionNotFound,
        message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_client_and_server_status_maps_to_a_code() {
        for s in [
            400u16, 401, 403, 404, 409, 422, 500, 501, 502, 503, 507, 307,
        ] {
            let st = StatusCode::from_u16(s).unwrap();
            assert!(default_code_for(st).is_some(), "no code for {s}");
        }
    }

    #[test]
    fn success_statuses_are_left_alone() {
        for s in [200u16, 201, 202, 204] {
            assert!(default_code_for(StatusCode::from_u16(s).unwrap()).is_none());
        }
    }
}
