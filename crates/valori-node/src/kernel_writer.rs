// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`KernelWriter`] — the `valori-node` implementation of [`valori_ingest::Writer`].
//!
//! Owns per-chunk write logic: vector insert, chunk-node creation, parent edge,
//! and chunk metadata commit. Document-level metadata (source, total_chunks,
//! strategy) is set by the handler after `IngestPipeline::run()` completes,
//! because the handler already has `doc_node_id` and the final count.

use valori_ingest::{Chunk, Document, Embedding, IngestError, WriteResult, Writer};

use crate::server::SharedEngine;

/// Writes one chunk+embedding into the in-process `KernelState`.
///
/// Stateful: holds the engine handle, resolved namespace, and the document node
/// that was created before the pipeline ran. The handler creates the document
/// node and passes it here; the writer creates one chunk node per `write()` call.
pub struct KernelWriter {
    engine: SharedEngine,
    /// Resolved namespace id for the target collection.
    ns: u16,
    /// Document node created before the pipeline ran; used as parent of every chunk.
    pub doc_node_id: u32,
    collection: String,
    source: String,
    strategy_used: String,
}

impl KernelWriter {
    pub fn new(
        engine: SharedEngine,
        ns: u16,
        doc_node_id: u32,
        collection: impl Into<String>,
        source: impl Into<String>,
        strategy_used: impl Into<String>,
    ) -> Self {
        Self {
            engine,
            ns,
            doc_node_id,
            collection: collection.into(),
            source: source.into(),
            strategy_used: strategy_used.into(),
        }
    }
}

#[async_trait::async_trait]
impl Writer for KernelWriter {
    async fn write(
        &mut self,
        chunk: &Chunk,
        embedding: Embedding,
        _doc: &Document,
    ) -> Result<WriteResult, IngestError> {
        let mut engine = self.engine.write().await;

        let rid = engine
            .insert_record_from_f32_ns(&embedding.values, self.ns)
            .map_err(|e| IngestError::Writer(e.to_string()))?;

        engine.reranker_insert(rid, &chunk.text);

        let chunk_node_id = engine
            .create_node_for_record(Some(rid), 1, self.ns)
            .unwrap_or(0);
        if chunk_node_id > 0 {
            let _ = engine.create_edge(self.doc_node_id, chunk_node_id, 6);
        }

        let now = now_unix();
        let _ = engine.set_meta_audited(
            format!("record:{rid}"),
            serde_json::json!({
                "text":             chunk.text,
                "source":           self.source,
                "chunk_index":      chunk.index,
                "section_title":    chunk.title,
                "document_node_id": self.doc_node_id,
                "chunk_node_id":    chunk_node_id,
                "collection":       self.collection,
                "chunk_mode":       self.strategy_used,
                "ingested_at":      now,
                "embed_model":      embedding.model_id,
            }),
        );

        Ok(WriteResult {
            record_id: rid.to_string(),
            chunk_node_id: if chunk_node_id > 0 { Some(chunk_node_id) } else { None },
        })
    }
}

fn now_unix() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}
