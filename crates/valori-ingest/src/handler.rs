// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Stateless axum handler for `POST /v1/ingest/document`.
//!
//! Chunking only — no embedding, no engine state. Compiles into both the
//! standalone (`server.rs`) and cluster (`cluster_server.rs`) routers unchanged.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::chunker::{chunk_document, IngestChunk, MAX_INGEST_TEXT_BYTES};

// ── Request / Response ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct IngestDocumentRequest {
    /// Raw text content of the document.
    pub text: String,
    /// Collection to ingest into (default = "default").
    pub collection: Option<String>,
    /// Chunking strategy: `auto` | `tree` | `conversation` | `sentence` | `fixed`.
    pub strategy: Option<String>,
    /// Source label stored in metadata (e.g. filename). Optional.
    pub source: Option<String>,
    /// Fixed-strategy chunk size in chars (default 1000).
    pub chunk_size: Option<usize>,
    /// Fixed-strategy overlap in chars (default 200).
    pub chunk_overlap: Option<usize>,
}

#[derive(Serialize)]
pub struct IngestDocumentResponse {
    /// Strategy that was actually used (useful when `strategy="auto"`).
    pub strategy_used: String,
    /// Total number of chunks produced.
    pub chunk_count: usize,
    /// The chunks. Caller embeds each `text`, inserts the vector, records
    /// `record_id` → chunk for provenance.
    pub chunks: Vec<IngestChunk>,
    /// Collection the document was targeted at.
    pub collection: String,
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `POST /v1/ingest/document` — chunk a document server-side, no embedding.
///
/// Stateless: no `State<>` parameter — compiles into both routers unchanged.
pub async fn ingest_document(
    Json(payload): Json<IngestDocumentRequest>,
) -> Response {
    if payload.text.len() > MAX_INGEST_TEXT_BYTES {
        let body = serde_json::json!({
            "error": format!("text exceeds maximum ingest size ({MAX_INGEST_TEXT_BYTES} bytes)")
        });
        return (StatusCode::PAYLOAD_TOO_LARGE, Json(body)).into_response();
    }
    let collection = payload.collection.clone().unwrap_or_else(|| "default".into());
    let strategy_hint = payload.strategy.as_deref().unwrap_or("auto");
    let chunk_size = payload.chunk_size.unwrap_or(1000);
    let chunk_overlap = payload.chunk_overlap.unwrap_or(200);

    let (chunks, strategy_used) =
        chunk_document(&payload.text, strategy_hint, chunk_size, chunk_overlap);

    Json(IngestDocumentResponse {
        strategy_used,
        chunk_count: chunks.len(),
        chunks,
        collection,
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ingest_document_returns_chunks() {
        let payload = IngestDocumentRequest {
            text: "# Section One\nContent here that is long enough for the chunker.\n## Section Two\nMore content in section two here.".into(),
            collection: Some("test".into()),
            strategy: Some("tree".into()),
            source: None,
            chunk_size: None,
            chunk_overlap: None,
        };
        let resp = ingest_document(Json(payload)).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn ingest_document_rejects_oversized_text() {
        let payload = IngestDocumentRequest {
            text: "x".repeat(MAX_INGEST_TEXT_BYTES + 1),
            collection: None,
            strategy: None,
            source: None,
            chunk_size: None,
            chunk_overlap: None,
        };
        let resp = ingest_document(Json(payload)).await;
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
