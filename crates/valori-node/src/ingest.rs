#[cfg(feature = "utoipa")]
#[allow(unused_imports)]
use crate::openapi::ApiError;
use crate::server::SharedEngine;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
/// Full ingest pipeline handlers — chunk + embed + insert + graph + metadata.
///
/// Endpoints owned here:
///   POST /v1/ingest              — full pipeline (IngestPipeline → KernelWriter)
///   POST /v1/ingest/update       — diff-based document update (direct embed path)
///   GET  /v1/ingest/status/:id   — async job status
///
/// POST /v1/ingest/document (chunk-only, stateless) lives in valori-ingest::handler
/// and is registered directly in server.rs / cluster_server.rs.
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
// embed_batch and chunk_document are still used by ingest_update.
use crate::execution_registry::{ExecutionRecord, ExecutionRegistry};
use crate::kernel_writer::KernelWriter;
use valori_engine::ErrorCode;
use valori_ingest::{chunk_content_hash, chunk_document, embed_batch};
use valori_ingest::{DefaultChunker, IngestPipeline, ModelProviderEmbedder, TextReader};
use valori_models::provider_from_config;

const MAX_INGEST_TEXT_BYTES: usize = valori_ingest::chunker::MAX_INGEST_TEXT_BYTES;

// ── Request / response types ──────────────────────────────────────────────────

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Deserialize)]
pub struct IngestRequest {
    pub text: String,
    pub collection: Option<String>,
    pub strategy: Option<String>,
    pub source: Option<String>,
    pub chunk_size: Option<usize>,
    pub chunk_overlap: Option<usize>,
    pub r#async: Option<bool>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "utoipa", into_params(parameter_in = Query))]
#[derive(Deserialize, Default)]
pub struct IngestQuery {
    pub r#async: Option<bool>,
}

/// The `202` body of `POST /v1/ingest` when `async: true`.
///
/// The async branch has always returned this object; the contract used to
/// declare the `202` with no content at all, so a generated client saw
/// `never` and had no typed way to reach `job_id` — the one field the whole
/// async flow depends on, since it is what `GET /v1/ingest/status/{job_id}`
/// takes.
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct IngestAcceptedResponse {
    pub ok: bool,
    /// Poll `GET /v1/ingest/status/{job_id}` with this id.
    pub job_id: String,
    /// Always `processing` on this response.
    pub status: String,
    pub collection: String,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct IngestResponse {
    pub ok: bool,
    pub document_node_id: u32,
    pub strategy_used: String,
    pub chunk_count: usize,
    pub record_ids: Vec<u32>,
    pub collection: String,
    /// Fetch `GET /v1/operations/:id/execution` with this id for the full
    /// per-stage execution breakdown (Execution Explorer).
    pub operation_id: String,
}

// ── GET /v1/ingest/status/:job_id ─────────────────────────────────────────────

/// The lifecycle states an asynchronous ingest job actually reports.
///
/// Phase API-3.3: these are the three literals both routers write — see the
/// `jobs.insert(..)` calls in [`ingest`] (standalone) and `cluster_ingest`
/// (cluster). There is no separate `pending`: a job is `processing` from the
/// moment `POST /v1/ingest?async=true` answers `202`.
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestJobState {
    /// Chunking, embedding, or committing is still in flight. Keep polling.
    Processing,
    /// Terminal success. `document_node_id` and `record_ids` are populated.
    Completed,
    /// Terminal failure. `error` carries the reason.
    Failed,
}

/// The body of `GET /v1/ingest/status/{job_id}`.
///
/// # Why this type exists
///
/// Phase API-3.3: this response was annotated `body = Object`, rendering as a
/// bare `type: object` with no properties — `object` in TypeScript,
/// `Dict[str, Any]` in Python. An SDK user polling an async ingest had no
/// typed way to learn whether the job finished, and no discoverable name for
/// the field carrying the answer. That defeats the purpose of the `202`
/// contract that points here.
///
/// Every field is optional except `status` and `job_id`, because which ones
/// are present genuinely depends on the stage the job has reached — the
/// terminal-success fields do not exist while it is `processing`, and `error`
/// exists only on `failed`. `status` is the discriminant to branch on.
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestJobStatusResponse {
    /// Which stage the job has reached. Branch on this.
    pub status: IngestJobState,
    /// Echo of the polled job id.
    pub job_id: String,
    /// Target collection. Absent on `failed` jobs that failed before resolving one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
    /// Chunks the document was split into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_count: Option<usize>,
    /// Chunking strategy the server selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy_used: Option<String>,
    /// `completed` only — the graph node representing the ingested document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_node_id: Option<u32>,
    /// `completed` only — the records written, one per chunk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_ids: Option<Vec<u32>>,
    /// `completed` only — correlates with `GET /v1/operations/{id}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    /// `failed` only — the human-readable reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    get,
    path = "/v1/ingest/status/{job_id}",
    operation_id = "get_ingest_status",
    tag = "ingest",
    summary = "Poll an asynchronous ingest job",
    description = "Job state is held in-process and does not survive a restart. The payload shape depends on which stage the job has reached.",
    params(
        ("job_id" = String, Path, description = "Job id returned by an async ingest"),
    ),
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Current job state", body = IngestJobStatusResponse),
        (status = 404, description = "No such job"),
    ),
))]
pub async fn get_ingest_status(
    axum::extract::Path(job_id): axum::extract::Path<String>,
    axum::Extension(tasks): axum::Extension<std::sync::Arc<crate::runner::TaskRegistry>>,
) -> Response {
    let jobs = tasks.jobs.read().await;
    match jobs.get(&job_id) {
        Some(status) => axum::Json(status.clone()).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({
                "error": format!("job '{job_id}' not found")
            })),
        )
            .into_response(),
    }
}

