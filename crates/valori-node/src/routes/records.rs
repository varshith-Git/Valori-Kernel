// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Record deletion — shared bodies for `POST /v1/delete` and
//! `POST /v1/soft-delete`.
//!
//! Canonical behavior (both paths, enforced here):
//! * Unknown `collection` → 404. (Standalone previously returned 400.)
//! * `POST /v1/soft-delete` exists on BOTH paths. (Previously cluster-only;
//!   the standalone engine has had `soft_delete_record` all along.)
//! * Responses carry `log_index` on the cluster path only.
//! * Both paths emit a Delete receipt through `receipt_bridge`.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use std::sync::Arc;

use crate::api::{DeleteRecordRequest, DeleteRecordResponse};

/// Outcome of a committed delete, with everything the shared handler needs
/// to emit the receipt.
pub struct DeletedRecord {
    pub log_index: Option<u64>,
    pub shard_id: u8,
    /// True on the cluster path — recorded in the receipt.
    pub cluster: bool,
    pub state_before: String,
    pub state_after: String,
}

#[async_trait::async_trait]
pub trait RecordOps: Send + Sync {
    /// Optional collection name → namespace id (`None` = default).
    async fn resolve_collection(&self, name: Option<&str>) -> Option<u16>;
    /// Commit the (soft) delete.
    async fn delete(&self, ns: u16, id: u32, soft: bool) -> Result<DeletedRecord, Response>;
}

async fn resolve<O: RecordOps>(ops: &O, collection: Option<&str>) -> Result<u16, Response> {
    ops.resolve_collection(collection).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!(
                    "unknown collection '{}' — create it first with POST /v1/namespaces",
                    collection.unwrap_or("default")
                )
            })),
        )
            .into_response()
    })
}

pub async fn delete_record<O: RecordOps>(
    ops: &O,
    receipts: &Arc<valori_effect::ReceiptStore>,
    req: DeleteRecordRequest,
    soft: bool,
) -> Result<Json<DeleteRecordResponse>, Response> {
    let ns = resolve(ops, req.collection.as_deref()).await?;
    let d = ops.delete(ns, req.id, soft).await?;
    {
        use valori_planner::operation::{OperationInputs, OperationKind};
        let inputs = OperationInputs::Delete {
            collection: req.collection.clone().unwrap_or_else(|| "default".into()),
            shard_id: d.shard_id,
            mode: if soft { "soft".into() } else { "hard".into() },
        };
        crate::receipt_bridge::emit_write(
            receipts,
            OperationKind::Delete,
            &inputs,
            ns,
            d.shard_id,
            d.log_index.unwrap_or(0),
            d.cluster,
            d.state_before,
            d.state_after,
        );
    }
    Ok(Json(DeleteRecordResponse { success: true, log_index: d.log_index }))
}
