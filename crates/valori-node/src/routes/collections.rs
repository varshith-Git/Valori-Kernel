// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Collection (namespace) endpoints — shared bodies for
//! `POST /v1/namespaces`, `GET /v1/namespaces`, `DELETE /v1/namespaces/:name`.
//!
//! Canonical behavior (both paths, enforced here):
//! * create: 400 for empty / >64 chars / non-`[a-zA-Z0-9_-]` names (M-2);
//!   400 if `dimension`/`metric` are missing — no exceptions, "default"
//!   included (Phase 3.3: "default" has no special architectural meaning;
//!   a collection literally named "default" is configured exactly like any
//!   other name); otherwise 200 with the committed id and a `created` flag.
//! * list: 200 with every explicitly-created collection. A brand-new
//!   project lists zero collections — none is auto-created.
//! * drop: 404 for unknown names, 204 on success. No name is undroppable.
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

/// Parsed, validated per-collection vector configuration — the shared
/// handler's output, and what `CollectionOps::create` commits.
///
/// # Always required — no name-based exception (Phase 3.3)
///
/// Every collection, "default" included, must supply `dimension` and
/// `metric` explicitly at creation. `parse_collection_config` rejects a
/// create request that omits either with a `400`, regardless of name.
/// There is no "inherits a project-wide fallback" and no zero-config name
/// any more — a brand-new Project starts with zero collections, and every
/// collection a user creates carries its own explicit config from the
/// start. `index` remains the one optional field (defaults to no dedicated
/// ANN structure).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollectionConfigRequest {
    pub dim: u32,
    pub metric: valori_domain::Metric,
    pub index: valori_domain::IndexKind,
}

/// The state-touching primitives each path provides. Everything else —
/// validation, special cases, response shaping — lives in the shared
/// handlers below and is written exactly once.
#[async_trait::async_trait]
pub trait CollectionOps: Send + Sync {
    /// Resolve an existing collection name to its namespace id.
    async fn resolve(&self, name: &str) -> Option<u16>;
    /// Commit the creation (idempotent). The name is already validated and
    /// `config` is always present — `parse_collection_config` guarantees
    /// this for every name, "default" included (Phase 3.3).
    async fn create(
        &self,
        name: &str,
        config: CollectionConfigRequest,
    ) -> Result<CreatedCollection, Response>;
    /// Commit the drop. The shared handler has already 404'd unknown names.
    async fn drop_collection(&self, name: &str) -> Result<(), Response>;
    /// All explicitly-created collections, as `(name, id)`. Empty for a
    /// brand-new project.
    async fn list(&self) -> Vec<(String, u16)>;
    /// The explicit vector config for `namespace_id`, if it has one.
    async fn config(&self, namespace_id: u16) -> Option<CollectionConfigRequest>;
    async fn record_count(&self, namespace_id: u16) -> usize;
    async fn max_records(&self) -> usize;
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
        return Err(bad_request(
            "collection name must be 64 characters or fewer",
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(bad_request(
            "collection name may only contain [a-zA-Z0-9_-]",
        ));
    }
    let config = parse_collection_config(&name, &payload)?;
    let outcome = ops.create(&name, config).await?;
    Ok(Json(CreateCollectionResponse {
        name,
        id: outcome.id,
        created: !outcome.already_existed,
    }))
}

/// Validate and parse `dimension`/`metric`/`index` into a
/// `CollectionConfigRequest` — **required** for every name, no exceptions
/// (Phase 3.3: "default" included — see the module doc's "final product
/// contract").
///
/// - `dimension`: required. Missing → 400. Zero or over `MAX_DIM` → 400.
/// - `metric`: required. Missing → 400 (no silent default — Phase 3.2 §11:
///   "the actual request sent over the wire should contain the metric
///   explicitly"). Unparseable → 400. Only `"squared_l2"` exists today.
/// - `index`: optional. Missing → `IndexKind::Brute`, which the runtime
///   treats as "no dedicated ANN structure, exact namespace-specific
///   kernel search" (`index = NONE` in product terms) — not a
///   `BruteForceIndex` object; see `Engine::ensure_collection_index`'s own
///   doc comment for why brute-force collections get no dedicated index at
///   all. Unparseable → 400.
fn parse_collection_config(
    name: &str,
    payload: &CreateCollectionRequest,
) -> Result<CollectionConfigRequest, Response> {
    let dim = payload.dimension.ok_or_else(|| {
        bad_request(format!(
            "collection '{name}' must be created with an explicit 'dimension' — Valori no longer \
             infers a collection's dimension from its first insert or from any project-level default."
        ))
    })?;
    if dim == 0 || dim as usize > valori_kernel::config::MAX_DIM {
        return Err(bad_request(format!(
            "dimension must be between 1 and {}",
            valori_kernel::config::MAX_DIM
        )));
    }
    let metric_str = payload.metric.as_deref().ok_or_else(|| {
        bad_request(format!(
            "collection '{name}' must be created with an explicit 'metric' (e.g. \"squared_l2\") — \
             no metric is assumed or inherited."
        ))
    })?;
    let metric = metric_str
        .parse::<valori_domain::Metric>()
        .map_err(|e| bad_request(e.to_string()))?;
    let index = match &payload.index {
        Some(s) => s
            .parse::<valori_domain::IndexKind>()
            .map_err(|e| bad_request(e.to_string()))?,
        // Omitted index = index NONE — exact namespace-specific search, no
        // dedicated ANN structure. `IndexKind::Brute` is the wire tag for
        // that state, not a request for a BruteForceIndex object.
        None => valori_domain::IndexKind::Brute,
    };
    Ok(CollectionConfigRequest { dim, metric, index })
}

pub async fn list_collections<O: CollectionOps>(ops: &O) -> Json<ListCollectionsResponse> {
    let mut collections = Vec::new();
    let max_records = ops.max_records().await;
    for (name, id) in ops.list().await {
        let cfg = ops.config(id).await;
        let record_count = ops.record_count(id).await;
        collections.push(CollectionInfo {
            name,
            id,
            dimension: cfg.map(|c| c.dim),
            metric: cfg.map(|c| c.metric.to_string()),
            index: cfg.map(|c| c.index.to_string()),
            record_count: Some(record_count),
            max_records: Some(max_records),
        });
    }
    Json(ListCollectionsResponse { collections })
}

pub async fn drop_collection<O: CollectionOps>(
    ops: &O,
    name: &str,
) -> Result<StatusCode, Response> {
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