// ── POST /v1/ingest ───────────────────────────────────────────────────────────

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/ingest",
    operation_id = "ingest_document",
    tag = "ingest",
    summary = "Chunk, embed, and insert a document",
    description = "The full pipeline in one call. Requires `VALORI_EMBED_PROVIDER`. With `async: true` the call returns immediately and progress is polled through `GET /v1/ingest/status/{job_id}`.",
    params(
        IngestQuery,
    ),
    request_body = IngestRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Document ingested", body = IngestResponse),
        (status = 202, description = "Accepted for background processing; poll `GET /v1/ingest/status/{job_id}`", body = IngestAcceptedResponse),
        (status = 400, description = "Empty text, or the chunker produced no chunks", body = ApiError),
        (status = 413, description = "Text exceeds the maximum ingest size", body = ApiError),
        (status = 422, description = "VALORI_EMBED_PROVIDER is not configured", body = ApiError),
        (status = 500, description = "The ingest pipeline failed", body = ApiError),
        (status = 502, description = "The embedding provider failed", body = ApiError),
    ),
))]
pub async fn ingest(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<std::sync::Arc<valori_effect::ReceiptStore>>,
    axum::Extension(tasks): axum::Extension<std::sync::Arc<crate::runner::TaskRegistry>>,
    axum::Extension(executions): axum::Extension<std::sync::Arc<ExecutionRegistry>>,
    axum::extract::Query(query): axum::extract::Query<IngestQuery>,
    Json(payload): Json<IngestRequest>,
) -> Response {
    if payload.text.len() > MAX_INGEST_TEXT_BYTES {
        return valori_engine::error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            ErrorCode::ValidationError,
            format!("text exceeds maximum ingest size ({MAX_INGEST_TEXT_BYTES} bytes)"),
        );
    }
    let collection = payload
        .collection
        .clone()
        .unwrap_or_else(|| "default".into());
    let source = payload.source.clone().unwrap_or_else(|| "unknown".into());
    let strategy = payload.strategy.as_deref().unwrap_or("auto").to_string();
    let chunk_size = payload.chunk_size.unwrap_or(1000);
    let overlap = payload.chunk_overlap.unwrap_or(200);
    let is_async = query.r#async.or(payload.r#async).unwrap_or(false);

    // Embed config (set from VALORI_EMBED_PROVIDER/MODEL/URL at startup).
    let embed_cfg = {
        let engine = state.read().await;
        engine.embed_config.clone()
    };
    let embed_cfg = match embed_cfg {
        Some(c) => c,
        None => {
            return err_422("on-node embedding not configured — set VALORI_EMBED_PROVIDER (ollama/openai/custom), VALORI_EMBED_MODEL, VALORI_EMBED_URL");
        }
    };

    // Resolve the target namespace — needed before the pipeline runs so the
    // document node can be created and passed to KernelWriter.
    let ns = {
        let engine = state.read().await;
        match engine.resolve_collection(Some(&collection)) {
            Ok(n) => n,
            Err(e) => return err_400(&e.to_string()),
        }
    };

    // Create the document node once; KernelWriter uses it as parent for chunks.
    let doc_node_id = {
        let mut engine = state.write().await;
        engine.create_node_for_record(None, 0, ns).unwrap_or(0)
    };

    // Build provider from existing EmbedConfig (parsed from env vars at startup).
    let provider = match provider_from_config(
        &embed_cfg.provider,
        &embed_cfg.model,
        Some(&embed_cfg.url),
        embed_cfg.api_key.as_deref(),
        0,
    ) {
        Ok(p) => p,
        Err(e) => return err_422(&e.to_string()),
    };

    let writer = KernelWriter::new(
        state.clone(),
        ns,
        doc_node_id,
        &collection,
        &source,
        &strategy,
    );

    if is_async {
        // For async runs: snapshot counts for the immediate response, then
        // let the pipeline run in the background.
        let job_id = format!("job_{}", valori_core::id::ExecutionId::new_random());
        let resp = IngestAcceptedResponse {
            ok: true,
            job_id: job_id.clone(),
            status: "processing".into(),
            collection: collection.clone(),
        };
        {
            let mut jobs = tasks.jobs.write().await;
            jobs.insert(
                job_id.clone(),
                serde_json::json!({
                    "status": "processing", "job_id": job_id, "collection": collection,
                }),
            );
        }

        let operation_id = format!("ingest-{}", valori_core::id::ExecutionId::new_random());
        let text = payload.text.clone();
        let source_cl = source.clone();
        let strategy_cl = strategy.clone();
        let collection_cl = collection.clone();
        let job_id_cl = job_id.clone();
        let op_id_cl = operation_id.clone();
        let jobs_cl = tasks.jobs.clone();
        let receipts_cl = receipts.clone();
        let executions_cl = executions.clone();
        let state_cl = state.clone();

        tokio::spawn(async move {
            let state_before = state_hash(&state_cl).await;
            let mut pipeline = IngestPipeline::builder()
                .reader(TextReader)
                .chunker(DefaultChunker::new(&strategy_cl, chunk_size, overlap))
                .embedder(ModelProviderEmbedder::new(provider))
                .writer(writer)
                .build();

            match pipeline
                .run_observed(&text, Some(&source_cl), None, None)
                .await
            {
                Ok(result) => {
                    let record_ids: Vec<u32> = result
                        .writes
                        .iter()
                        .filter_map(|r| r.record_id.parse().ok())
                        .collect();
                    // Document-level metadata (total_chunks now known).
                    {
                        let mut engine = state_cl.write().await;
                        let now = now_unix();
                        let _ = engine.set_meta_audited(
                            format!("document:{doc_node_id}"),
                            serde_json::json!({
                                "source": source_cl, "total_chunks": result.writes.len(),
                                "collection": collection_cl, "strategy": strategy_cl,
                                "ingested_at": now,
                            }),
                        );
                    }
                    let state_after = state_hash(&state_cl).await;
                    let receipt = emit_ingest_receipt(
                        &receipts_cl,
                        &strategy_cl,
                        &collection_cl,
                        ns,
                        state_before.clone(),
                        state_after.clone(),
                    );
                    executions_cl.insert(ExecutionRecord::from_pipeline_result(
                        op_id_cl.clone(),
                        collection_cl.clone(),
                        &result,
                        Some(receipt.receipt_id),
                        Some(state_before),
                        Some(state_after),
                    ));
                    let mut jobs = jobs_cl.write().await;
                    jobs.insert(
                        job_id_cl.clone(),
                        serde_json::json!({
                            "status": "completed", "job_id": job_id_cl,
                            "document_node_id": doc_node_id,
                            "chunk_count": record_ids.len(),
                            "record_ids": record_ids, "collection": collection_cl,
                            "strategy_used": strategy_cl,
                            "operation_id": op_id_cl,
                        }),
                    );
                }
                Err(e) => {
                    let mut jobs = jobs_cl.write().await;
                    jobs.insert(
                        job_id_cl.clone(),
                        serde_json::json!({
                            "status": "failed", "job_id": job_id_cl, "error": e.to_string(),
                        }),
                    );
                }
            }
        });
        return (StatusCode::ACCEPTED, axum::Json(resp)).into_response();
    }

    // ── Synchronous path ──────────────────────────────────────────────────────
    let operation_id = format!("ingest-{}", valori_core::id::ExecutionId::new_random());
    let state_before = state_hash(&state).await;

    let mut pipeline = IngestPipeline::builder()
        .reader(TextReader)
        .chunker(DefaultChunker::new(&strategy, chunk_size, overlap))
        .embedder(ModelProviderEmbedder::new(provider))
        .writer(writer)
        .build();

    let result = match pipeline
        .run_observed(&payload.text, Some(&source), None, None)
        .await
    {
        Ok(r) if r.writes.is_empty() => return err_400("no chunks produced"),
        Ok(r) => r,
        Err(e) => {
            let (status, code) = match &e {
                valori_ingest::IngestError::Embed(_) => {
                    (StatusCode::BAD_GATEWAY, ErrorCode::Unavailable)
                }
                _ => (StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::InternalError),
            };
            return valori_engine::error_response(status, code, e.to_string());
        }
    };

    let record_ids: Vec<u32> = result
        .writes
        .iter()
        .filter_map(|r| r.record_id.parse().ok())
        .collect();

    // Document-level metadata (total_chunks now known after run).
    {
        let mut engine = state.write().await;
        let now = now_unix();
        if let Err(e) = engine.set_meta_audited(
            format!("document:{doc_node_id}"),
            serde_json::json!({
                "source": source, "total_chunks": result.writes.len(),
                "collection": collection, "strategy": strategy,
                "ingested_at": now,
            }),
        ) {
            tracing::warn!("ingest: failed to commit document metadata: {e:?}");
        }
    }

    let state_after = state_hash(&state).await;
    let receipt = emit_ingest_receipt(
        &receipts,
        &strategy,
        &collection,
        ns,
        state_before.clone(),
        state_after.clone(),
    );
    executions.insert(ExecutionRecord::from_pipeline_result(
        operation_id.clone(),
        collection.clone(),
        &result,
        Some(receipt.receipt_id),
        Some(state_before),
        Some(state_after),
    ));

    Json(IngestResponse {
        ok: true,
        document_node_id: doc_node_id,
        strategy_used: strategy,
        chunk_count: result.writes.len(),
        record_ids,
        collection,
        operation_id,
    })
    .into_response()
}

