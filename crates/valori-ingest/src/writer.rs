// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`Writer`] trait — the final stage of the ingest pipeline.
//!
//! `valori-ingest` defines the contract; implementations live where they have
//! access to the kernel:
//! - `KernelWriter` in `valori-node` — writes to the in-process `KernelState`
//! - `RemoteWriter` (future) — HTTP inserts to a remote node
//! - `NoopWriter` — test helper, discards output

use crate::document::{Chunk, Document, Embedding, IngestError, WriteResult};

/// Persists one chunk + its embedding. Returns a [`WriteResult`].
///
/// Stateful: implementations hold a handle to the target store.
/// The pipeline drives `write()` once per chunk in order.
#[async_trait::async_trait]
pub trait Writer: Send + Sync {
    async fn write(
        &mut self,
        chunk: &Chunk,
        embedding: Embedding,
        doc: &Document,
    ) -> Result<WriteResult, IngestError>;
}

/// Discards all output. Used in tests where the write side is irrelevant.
pub struct NoopWriter;

#[async_trait::async_trait]
impl Writer for NoopWriter {
    async fn write(
        &mut self,
        chunk: &Chunk,
        _embedding: Embedding,
        _doc: &Document,
    ) -> Result<WriteResult, IngestError> {
        Ok(WriteResult {
            record_id: format!("noop-{}", chunk.index),
            chunk_node_id: None,
        })
    }
}
