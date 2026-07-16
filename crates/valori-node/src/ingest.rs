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
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use crate::server::SharedEngine;
// embed_batch and chunk_document are still used by ingest_update.
use valori_ingest::{embed_batch, chunk_document, chunk_content_hash};
use valori_ingest::{DefaultChunker, IngestPipeline, ModelProviderEmbedder, TextReader};
use valori_models::provider_from_config;
use crate::execution_registry::{ExecutionRecord, ExecutionRegistry};
use crate::kernel_writer::KernelWriter;

const MAX_INGEST_TEXT_BYTES: usize = valori_ingest::chunker::MAX_INGEST_TEXT_BYTES;

// ── Request / response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct IngestRequest {
    pub text: String,
    pub collection: Option<String>,
    pub strategy:   Option<String>,
    pub source:     Option<String>,
    pub chunk_size:    Option<usize>,
    pub chunk_overlap: Option<usize>,
    pub r#async:       Option<bool>,
}

#[derive(Deserialize, Default)]
pub struct IngestQuery {
    pub r#async: Option<bool>,
}

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

#[derive(Serialize)]
struct IngestErrorBody { error: String }

// ── GET /v1/ingest/status/:job_id ─────────────────────────────────────────────

pub async fn get_ingest_status(
    axum::extract::Path(job_id): axum::extract::Path<String>,
    axum::Extension(tasks): axum::Extension<std::sync::Arc<crate::runner::TaskRegistry>>,
) -> Response {
    let jobs = tasks.jobs.read().await;
    match jobs.get(&job_id) {
        Some(status) => axum::Json(status.clone()).into_response(),
        None => (StatusCode::NOT_FOUND, axum::Json(serde_json::json!({
            "error": format!("job '{job_id}' not found")
        }))).into_response(),
    }
}

// ── POST /v1/ingest ───────────────────────────────────────────────────────────