// ── Shared helpers ────────────────────────────────────────────────────────────

async fn state_hash(state: &SharedEngine) -> String {
    let engine = state.read().await;
    valori_kernel::snapshot::blake3::hash_state_blake3(&engine.state)
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

fn now_unix() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

fn emit_ingest_receipt(
    receipts: &std::sync::Arc<valori_effect::ReceiptStore>,
    strategy: &str,
    collection: &str,
    ns: u16,
    state_before: String,
    state_after: String,
) -> valori_effect::Receipt {
    use valori_planner::operation::{OperationInputs, OperationKind};
    let inputs = OperationInputs::Ingest {
        strategy: strategy.to_string(),
        collection: collection.to_string(),
        shard_id: 0,
        embed_enabled: true,
    };
    crate::receipt_bridge::emit_write(
        receipts,
        OperationKind::Ingest,
        &inputs,
        ns,
        0,
        0,
        false,
        state_before,
        state_after,
    )
}

// The ingest handlers used to emit a bare `{"error": "..."}` object, which is
// not the `ApiError` the contract declares for these statuses — it is missing
// the `code` discriminant an SDK is supposed to branch on. Routing them through
// `valori_engine::error_response` makes the runtime match the document. The
// change is additive: `error` keeps its meaning and `code` appears alongside it.
fn err_400(msg: &str) -> Response {
    valori_engine::error_response(StatusCode::BAD_REQUEST, ErrorCode::ValidationError, msg)
}

fn err_422(msg: &str) -> Response {
    valori_engine::error_response(
        StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::NotImplemented,
        msg,
    )
}

// ── POST /v1/ingest/update ────────────────────────────────────────────────────

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Deserialize)]
pub struct IngestUpdateRequest {
    pub document_node_id: u32,
    pub text: String,
    pub collection: Option<String>,
    pub strategy: Option<String>,
    pub source: Option<String>,
    pub chunk_size: Option<usize>,
    pub chunk_overlap: Option<usize>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct IngestUpdateResponse {
    pub ok: bool,
    pub document_node_id: u32,
    pub strategy_used: String,
    pub new_chunk_count: usize,
    pub kept_count: usize,
    pub removed_count: usize,
    pub added_count: usize,
    pub record_ids: Vec<u32>,
    pub collection: String,
}

#[cfg_attr(feature = "utoipa", utoipa::path(
    post,
    path = "/v1/ingest/update",
    operation_id = "update_ingested_document",
    tag = "ingest",
    summary = "Re-ingest a document, re-embedding only what changed",
    description = "Diffs the new chunk set against the stored one by BLAKE3 content hash. Unchanged chunks keep their existing records and are never re-embedded; the counts in the response say exactly what happened.",
    request_body = IngestUpdateRequest,
    security(("BearerAuth" = [])),
    responses(
        (status = 200, description = "Document updated", body = IngestUpdateResponse),
        (status = 400, description = "Malformed or invalid request", body = ApiError),
        (status = 404, description = "No such document node", body = ApiError),
        (status = 413, description = "Text exceeds the maximum ingest size", body = ApiError),
        (status = 422, description = "VALORI_EMBED_PROVIDER is not configured", body = ApiError),
        (status = 500, description = "Record insertion failed", body = ApiError),
        (status = 502, description = "The embedding provider failed", body = ApiError),
    ),
))]
pub async fn ingest_update(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<std::sync::Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<IngestUpdateRequest>,
) -> Response {
    if payload.text.len() > MAX_INGEST_TEXT_BYTES {
        return valori_engine::error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            ErrorCode::ValidationError,
            format!("text exceeds maximum ingest size ({MAX_INGEST_TEXT_BYTES} bytes)"),
        );
    }
    let collection = payload
        .collection
        .clone()
        .unwrap_or_else(|| "default".into());
    let source = payload.source.clone().unwrap_or_else(|| "unknown".into());
    let strategy = payload.strategy.as_deref().unwrap_or("auto");
    let chunk_size = payload.chunk_size.unwrap_or(1000);
    let overlap = payload.chunk_overlap.unwrap_or(200);
    let doc_node_id = payload.document_node_id;

    let embed_cfg = {
        let engine = state.read().await;
        engine.embed_config.clone()
    };
    let embed_cfg = match embed_cfg {
        Some(c) => c,
        None => {
            return valori_engine::error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::NotImplemented,
                "on-node embedding not configured — set VALORI_EMBED_PROVIDER",
            );
        }
    };

    let (new_chunks, strategy_used) = chunk_document(&payload.text, strategy, chunk_size, overlap);
    if new_chunks.is_empty() {
        return valori_engine::error_response(
            StatusCode::BAD_REQUEST,
            ErrorCode::ValidationError,
            "no chunks produced",
        );
    }

    let new_hashes: Vec<[u8; 32]> = new_chunks
        .iter()
        .map(|c| chunk_content_hash(&c.text))
        .collect();

    let old_chunks: Vec<(u32, u32, [u8; 32])> = {
        let engine = state.read().await;
        collect_old_chunks(&engine, doc_node_id)
    };

    use std::collections::HashMap;
    let mut new_hash_to_idx: HashMap<[u8; 32], Vec<usize>> = HashMap::new();
    for (i, h) in new_hashes.iter().enumerate() {
        new_hash_to_idx.entry(*h).or_default().push(i);
    }

    let mut kept_new_indices: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut kept_records: HashMap<usize, u32> = HashMap::new();
    let mut to_remove: Vec<(u32, u32)> = Vec::new();

    for (rid, cnid, old_hash) in &old_chunks {
        if let Some(indices) = new_hash_to_idx.get_mut(old_hash) {
            if let Some(idx) = indices
                .iter()
                .find(|i| !kept_new_indices.contains(i))
                .copied()
            {
                kept_new_indices.insert(idx);
                kept_records.insert(idx, *rid);
            } else {
                to_remove.push((*rid, *cnid));
            }
        } else {
            to_remove.push((*rid, *cnid));
        }
    }

    let to_add: Vec<usize> = (0..new_chunks.len())
        .filter(|i| !kept_new_indices.contains(i))
        .collect();

    let (state_before, ns) = {
        let engine = state.read().await;
        let ns = engine.resolve_collection(Some(&collection)).unwrap_or(0);
        let hash = valori_kernel::snapshot::blake3::hash_state_blake3(&engine.state)
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        (hash, ns)
    };

    {
        let mut engine = state.write().await;
        for (rid, _cnid) in &to_remove {
            if let Err(e) = engine.soft_delete_record(*rid) {
                tracing::warn!("ingest/update: failed to soft-delete record {rid}: {e:?}");
            }
        }
    }

    let mut added_record_ids: HashMap<usize, u32> = HashMap::new();
    if !to_add.is_empty() {
        let texts_to_embed: Vec<String> =
            to_add.iter().map(|&i| new_chunks[i].text.clone()).collect();
        let http = crate::server::shared_http_client();
        let vectors = match embed_batch(&texts_to_embed, &embed_cfg, http).await {
            Ok(v) => v,
            Err(e) => {
                return valori_engine::error_response(
                    StatusCode::BAD_GATEWAY,
                    ErrorCode::Unavailable,
                    e.to_string(),
                );
            }
        };

        let mut engine = state.write().await;
        let ns = match engine.resolve_collection(Some(&collection)) {
            Ok(n) => n,
            Err(e) => {
                return valori_engine::error_response(
                    StatusCode::BAD_REQUEST,
                    ErrorCode::ValidationError,
                    e.to_string(),
                );
            }
        };

        for (vec_idx, &chunk_idx) in to_add.iter().enumerate() {
            let rid = match engine.insert_record_from_f32_ns(&vectors[vec_idx], ns) {
                Ok(id) => id,
                Err(e) => {
                    return valori_engine::error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        ErrorCode::InternalError,
                        e.to_string(),
                    );
                }
            };
            engine.reranker_insert(rid, &new_chunks[chunk_idx].text);

            let chunk_node_id = match engine.create_node_for_record(
                Some(rid),
                valori_kernel::types::enums::NodeKind::Chunk as u8,
                ns,
            ) {
                Ok(id) => id,
                Err(e) => {
                    tracing::warn!("ingest/update: chunk node create failed: {e:?}");
                    0
                }
            };
            if chunk_node_id > 0 {
                let _ = engine.create_edge_ns(
                    doc_node_id,
                    chunk_node_id,
                    valori_kernel::types::enums::EdgeKind::ParentOf as u8,
                    ns,
                );
            }

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs().to_string())
                .unwrap_or_else(|_| "0".into());
            let _ = engine.set_meta_audited(
                format!("record:{rid}"),
                serde_json::json!({
                    "text": new_chunks[chunk_idx].text, "source": source,
                    "chunk_index": chunk_idx, "total_chunks": new_chunks.len(),
                    "section_title": new_chunks[chunk_idx].title,
                    "document_node_id": doc_node_id, "chunk_node_id": chunk_node_id,
                    "collection": collection, "chunk_mode": strategy_used,
                    "ingested_at": &now, "embed_model": &embed_cfg.model,
                    "embed_provider": &embed_cfg.provider,
                    "content_hash": new_hashes[chunk_idx].iter().map(|b| format!("{b:02x}")).collect::<String>(),
                }),
            );
            added_record_ids.insert(chunk_idx, rid);
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_else(|_| "0".into());
        let _ = engine.set_meta_audited(
            format!("document:{doc_node_id}"),
            serde_json::json!({
                "source": source, "total_chunks": new_chunks.len(),
                "collection": collection, "strategy": strategy_used,
                "embed_model": &embed_cfg.model, "updated_at": &now,
            }),
        );
    }

    let state_after: String = {
        let engine = state.read().await;
        valori_kernel::snapshot::blake3::hash_state_blake3(&engine.state)
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    };
    {
        use valori_planner::operation::{OperationInputs, OperationKind};
        let inputs = OperationInputs::Ingest {
            strategy: strategy_used.clone(),
            collection: collection.clone(),
            shard_id: 0,
            embed_enabled: true,
        };
        crate::receipt_bridge::emit_write(
            &receipts,
            OperationKind::Ingest,
            &inputs,
            ns,
            0,
            0,
            false,
            state_before,
            state_after,
        );
    }

    let mut record_ids = Vec::with_capacity(new_chunks.len());
    for i in 0..new_chunks.len() {
        if let Some(&rid) = kept_records.get(&i) {
            record_ids.push(rid);
        } else if let Some(&rid) = added_record_ids.get(&i) {
            record_ids.push(rid);
        }
    }

    Json(IngestUpdateResponse {
        ok: true,
        document_node_id: doc_node_id,
        strategy_used,
        new_chunk_count: new_chunks.len(),
        kept_count: kept_new_indices.len(),
        removed_count: to_remove.len(),
        added_count: to_add.len(),
        record_ids,
        collection,
    })
    .into_response()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn collect_old_chunks(
    engine: &crate::engine::Engine,
    doc_node_id: u32,
) -> Vec<(u32, u32, [u8; 32])> {
    use valori_kernel::types::enums::EdgeKind;
    use valori_kernel::types::id::NodeId;

    let mut result = Vec::new();
    let Some(edges) = engine.outgoing_edges(NodeId(doc_node_id)) else {
        return result;
    };
    for edge in edges {
        if edge.kind != EdgeKind::ParentOf {
            continue;
        }
        let chunk_node_id = edge.to.0;
        let Some(chunk_node) = engine.get_node(edge.to) else {
            continue;
        };
        let Some(record_id) = chunk_node.record else {
            continue;
        };
        let rid = record_id.0;
        let meta_key = format!("record:{rid}");
        let text = engine.metadata.get(&meta_key).and_then(|v| {
            v.get("text")
                .and_then(|t| t.as_str().map(|s| s.to_string()))
        });
        let hash = match text {
            Some(t) => chunk_content_hash(&t),
            None => [0u8; 32],
        };
        result.push((rid, chunk_node_id, hash));
    }
    result
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::TaskRegistry;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_get_ingest_status_not_found() {
        let registry = Arc::new(TaskRegistry::default_registry());
        let ext = axum::Extension(registry);
        let path = axum::extract::Path("job_nonexistent".to_string());
        let resp = get_ingest_status(path, ext).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_ingest_status_found() {
        let registry = Arc::new(TaskRegistry::default_registry());
        {
            let mut jobs = registry.jobs.write().await;
            jobs.insert(
                "job_123".to_string(),
                serde_json::json!({
                    "status": "processing", "job_id": "job_123", "chunk_count": 5
                }),
            );
        }
        let ext = axum::Extension(registry);
        let path = axum::extract::Path("job_123".to_string());
        let resp = get_ingest_status(path, ext).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
