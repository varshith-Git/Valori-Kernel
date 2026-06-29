/// /v1/ingest/document — server-side document ingestion with built-in chunking.
///
/// Strategies
/// ----------
/// auto         Sniff the text and pick the best strategy automatically.
/// tree         Split on section headers (numbered "3.1 Title" or "## Title").
///              One chunk per section; title prepended to body. Best for
///              structured documents (papers, manuals, reports).
/// conversation Split on question boundaries (lines ending with "?").
///              Groups each Q + answer block as one chunk. Best for interview
///              transcripts, meeting notes, chat logs.
/// sentence     Split on sentence endings (.  !  ?). Each sentence is one
///              retrieval unit; the surrounding ±2 sentences are included in
///              the stored text for LLM context. Best for prose / articles.
/// fixed        Overlapping fixed-size windows (default chunk_size=1000,
///              overlap=200). General-purpose fallback.
///
/// The handler:
///   1. Chunks the raw text server-side.
///   2. Stores each chunk's text in the metadata sidecar (for the reranker
///      and the Ask UI) under key `record:<id>`.
///   3. Returns chunk texts + record IDs; the caller is still responsible
///      for embedding and inserting vectors via /v1/vectors/batch_insert.
///      (Phase 2 will add on-node embedding when an embed provider is
///      configured via env var.)
///
/// This design keeps the embedding provider outside the kernel (the kernel
/// must stay no_std), but moves all chunking intelligence server-side so
/// every SDK and UI gets identical, auditable chunk boundaries.

use axum::{extract::State, Json};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use crate::server::SharedEngine;
use crate::embedder::embed_batch;

/// M-6: Maximum accepted text length for ingest/chunk endpoints.
/// 10 MB is generous for real documents; beyond this the chunker + embedding
/// loops become a DoS vector (O(n) memory + one embed call per chunk).
const MAX_INGEST_TEXT_BYTES: usize = 10 * 1024 * 1024;

// ── Public request / response types ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct IngestDocumentRequest {
    /// Raw text content of the document.
    pub text: String,
    /// Collection to ingest into (default = "default").
    pub collection: Option<String>,
    /// Chunking strategy. One of: auto | tree | conversation | sentence | fixed.
    /// Default: auto.
    pub strategy: Option<String>,
    /// Source label stored in metadata (e.g. filename). Optional.
    pub source: Option<String>,
    /// Fixed-strategy chunk size in chars (default 1000).
    pub chunk_size: Option<usize>,
    /// Fixed-strategy overlap in chars (default 200).
    pub chunk_overlap: Option<usize>,
}

#[derive(Serialize)]
pub struct IngestChunk {
    /// 0-based index of this chunk.
    pub index: usize,
    /// Section title (tree strategy) or empty string.
    pub title: String,
    /// Full chunk text ready to embed. Store this as `text` in metadata.
    pub text: String,
}

#[derive(Serialize)]
pub struct IngestDocumentResponse {
    /// Strategy that was actually used (useful when strategy="auto").
    pub strategy_used: String,
    /// Total number of chunks produced.
    pub chunk_count: usize,
    /// The chunks. Caller embeds each `text`, inserts the vector via
    /// /v1/vectors/batch_insert with `texts` set, then records
    /// `record_id` → chunk for provenance.
    pub chunks: Vec<IngestChunk>,
    /// Collection the document was targeted at.
    pub collection: String,
}

// ── Axum handler ─────────────────────────────────────────────────────────────
// Chunking is pure-text and stateless — no engine state needed.
// Collection validation is left to the caller's subsequent batch_insert call.

pub async fn ingest_document(
    Json(payload): Json<IngestDocumentRequest>,
) -> Response {
    if payload.text.len() > MAX_INGEST_TEXT_BYTES {
        let body = serde_json::json!({"error": format!("text exceeds maximum ingest size ({MAX_INGEST_TEXT_BYTES} bytes)")});
        return (StatusCode::PAYLOAD_TOO_LARGE, axum::Json(body)).into_response();
    }
    let collection = payload.collection.clone().unwrap_or_else(|| "default".into());
    let strategy_hint  = payload.strategy.as_deref().unwrap_or("auto");
    let chunk_size     = payload.chunk_size.unwrap_or(1000);
    let chunk_overlap  = payload.chunk_overlap.unwrap_or(200);

    let (chunks, strategy_used) = chunk_document(
        &payload.text,
        strategy_hint,
        chunk_size,
        chunk_overlap,
    );

    Json(IngestDocumentResponse {
        strategy_used,
        chunk_count: chunks.len(),
        chunks,
        collection,
    }).into_response()
}