pub async fn ingest(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<std::sync::Arc<valori_effect::ReceiptStore>>,
    axum::Extension(tasks): axum::Extension<std::sync::Arc<crate::runner::TaskRegistry>>,
    axum::Extension(executions): axum::Extension<std::sync::Arc<ExecutionRegistry>>,
    axum::extract::Query(query): axum::extract::Query<IngestQuery>,
    Json(payload): Json<IngestRequest>,
) -> Response {
    if payload.text.len() > MAX_INGEST_TEXT_BYTES {
        let body = serde_json::json!({"error": format!("text exceeds maximum ingest size ({MAX_INGEST_TEXT_BYTES} bytes)")});
        return (StatusCode::PAYLOAD_TOO_LARGE, axum::Json(body)).into_response();
    }
    let collection    = payload.collection.clone().unwrap_or_else(|| "default".into());
    let source        = payload.source.clone().unwrap_or_else(|| "unknown".into());
    let strategy      = payload.strategy.as_deref().unwrap_or("auto").to_string();
    let chunk_size    = payload.chunk_size.unwrap_or(1000);
    let overlap       = payload.chunk_overlap.unwrap_or(200);
    let is_async      = query.r#async.or(payload.r#async).unwrap_or(false);

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
        &embed_cfg.provider, &embed_cfg.model,
        Some(&embed_cfg.url), embed_cfg.api_key.as_deref(), 0,
    ) {
        Ok(p) => p,
        Err(e) => return err_422(&e.to_string()),
    };

    let writer = KernelWriter::new(
        state.clone(), ns, doc_node_id, &collection, &source, &strategy,
    );

    if is_async {
        // For async runs: snapshot counts for the immediate response, then
        // let the pipeline run in the background.
        let job_id = format!("job_{}", valori_core::id::ExecutionId::new_random());
        let resp = serde_json::json!({
            "ok": true, "job_id": job_id, "status": "processing", "collection": collection,
        });
        {
            let mut jobs = tasks.jobs.write().await;
            jobs.insert(job_id.clone(), serde_json::json!({
                "status": "processing", "job_id": job_id, "collection": collection,
            }));
        }

        let operation_id = format!("ingest-{}", valori_core::id::ExecutionId::new_random());
        let text        = payload.text.clone();
        let source_cl   = source.clone();
        let strategy_cl = strategy.clone();
        let collection_cl = collection.clone();
        let job_id_cl   = job_id.clone();
        let op_id_cl    = operation_id.clone();
        let jobs_cl     = tasks.jobs.clone();
        let receipts_cl = receipts.clone();
        let executions_cl = executions.clone();
        let state_cl    = state.clone();

        tokio::spawn(async move {
            let state_before = state_hash(&state_cl).await;
            let mut pipeline = IngestPipeline::builder()
                .reader(TextReader)
                .chunker(DefaultChunker::new(&strategy_cl, chunk_size, overlap))
                .embedder(ModelProviderEmbedder::new(provider))
                .writer(writer)
                .build();

            match pipeline.run_observed(&text, Some(&source_cl), None, None).await {
                Ok(result) => {
                    let record_ids: Vec<u32> = result.writes.iter()
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
                    let receipt = emit_ingest_receipt(&receipts_cl, &strategy_cl, &collection_cl, ns, state_before.clone(), state_after.clone());
                    executions_cl.insert(ExecutionRecord::from_pipeline_result(
                        op_id_cl.clone(), collection_cl.clone(), &result,
                        Some(receipt.receipt_id), Some(state_before), Some(state_after),
                    ));
                    let mut jobs = jobs_cl.write().await;
                    jobs.insert(job_id_cl.clone(), serde_json::json!({
                        "status": "completed", "job_id": job_id_cl,
                        "document_node_id": doc_node_id,
                        "chunk_count": record_ids.len(),
                        "record_ids": record_ids, "collection": collection_cl,
                        "strategy_used": strategy_cl,
                        "operation_id": op_id_cl,
                    }));
                }
                Err(e) => {
                    let mut jobs = jobs_cl.write().await;
                    jobs.insert(job_id_cl.clone(), serde_json::json!({
                        "status": "failed", "job_id": job_id_cl, "error": e.to_string(),
                    }));
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

    let result = match pipeline.run_observed(&payload.text, Some(&source), None, None).await {
        Ok(r) if r.writes.is_empty() => return err_400("no chunks produced"),
        Ok(r) => r,
        Err(e) => {
            let code = match &e {
                valori_ingest::IngestError::Embed(_) => StatusCode::BAD_GATEWAY,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            return (code, axum::Json(serde_json::json!({"error": e.to_string()}))).into_response();
        }
    };

    let record_ids: Vec<u32> = result.writes.iter()
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
    let receipt = emit_ingest_receipt(&receipts, &strategy, &collection, ns, state_before.clone(), state_after.clone());
    executions.insert(ExecutionRecord::from_pipeline_result(
        operation_id.clone(), collection.clone(), &result,
        Some(receipt.receipt_id), Some(state_before), Some(state_after),
    ));

    Json(IngestResponse {
        ok: true, document_node_id: doc_node_id,
        strategy_used: strategy, chunk_count: result.writes.len(),
        record_ids, collection, operation_id,
    }).into_response()
}

// ── Shared helpers ────────────────────────────────────────────────────────────

async fn state_hash(state: &SharedEngine) -> String {
    let engine = state.read().await;
    valori_kernel::snapshot::blake3::hash_state_blake3(&engine.state)
        .iter().map(|b| format!("{:02x}", b)).collect()
}

fn now_unix() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

fn emit_ingest_receipt(
    receipts: &std::sync::Arc<valori_effect::ReceiptStore>,
    strategy: &str, collection: &str, ns: u16,
    state_before: String, state_after: String,
) -> valori_effect::Receipt {
    use valori_planner::operation::{OperationInputs, OperationKind};
    let inputs = OperationInputs::Ingest {
        strategy: strategy.to_string(), collection: collection.to_string(),
        shard_id: 0, embed_enabled: true,
    };
    crate::receipt_bridge::emit_write(receipts, OperationKind::Ingest, &inputs, ns, 0, 0, false, state_before, state_after)
}

fn err_400(msg: &str) -> Response {
    (StatusCode::BAD_REQUEST, axum::Json(serde_json::json!({"error": msg}))).into_response()
}

fn err_422(msg: &str) -> Response {
    (StatusCode::UNPROCESSABLE_ENTITY, axum::Json(serde_json::json!({"error": msg}))).into_response()
}

// ── POST /v1/ingest/update ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct IngestUpdateRequest {
    pub document_node_id: u32,
    pub text: String,
    pub collection: Option<String>,
    pub strategy:   Option<String>,
    pub source:     Option<String>,
    pub chunk_size:    Option<usize>,
    pub chunk_overlap: Option<usize>,
}

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

pub async fn ingest_update(
    State(state): State<SharedEngine>,
    axum::Extension(receipts): axum::Extension<std::sync::Arc<valori_effect::ReceiptStore>>,
    Json(payload): Json<IngestUpdateRequest>,
) -> Response {
    if payload.text.len() > MAX_INGEST_TEXT_BYTES {
        let body = serde_json::json!({"error": format!("text exceeds maximum ingest size ({MAX_INGEST_TEXT_BYTES} bytes)")});
        return (StatusCode::PAYLOAD_TOO_LARGE, axum::Json(body)).into_response();
    }
    let collection  = payload.collection.clone().unwrap_or_else(|| "default".into());
    let source      = payload.source.clone().unwrap_or_else(|| "unknown".into());
    let strategy    = payload.strategy.as_deref().unwrap_or("auto");
    let chunk_size  = payload.chunk_size.unwrap_or(1000);
    let overlap     = payload.chunk_overlap.unwrap_or(200);
    let doc_node_id = payload.document_node_id;

    let embed_cfg = {
        let engine = state.read().await;
        engine.embed_config.clone()
    };
    let embed_cfg = match embed_cfg {
        Some(c) => c,
        None => {
            let body = serde_json::to_vec(&IngestErrorBody {
                error: "on-node embedding not configured — set VALORI_EMBED_PROVIDER".into(),
            }).unwrap();
            return (StatusCode::UNPROCESSABLE_ENTITY,
                    axum::http::header::HeaderMap::new(), body).into_response();
        }
    };

    let (new_chunks, strategy_used) = chunk_document(&payload.text, strategy, chunk_size, overlap);
    if new_chunks.is_empty() {
        let body = serde_json::to_vec(&IngestErrorBody { error: "no chunks produced".into() }).unwrap();
        return (StatusCode::BAD_REQUEST, axum::http::header::HeaderMap::new(), body).into_response();
    }

    let new_hashes: Vec<[u8; 32]> = new_chunks.iter()
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
            if let Some(idx) = indices.iter().find(|i| !kept_new_indices.contains(i)).copied() {
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
            .iter().map(|b| format!("{:02x}", b)).collect();
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
        let texts_to_embed: Vec<String> = to_add.iter().map(|&i| new_chunks[i].text.clone()).collect();
        let http = crate::server::shared_http_client();
        let vectors = match embed_batch(&texts_to_embed, &embed_cfg, http).await {
            Ok(v) => v,
            Err(e) => {
                let body = serde_json::to_vec(&IngestErrorBody { error: e.to_string() }).unwrap();
                return (StatusCode::BAD_GATEWAY, axum::http::header::HeaderMap::new(), body).into_response();
            }
        };

        let mut engine = state.write().await;
        let ns = match engine.resolve_collection(Some(&collection)) {
            Ok(n) => n,
            Err(e) => {
                let body = serde_json::to_vec(&IngestErrorBody { error: e.to_string() }).unwrap();
                return (StatusCode::BAD_REQUEST, axum::http::header::HeaderMap::new(), body).into_response();
            }
        };

        for (vec_idx, &chunk_idx) in to_add.iter().enumerate() {
            let rid = match engine.insert_record_from_f32_ns(&vectors[vec_idx], ns) {
                Ok(id) => id,
                Err(e) => {
                    let body = serde_json::to_vec(&IngestErrorBody { error: e.to_string() }).unwrap();
                    return (StatusCode::INTERNAL_SERVER_ERROR, axum::http::header::HeaderMap::new(), body).into_response();
                }
            };
            engine.reranker_insert(rid, &new_chunks[chunk_idx].text);

            let chunk_node_id = match engine.create_node_for_record(
                Some(rid),
                valori_kernel::types::enums::NodeKind::Chunk as u8,
                ns,
            ) {
                Ok(id) => id,
                Err(e) => { tracing::warn!("ingest/update: chunk node create failed: {e:?}"); 0 }
            };
            if chunk_node_id > 0 {
                let _ = engine.create_edge(
                    doc_node_id, chunk_node_id,
                    valori_kernel::types::enums::EdgeKind::ParentOf as u8,
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
            .iter().map(|b| format!("{:02x}", b)).collect()
    };
    {
        use valori_planner::operation::{OperationInputs, OperationKind};
        let inputs = OperationInputs::Ingest {
            strategy: strategy_used.clone(), collection: collection.clone(),
            shard_id: 0, embed_enabled: true,
        };
        crate::receipt_bridge::emit_write(
            &receipts, OperationKind::Ingest, &inputs, ns, 0, 0, false, state_before, state_after,
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
        ok: true, document_node_id: doc_node_id, strategy_used,
        new_chunk_count: new_chunks.len(), kept_count: kept_new_indices.len(),
        removed_count: to_remove.len(), added_count: to_add.len(),
        record_ids, collection,
    }).into_response()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn collect_old_chunks(engine: &crate::engine::Engine, doc_node_id: u32) -> Vec<(u32, u32, [u8; 32])> {
    use valori_kernel::types::id::NodeId;
    use valori_kernel::types::enums::EdgeKind;

    let mut result = Vec::new();
    let Some(edges) = engine.outgoing_edges(NodeId(doc_node_id)) else {
        return result;
    };
    for edge in edges {
        if edge.kind != EdgeKind::ParentOf { continue; }
        let chunk_node_id = edge.to.0;
        let Some(chunk_node) = engine.get_node(edge.to) else { continue };
        let Some(record_id) = chunk_node.record else { continue };
        let rid = record_id.0;
        let meta_key = format!("record:{rid}");
        let text = engine.metadata.get(&meta_key)
            .and_then(|v| v.get("text").and_then(|t| t.as_str().map(|s| s.to_string())));
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
            jobs.insert("job_123".to_string(), serde_json::json!({
                "status": "processing", "job_id": "job_123", "chunk_count": 5
            }));
        }
        let ext = axum::Extension(registry);
        let path = axum::extract::Path("job_123".to_string());
        let resp = get_ingest_status(path, ext).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
