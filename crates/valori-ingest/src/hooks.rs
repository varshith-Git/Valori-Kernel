// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`PipelineHook`] — E4.4.
//!
//! Hooks observe pipeline stage transitions without modifying data.
//! Plugins, telemetry collectors, and audit loggers implement this trait.
//! All methods have default no-op implementations.

use crate::document::{Chunk, Document, Embedding};

/// Observer called at stage boundaries during pipeline execution.
///
/// Hooks are synchronous — they must not block on I/O. For async work,
/// use `tokio::spawn` internally or use the progress channel (E4.2).
pub trait PipelineHook: Send + Sync {
    /// Called after the reader produced a document, before validation.
    fn after_read(&self, _doc: &Document) {}
    /// Called after validation passed, before chunking.
    fn before_chunk(&self, _doc: &Document) {}
    /// Called after chunking, before embedding.
    fn after_chunk(&self, _chunks: &[Chunk]) {}
    /// Called before the embedder runs on a batch.
    fn before_embed(&self, _batch: &[Chunk]) {}
    /// Called after a batch is embedded, before writing.
    fn after_embed(&self, _embeddings: &[Embedding]) {}
    /// Called after each write completes.
    fn after_write(&self, _record_id: &str) {}
}

/// No-op hook — useful as a default and in tests.
pub struct NoopHook;
impl PipelineHook for NoopHook {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use std::sync::{Arc, Mutex};

    struct CountingHook {
        count: Arc<Mutex<usize>>,
    }
    impl PipelineHook for CountingHook {
        fn before_chunk(&self, _doc: &Document) {
            *self.count.lock().unwrap() += 1;
        }
    }

    #[test]
    fn hook_called() {
        let count = Arc::new(Mutex::new(0usize));
        let hook = CountingHook {
            count: count.clone(),
        };
        let doc = Document::from_text("hello", Some("test"));
        hook.before_chunk(&doc);
        assert_eq!(*count.lock().unwrap(), 1);
    }

    #[test]
    fn noop_hook_compiles() {
        let doc = Document::from_text("x", None);
        NoopHook.after_read(&doc);
        NoopHook.before_chunk(&doc);
    }
}