// ── Strategy dispatcher ───────────────────────────────────────────────────────

/// Returns (chunks, strategy_name_used).
pub fn chunk_document(
    text: &str,
    strategy: &str,
    chunk_size: usize,
    chunk_overlap: usize,
) -> (Vec<IngestChunk>, String) {
    match strategy {
        "tree"         => {
            let nodes = chunk_tree(text);
            if nodes.len() >= 2 {
                return (nodes, "tree".into());
            }
            // Fall through to auto if tree found nothing
            let (c, _) = chunk_document(text, "auto", chunk_size, chunk_overlap);
            (c, "tree->auto".into())
        }
        "conversation" => {
            let nodes = chunk_conversation(text);
            if nodes.len() >= 2 {
                return (nodes, "conversation".into());
            }
            let (c, _) = chunk_document(text, "fixed", chunk_size, chunk_overlap);
            (c, "conversation->fixed".into())
        }
        "sentence"     => (chunk_sentence_window(text), "sentence".into()),
        "fixed"        => (chunk_fixed(text, chunk_size, chunk_overlap), "fixed".into()),
        _              => {
            // auto: sniff the text
            let detected = detect_strategy(text);
            chunk_document(text, detected, chunk_size, chunk_overlap)
        }
    }
}

// ── Auto-detection ────────────────────────────────────────────────────────────

fn detect_strategy(text: &str) -> &'static str {
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len().max(1);

    // Count section header lines
    let header_lines = lines.iter().filter(|l| is_section_header(l)).count();
    // Count timestamp patterns (HH:MM or MM:SS at start of line)
    let ts_lines = lines.iter().filter(|l| {
        let s = l.trim();
        s.len() >= 4 && s.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
            && s.contains(':')
            && s.len() <= 8
    }).count();
    // Count question lines
    let question_lines = lines.iter().filter(|l| l.trim().ends_with('?')).count();

    if header_lines * 10 > total {
        // >10% of lines are section headers → structured doc
        "tree"
    } else if ts_lines * 5 > total || (question_lines > 3 && ts_lines > 3) {
        // >20% timestamp lines, or many timestamps + questions → transcript
        "conversation"
    } else {
        "fixed"
    }
}

// ── Tree chunker ──────────────────────────────────────────────────────────────
//
// Detects numbered section headers ("3.1 Training", "## Background") and
// groups everything between two headers as one chunk.  The header line is
// prepended to the body so the LLM always sees the section title with the
// answer.

fn is_section_header(line: &str) -> bool {
    let s = line.trim();
    if s.is_empty() { return false; }
    // Markdown header: ## Title
    if s.starts_with('#') {
        let rest = s.trim_start_matches('#').trim();
        return rest.len() >= 3;
    }
    // Numbered section: "3", "3.1", "3.1.2" followed by a title word
    let mut chars = s.chars();
    let first = chars.next().unwrap_or(' ');
    if first.is_ascii_digit() {
        let head: String = s.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        let rest = s[head.len()..].trim();
        // Must have at least one word after the number, starts with uppercase
        return rest.len() >= 2
            && rest.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
            && rest.len() <= 80
            && !rest.ends_with('.');
    }
    false
}

