// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Metadata sidecar — shared bodies for `POST /v1/memory/meta/set` and
//! `GET /v1/memory/meta/get`.
//!
//! Canonical wire format (both paths, enforced here):
//! * set → `{"success": true}`. (The cluster path previously answered
//!   `{"ok": true}` — a silent wire divergence from standalone.)
//! * get → `{"target_id": …, "metadata": …}` with `metadata: null` when unset.

use axum::response::Response;
use axum::Json;

use crate::api::{
    MetadataGetRequest, MetadataGetResponse, MetadataSetRequest, MetadataSetResponse,
};

#[async_trait::async_trait]
pub trait MetaOps: Send + Sync {
    /// Commit the metadata write (audited on both paths: standalone through
    /// `set_meta_audited`, cluster through `KernelEvent::SetMeta` via Raft).
    async fn set_meta(
        &self,
        target_id: String,
        metadata: serde_json::Value,
    ) -> Result<(), Response>;
    async fn get_meta(&self, target_id: &str) -> Option<serde_json::Value>;
}

pub async fn meta_set<O: MetaOps>(
    ops: &O,
    req: MetadataSetRequest,
) -> Result<Json<MetadataSetResponse>, Response> {
    ops.set_meta(req.target_id, req.metadata).await?;
    Ok(Json(MetadataSetResponse { success: true }))
}

pub async fn meta_get<O: MetaOps>(ops: &O, req: MetadataGetRequest) -> Json<MetadataGetResponse> {
    let metadata = ops.get_meta(&req.target_id).await;
    Json(MetadataGetResponse {
        target_id: req.target_id,
        metadata,
    })
}
