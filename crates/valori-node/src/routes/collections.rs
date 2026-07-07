// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Collection (namespace) endpoints — shared bodies for
//! `POST /v1/namespaces`, `GET /v1/namespaces`, `DELETE /v1/namespaces/:name`.
//!
//! Canonical behavior (both paths, enforced here):
//! * create: 400 for empty / >64 chars / non-`[a-zA-Z0-9_-]` names (M-2);
//!   "default" is an idempotent no-op returning `{id: 0, created: false}`;
//!   otherwise 200 with the committed id and a `created` flag.
//! * list: 200 with every collection incl. "default".
//! * drop: 400 for "default", 404 for unknown names, 204 on success.
//!
//! Unification note: before this module the cluster path skipped the M-2
//! name validation entirely, and the standalone path returned 400 (not 404)
//! for dropping an unknown collection. Both were silent divergences.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::api::{
    CollectionInfo, CreateCollectionRequest, CreateCollectionResponse, ListCollectionsResponse,
};
use crate::errors::EngineError;

/// Outcome of a committed create: the namespace id, plus whether the name
/// already existed. `already_existed` may be computed best-effort on the
/// cluster path (a concurrent create can race the pre-check) — cosmetic only,
/// the id always comes from the committed write.
pub struct CreatedCollection {
    pub id: u16,
    pub already_existed: bool,
}

/// The state-touching primitives each path provides. Everything else —
/// validation, special cases, response shaping — lives in the shared
/// handlers below and is written exactly once.
#[async_trait::async_trait]
pub trait CollectionOps: Send + Sync {
    /// Resolve an existing collection name to its namespace id.
    async fn resolve(&self, name: &str) -> Option<u16>;
    /// Commit the creation (idempotent). The name is already validated.
    async fn create(&self, name: &str) -> Result<CreatedCollection, Response>;
    /// Commit the drop. The shared handler has already rejected "default"
    /// and 404'd unknown names.
    async fn drop_collection(&self, name: &str) -> Result<(), Response>;
    /// All collections incl. "default", as `(name, id)`.
    async fn list(&self) -> Vec<(String, u16)>;
}

fn bad_request(msg: impl Into<String>) -> Response {
    EngineError::InvalidInput(msg.into()).into_response()
}

pub async fn create_collection<O: CollectionOps>(
    ops: &O,
    payload: CreateCollectionRequest,
) -> Result<Json<CreateCollectionResponse>, Response> {
    let name = payload.name.trim().to_string();
    // M-2: restrict to safe identifier characters to prevent path/injection issues.
    if name.is_empty() {
        return Err(bad_request("collection name cannot be empty"));
    }
    if name.len() > 64 {
        return Err(bad_request("collection name must be 64 characters or fewer"));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(bad_request("collection name may only contain [a-zA-Z0-9_-]"));
    }
    if name == "default" {
        // Idempotent no-op — "default" always exists as id 0.
        return Ok(Json(CreateCollectionResponse { name, id: 0, created: false }));
    }
    let outcome = ops.create(&name).await?;
    Ok(Json(CreateCollectionResponse {
        name,
        id: outcome.id,
        created: !outcome.already_existed,
    }))
}

pub async fn list_collections<O: CollectionOps>(ops: &O) -> Json<ListCollectionsResponse> {
    let collections = ops
        .list()
        .await
        .into_iter()
        .map(|(name, id)| CollectionInfo { name, id })
        .collect();
    Json(ListCollectionsResponse { collections })
}

pub async fn drop_collection<O: CollectionOps>(
    ops: &O,
    name: &str,
) -> Result<StatusCode, Response> {
    if name == "default" {
        return Err(bad_request("the 'default' collection cannot be dropped"));
    }
    if ops.resolve(name).await.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("collection '{name}' not found") })),
        )
            .into_response());
    }
    ops.drop_collection(name).await?;
    Ok(StatusCode::NO_CONTENT)
}