fn chunk_tree(text: &str) -> Vec<IngestChunk> {
    let normalized = normalize(text);
    let lines: Vec<&str> = normalized.lines().collect();

    let mut header_positions: Vec<usize> = Vec::new();
    let mut in_code = false;
    for (i, line) in lines.iter().enumerate() {
        let s = line.trim();
        if s.starts_with("```") { in_code = !in_code; continue; }
        if in_code { continue; }
        if is_section_header(s) {
            header_positions.push(i);
        }
    }

    if header_positions.len() < 2 {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    for (idx, &start) in header_positions.iter().enumerate() {
        let end = if idx + 1 < header_positions.len() {
            header_positions[idx + 1]
        } else {
            lines.len()
        };
        let title = lines[start].trim().to_string();
        let body  = lines[start + 1..end]
            .iter()
            .map(|l| *l)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        if body.len() < 30 && idx + 1 < header_positions.len() {
            continue; // skip trivially empty sections
        }
        let combined = format!("{}\n{}", title, body);
        chunks.push(IngestChunk {
            index: chunks.len(),
            title,
            text: combined,
        });
    }
    chunks
}

// ── Conversation chunker ──────────────────────────────────────────────────────
//
// Splits on lines that end with "?" (interviewer questions).  Each block is
// the question + all lines until the next question.  Timestamps are stripped
// so the chunk text is clean prose.

fn strip_timestamp(line: &str) -> &str {
    let s = line.trim();
    // Lines like "12:03" or "1:23:45" are pure timestamps — skip entirely
    if s.len() <= 8 && s.chars().all(|c| c.is_ascii_digit() || c == ':') {
        return "";
    }
    s
}

fn chunk_conversation(text: &str) -> Vec<IngestChunk> {
    let normalized = normalize(text);
    let lines: Vec<&str> = normalized.lines().collect();

    // Build clean lines (timestamp lines become empty → filtered out)
    let clean: Vec<String> = lines.iter()
        .map(|l| strip_timestamp(l).to_string())
        .filter(|l| !l.is_empty())
        .collect();

    if clean.is_empty() { return Vec::new(); }

    // Find question-boundary indices in clean lines
    let mut q_indices: Vec<usize> = Vec::new();
    for (i, line) in clean.iter().enumerate() {
        if line.trim().ends_with('?') {
            q_indices.push(i);
        }
    }

    if q_indices.len() < 2 {
        // No question structure — just split into ~800-char blocks
        return chunk_fixed_str(&clean.join("\n"), 800, 100);
    }

    let mut chunks: Vec<IngestChunk> = Vec::new();

    for (idx, &q_start) in q_indices.iter().enumerate() {
        let q_end = if idx + 1 < q_indices.len() {
            q_indices[idx + 1]
        } else {
            clean.len()
        };
        let block: Vec<&str> = clean[q_start..q_end].iter().map(|s| s.as_str()).collect();
        let text_block = block.join(" ").trim().to_string();
        if text_block.len() < 20 { continue; }
        let title = clean[q_start].chars().take(80).collect::<String>();
        chunks.push(IngestChunk {
            index: chunks.len(),
            title,
            text: text_block,
        });
    }

    // Capture any leading text before first question
    if let Some(&first_q) = q_indices.first() {
        if first_q > 0 {
            let intro = clean[..first_q].join(" ").trim().to_string();
            if intro.len() >= 30 {
                let mut reindexed = vec![IngestChunk {
                    index: 0,
                    title: "Introduction".into(),
                    text: intro,
                }];
                for (i, mut c) in chunks.into_iter().enumerate() {
                    c.index = i + 1;
                    reindexed.push(c);
                }
                return reindexed;
            }
        }
    }

    chunks
}

// ── Sentence-window chunker ───────────────────────────────────────────────────
//
// Splits on sentence boundaries (. ! ?).  Each sentence is one retrieval unit
// but the stored text includes 2 sentences of surrounding context so the LLM
// has enough material to answer.

fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences: Vec<String> = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        current.push(chars[i]);
        if matches!(chars[i], '.' | '!' | '?') {
            // Peek: if next non-space char is uppercase or end, it's a boundary
            let next_meaningful = chars[i+1..].iter().find(|&&c| c != ' ' && c != '\n');
            let is_boundary = match next_meaningful {
                None => true,
                Some(&c) => c.is_uppercase() || c == '"' || c == '\'' || c == '(' ,
            };
            if is_boundary && current.trim().split_whitespace().count() >= 4 {
                sentences.push(current.trim().to_string());
                current = String::new();
            }
        }
        i += 1;
    }
    if !current.trim().is_empty() {
        sentences.push(current.trim().to_string());
    }
    sentences
}

fn chunk_sentence_window(text: &str) -> Vec<IngestChunk> {
    let normalized = normalize(text);
    let sentences = split_sentences(&normalized);
    if sentences.is_empty() { return Vec::new(); }

    const WINDOW: usize = 2; // sentences of context on each side

    sentences.iter().enumerate().map(|(i, _sentence)| {
        let lo = i.saturating_sub(WINDOW);
        let hi = (i + WINDOW + 1).min(sentences.len());
        let window_text = sentences[lo..hi].join(" ");
        IngestChunk {
            index: i,
            title: String::new(),
            text: window_text,
        }
    }).collect()
}

// ── Fixed-size chunker ────────────────────────────────────────────────────────

fn chunk_fixed(text: &str, size: usize, overlap: usize) -> Vec<IngestChunk> {
    chunk_fixed_str(&normalize(text), size, overlap)
}

fn chunk_fixed_str(text: &str, size: usize, overlap: usize) -> Vec<IngestChunk> {
    let size    = size.max(50);
    let overlap = overlap.min(size / 2);
    let step    = size - overlap;
    let chars: Vec<char> = text.chars().collect();
    let mut chunks = Vec::new();
    let mut start = 0usize;

    while start < chars.len() {
        let end = (start + size).min(chars.len());
        // Snap to sentence boundary within ±80 chars
        let end = snap_boundary(&chars, end, 80);
        let chunk_text: String = chars[start..end].iter().collect();
        let chunk_text = chunk_text.trim().to_string();
        if chunk_text.len() >= 30 {
            chunks.push(IngestChunk {
                index: chunks.len(),
                title: String::new(),
                text: chunk_text,
            });
        }
        if end >= chars.len() { break; }
        start = start + step;
    }
    chunks
}

fn snap_boundary(chars: &[char], pos: usize, window: usize) -> usize {
    if pos >= chars.len() { return chars.len(); }
    let lo = pos.saturating_sub(window);
    let hi = (pos + window).min(chars.len());
    // Search forward from pos for a sentence end
    for i in pos..hi {
        if matches!(chars[i], '.' | '!' | '?' | '\n') {
            return i + 1;
        }
    }
    // Search backward
    for i in (lo..pos).rev() {
        if matches!(chars[i], '.' | '!' | '?' | '\n') {
            return i + 1;
        }
    }
    pos
}

// ── Phase 2: full-pipeline handler (/v1/ingest) ───────────────────────────────
//
// chunk + embed + insert + graph nodes + metadata sidecar — one call.
// Requires VALORI_EMBED_PROVIDER (and model/url/key) to be configured.
// Returns the document_node_id + per-chunk record IDs so the caller has
// full provenance without any extra round-trips.

#[derive(Deserialize)]
pub struct IngestRequest {
    /// Raw text to ingest (UTF-8). The node chunks, embeds, and inserts it.
    pub text: String,
    pub collection: Option<String>,
    pub strategy:   Option<String>,
    /// Human-readable source label (filename, URL, …) stored in metadata.
    pub source:     Option<String>,
    pub chunk_size:    Option<usize>,
    pub chunk_overlap: Option<usize>,
}

#[derive(Serialize)]
pub struct IngestResponse {
    pub ok: bool,
    pub document_node_id: u32,
    pub strategy_used: String,
    pub chunk_count: usize,
    /// record_id for each chunk, in order.
    pub record_ids: Vec<u32>,
    pub collection: String,
}

#[derive(Serialize)]
struct IngestErrorBody { error: String }

pub async fn ingest(
    State(state): State<SharedEngine>,
    Json(payload): Json<IngestRequest>,
) -> Response {
    if payload.text.len() > MAX_INGEST_TEXT_BYTES {
        let body = serde_json::json!({"error": format!("text exceeds maximum ingest size ({MAX_INGEST_TEXT_BYTES} bytes)")});
        return (StatusCode::PAYLOAD_TOO_LARGE, axum::Json(body)).into_response();
    }
    let collection = payload.collection.clone().unwrap_or_else(|| "default".into());
    let source     = payload.source.clone().unwrap_or_else(|| "unknown".into());
    let strategy   = payload.strategy.as_deref().unwrap_or("auto");
    let chunk_size = payload.chunk_size.unwrap_or(1000);
    let overlap    = payload.chunk_overlap.unwrap_or(200);

    // 1. Check embed is configured
    let embed_cfg = {
        let engine = state.read().await;
        engine.embed_config.clone()
    };
    let embed_cfg = match embed_cfg {
        Some(c) => c,
        None => {
            let body = serde_json::to_vec(&IngestErrorBody {
                error: "on-node embedding not configured — set VALORI_EMBED_PROVIDER (ollama/openai/custom), VALORI_EMBED_MODEL, VALORI_EMBED_URL".into(),
            }).unwrap();
            return (StatusCode::UNPROCESSABLE_ENTITY,
                    axum::http::header::HeaderMap::new(),
                    body).into_response();
        }
    };

    // 2. Chunk
    let (chunks, strategy_used) = chunk_document(&payload.text, strategy, chunk_size, overlap);
    if chunks.is_empty() {
        let body = serde_json::to_vec(&IngestErrorBody { error: "no chunks produced".into() }).unwrap();
        return (StatusCode::BAD_REQUEST, axum::http::header::HeaderMap::new(), body).into_response();
    }

    // 3. Embed — one HTTP call per chunk for Ollama, batched for OpenAI
    let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
    let http = reqwest::Client::new();
    let vectors = match embed_batch(&texts, &embed_cfg, &http).await {
        Ok(v) => v,
        Err(e) => {
            let body = serde_json::to_vec(&IngestErrorBody { error: e.to_string() }).unwrap();
            return (StatusCode::BAD_GATEWAY, axum::http::header::HeaderMap::new(), body).into_response();
        }
    };

    if vectors.is_empty() || vectors[0].is_empty() {
        let body = serde_json::to_vec(&IngestErrorBody { error: "embed provider returned empty vectors".into() }).unwrap();
        return (StatusCode::BAD_GATEWAY, axum::http::header::HeaderMap::new(), body).into_response();
    }

    // 4. Insert vectors + register texts for reranker
    let mut engine = state.write().await;
    let ns = match engine.resolve_collection(Some(&collection)) {
        Ok(n) => n,
        Err(e) => {
            let body = serde_json::to_vec(&IngestErrorBody { error: e.to_string() }).unwrap();
            return (StatusCode::BAD_REQUEST, axum::http::header::HeaderMap::new(), body).into_response();
        }
    };

    let record_ids = match engine.insert_batch_ns(&vectors, None, ns, None) {
        Ok(ids) => ids,
        Err(e) => {
            let body = serde_json::to_vec(&IngestErrorBody { error: e.to_string() }).unwrap();
            return (StatusCode::INTERNAL_SERVER_ERROR, axum::http::header::HeaderMap::new(), body).into_response();
        }
    };

    // Register texts in reranker
    for (id, text) in record_ids.iter().zip(texts.iter()) {
        engine.reranker_insert(*id, text);
    }

    // 5. Document graph node (kind 0 = Document)
    let doc_node_id = match engine.create_node_for_record(None, 0, ns) {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!("ingest: failed to create document node: {e:?}");
            0
        }
    };

    // 6. Chunk graph nodes + edges + metadata sidecar
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into());
    for (i, (chunk, &rid)) in chunks.iter().zip(record_ids.iter()).enumerate() {
        // Chunk graph node (kind 1 = Chunk)
        if let Ok(chunk_node_id) = engine.create_node_for_record(Some(rid), 1, ns) {
            // ParentOf edge (kind 6)
            let _ = engine.create_edge(doc_node_id, chunk_node_id, 6);

            // Metadata sidecar
            engine.metadata.set(
                format!("record:{rid}"),
                serde_json::json!({
                    "text": chunk.text,
                    "source": source,
                    "chunk_index": i,
                    "total_chunks": chunks.len(),
                    "section_title": chunk.title,
                    "document_node_id": doc_node_id,
                    "chunk_node_id": chunk_node_id,
                    "collection": collection,
                    "chunk_mode": strategy_used,
                    "ingested_at": &now,
                    "embed_model": &embed_cfg.model,
                    "embed_provider": &embed_cfg.provider,
                }),
            );
        }
    }

    // Document-level metadata
    engine.metadata.set(
        format!("document:{doc_node_id}"),
        serde_json::json!({
            "source": source,
            "total_chunks": chunks.len(),
            "collection": collection,
            "strategy": strategy_used,
            "embed_model": &embed_cfg.model,
            "ingested_at": &now,
        }),
    );

    if let Err(e) = engine.flush_metadata() {
        tracing::warn!("ingest: metadata flush failed: {e:?}");
    }

    drop(engine);

    Json(IngestResponse {
        ok: true,
        document_node_id: doc_node_id,
        strategy_used,
        chunk_count: chunks.len(),
        record_ids,
        collection,
    }).into_response()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
        .split('\n')
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}
