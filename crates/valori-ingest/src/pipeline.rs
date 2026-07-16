// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`IngestPipeline`] — observable, configurable ingest orchestrator.
//!
//! # Stage order
//! ```text
//! Reader → [Validator] → Chunker → Embedder (batched, retried) → Writer
//! ```
//!
//! # Backward compatibility
//! [`IngestPipeline::run()`] still returns `Result<Vec<WriteResult>, IngestError>`.
//! All new behaviour is in [`IngestPipeline::run_observed()`].

use std::time::Instant;

use crate::cancel::CancellationToken;
use crate::chunker::Chunker;
use crate::config::PipelineConfig;
use crate::document::{Chunk, Embedding, IngestError, WriteResult};
use crate::embedder::Embedder;
use crate::execution::{
    now_unix_ms, PipelineResult, StageName, StageMetrics, StageResult,
};
use crate::hooks::PipelineHook;
use crate::progress::{send, ProgressEvent, ProgressSender};
use crate::reader::Reader;
use crate::validator::DocumentValidator;
use crate::writer::Writer;

/// Orchestrates Reader → [Validator] → Chunker → Embedder → Writer.
///
/// Constructed via [`IngestPipeline::builder()`]. `run()` is the original
/// API; `run_observed()` adds full execution telemetry.
pub struct IngestPipeline {
    reader:    Box<dyn Reader>,
    chunker:   Box<dyn Chunker>,
    embedder:  Box<dyn Embedder>,
    writer:    Box<dyn Writer>,
    config:    PipelineConfig,
    hooks:     Vec<Box<dyn PipelineHook>>,
    validator: Option<DocumentValidator>,
}

impl IngestPipeline {
    pub fn builder() -> IngestPipelineBuilder {
        IngestPipelineBuilder::default()
    }

    // ── Backward-compatible entry point ───────────────────────────────────────

    /// Process `input` end-to-end. Returns one [`WriteResult`] per chunk written.
    ///
    /// Delegates to `run_observed`; existing callers do not need to change.
    pub async fn run(
        &mut self,
        input: &str,
        source: Option<&str>,
    ) -> Result<Vec<WriteResult>, IngestError> {
        self.run_observed(input, source, None, None).await.map(|r| r.writes)
    }

    // ── Observable entry point ────────────────────────────────────────────────

    /// Run the pipeline and return a full [`PipelineResult`] with per-stage
    /// timing, metrics, and warnings.
    ///
    /// # Parameters
    /// - `progress` — optional channel; events are sent between stages.
    ///   The pipeline never blocks if the channel is full.
    /// - `cancel` — optional cancellation token; checked between each stage.
    pub async fn run_observed(
        &mut self,
        input: &str,
        source: Option<&str>,
        progress: Option<&ProgressSender>,
        cancel: Option<&CancellationToken>,
    ) -> Result<PipelineResult, IngestError> {
        let pipeline_start = Instant::now();
        let mut stages: Vec<StageResult> = Vec::new();

        macro_rules! check_cancel {
            () => {
                if let Some(tok) = cancel {
                    tok.check()?;
                }
            };
        }

        // ── Reader ────────────────────────────────────────────────────────────
        send(&progress, ProgressEvent::StageStarted { stage: StageName::Reader }).await;
        let t = Instant::now();
        let started = now_unix_ms();
        let doc = match self.reader.read(input, source).await {
            Ok(d) => d,
            Err(e) => {
                let msg = e.to_string();
                send(&progress, ProgressEvent::Failed {
                    stage: StageName::Reader, error: msg.clone(),
                }).await;
                return Err(e);
            }
        };
        let dur = t.elapsed().as_millis() as u64;
        stages.push(StageResult {
            stage: StageName::Reader,
            started_at_ms: started,
            duration_ms: dur,
            success: true,
            warnings: vec![],
            metrics: StageMetrics::Reader {
                bytes_read: doc.content.len(),
                mime: doc.mime_type.clone(),
            },
            error: None,
        });
        send(&progress, ProgressEvent::StageCompleted { stage: StageName::Reader, duration_ms: dur }).await;
        for h in &self.hooks { h.after_read(&doc); }
        check_cancel!();

        // ── Validator (optional) ──────────────────────────────────────────────
        if let Some(ref v) = self.validator {
            send(&progress, ProgressEvent::StageStarted { stage: StageName::Validator }).await;
            let t = Instant::now();
            let started = now_unix_ms();
            let warnings: Vec<String> = vec![];
            match v.validate(&doc) {
                Ok(()) => {}
                Err(e) => {
                    let msg = e.to_string();
                    send(&progress, ProgressEvent::Failed {
                        stage: StageName::Validator, error: msg.clone(),
                    }).await;
                    stages.push(StageResult {
                        stage: StageName::Validator,
                        started_at_ms: started,
                        duration_ms: t.elapsed().as_millis() as u64,
                        success: false,
                        warnings: vec![],
                        metrics: StageMetrics::Validator { checks_run: 1, warnings: vec![] },
                        error: Some(msg.clone()),
                    });
                    return Err(IngestError::Validation(msg));
                }
            }
            let dur = t.elapsed().as_millis() as u64;
            stages.push(StageResult {
                stage: StageName::Validator,
                started_at_ms: started,
                duration_ms: dur,
                success: true,
                warnings: warnings.clone(),
                metrics: StageMetrics::Validator { checks_run: 5, warnings: warnings },
                error: None,
            });
            send(&progress, ProgressEvent::StageCompleted { stage: StageName::Validator, duration_ms: dur }).await;
            check_cancel!();
        }

        // ── Chunker ───────────────────────────────────────────────────────────
        for h in &self.hooks { h.before_chunk(&doc); }
        send(&progress, ProgressEvent::StageStarted { stage: StageName::Chunker }).await;
        let t = Instant::now();
        let started = now_unix_ms();
        let chunks: Vec<Chunk> = self.chunker.chunk(&doc);
        let dur = t.elapsed().as_millis() as u64;
        let avg_chunk_bytes = if chunks.is_empty() { 0 }
            else { chunks.iter().map(|c| c.text.len()).sum::<usize>() / chunks.len() };
        let max_chunk_bytes = chunks.iter().map(|c| c.text.len()).max().unwrap_or(0);
        stages.push(StageResult {
            stage: StageName::Chunker,
            started_at_ms: started,
            duration_ms: dur,
            success: true,
            warnings: if chunks.is_empty() { vec!["no chunks produced".into()] } else { vec![] },
            metrics: StageMetrics::Chunker { chunks_created: chunks.len(), avg_chunk_bytes, max_chunk_bytes },
            error: None,
        });
        send(&progress, ProgressEvent::StageCompleted { stage: StageName::Chunker, duration_ms: dur }).await;
        for h in &self.hooks { h.after_chunk(&chunks); }
        check_cancel!();

        if chunks.is_empty() {
            let total_ms = pipeline_start.elapsed().as_millis() as u64;
            return Ok(PipelineResult {
                document_id: doc.id.clone(),
                document_source: doc.source.clone(),
                document_mime: doc.mime_type.clone(),
                stages,
                writes: vec![],
                chunks_produced: 0,
                records_written: 0,
                total_duration_ms: total_ms,
                success: true,
                error: None,
            });
        }

        // ── Embedder + Writer (batched streaming) ─────────────────────────────
        // Embed in batches of config.batch_size; write each batch immediately.
        // This bounds peak memory for large documents. E4.6 / E4.7.
        let batch_size = self.config.batch_size.max(1);
        let total_chunks = chunks.len();
        let mut writes: Vec<WriteResult> = Vec::with_capacity(total_chunks);
        let mut embed_batch_count: usize = 0;
        let mut embed_total_dims: usize = 0;
        let mut embed_latency_ms: u64 = 0;
        let mut embed_model_id: String = String::new();
        let embed_start = now_unix_ms();
        let embed_t = Instant::now();
        let write_start = now_unix_ms();
        let write_t = Instant::now();

        send(&progress, ProgressEvent::StageStarted { stage: StageName::Embedder }).await;

        let mut completed_chunks: usize = 0;
        for batch in chunks.chunks(batch_size) {
            check_cancel!();
            for h in &self.hooks { h.before_embed(batch); }

            let batch_t = Instant::now();
            let embeddings: Vec<Embedding> = self.config.retry
                .execute(|| self.embedder.embed(batch))
                .await
                .map_err(|e| {
                    let msg = e.to_string();
                    IngestError::Embed(msg)
                })?;

            embed_latency_ms += batch_t.elapsed().as_millis() as u64;
            embed_batch_count += 1;
            if let Some(first) = embeddings.first() {
                embed_total_dims = first.dimensions;
                if embed_model_id.is_empty() {
                    embed_model_id = first.model_id.clone();
                }
            }
            for h in &self.hooks { h.after_embed(&embeddings); }

            for (chunk, embedding) in batch.iter().zip(embeddings.into_iter()) {
                let result = self.writer.write(chunk, embedding, &doc).await
                    .map_err(|e| IngestError::Writer(e.to_string()))?;
                for h in &self.hooks { h.after_write(&result.record_id); }
                writes.push(result);
                completed_chunks += 1;
                send(&progress, ProgressEvent::ChunkProgress {
                    completed: completed_chunks, total: total_chunks,
                }).await;
            }
        }

        // Record embedder stage. `model_id` is always "{provider}/{model}"
        // (ModelProviderEmbedder's convention — see embedder.rs); split once
        // rather than assuming a provider never contains '/'.
        let (embed_provider, embed_model) = match embed_model_id.split_once('/') {
            Some((p, m)) => (p.to_string(), m.to_string()),
            None => (String::new(), embed_model_id.clone()),
        };
        let embed_dur = embed_t.elapsed().as_millis() as u64;
        stages.push(StageResult {
            stage: StageName::Embedder,
            started_at_ms: embed_start,
            duration_ms: embed_dur,
            success: true,
            warnings: vec![],
            metrics: StageMetrics::Embedder {
                batch_count: embed_batch_count,
                dimensions: embed_total_dims,
                latency_ms: embed_latency_ms,
                provider: embed_provider,
                model: embed_model,
            },
            error: None,
        });
        send(&progress, ProgressEvent::StageCompleted { stage: StageName::Embedder, duration_ms: embed_dur }).await;

        // Record writer stage
        let graph_nodes_created = writes.iter().filter(|w| w.chunk_node_id.is_some()).count();
        let write_dur = write_t.elapsed().as_millis() as u64;
        stages.push(StageResult {
            stage: StageName::Writer,
            started_at_ms: write_start,
            duration_ms: write_dur,
            success: true,
            warnings: vec![],
            metrics: StageMetrics::Writer {
                records_written: writes.len(),
                graph_nodes_created,
                graph_edges_created: graph_nodes_created,
            },
            error: None,
        });
        send(&progress, ProgressEvent::StageCompleted { stage: StageName::Writer, duration_ms: write_dur }).await;

        let total_ms = pipeline_start.elapsed().as_millis() as u64;
        let records = writes.len();
        send(&progress, ProgressEvent::Done {
            records_written: records,
            chunks_produced: total_chunks,
            total_duration_ms: total_ms,
        }).await;

        Ok(PipelineResult {
            document_id: doc.id,
            document_source: doc.source,
            document_mime: doc.mime_type,
            stages,
            writes,
            chunks_produced: total_chunks,
            records_written: records,
            total_duration_ms: total_ms,
            success: true,
            error: None,
        })
    }
}

// ── Builder ───────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct IngestPipelineBuilder {
    reader:    Option<Box<dyn Reader>>,
    chunker:   Option<Box<dyn Chunker>>,
    embedder:  Option<Box<dyn Embedder>>,
    writer:    Option<Box<dyn Writer>>,
    config:    Option<PipelineConfig>,
    hooks:     Vec<Box<dyn PipelineHook>>,
    validator: Option<DocumentValidator>,
}

impl IngestPipelineBuilder {
    pub fn reader(mut self, r: impl Reader + 'static) -> Self {
        self.reader = Some(Box::new(r)); self
    }
    pub fn chunker(mut self, c: impl Chunker + 'static) -> Self {
        self.chunker = Some(Box::new(c)); self
    }
    pub fn embedder(mut self, e: impl Embedder + 'static) -> Self {
        self.embedder = Some(Box::new(e)); self
    }
    pub fn writer(mut self, w: impl Writer + 'static) -> Self {
        self.writer = Some(Box::new(w)); self
    }
    pub fn config(mut self, c: PipelineConfig) -> Self {
        self.config = Some(c); self
    }
    pub fn hook(mut self, h: impl PipelineHook + 'static) -> Self {
        self.hooks.push(Box::new(h)); self
    }
    pub fn validator(mut self, v: DocumentValidator) -> Self {
        self.validator = Some(v); self
    }

    /// Panics if any required stage (reader / chunker / embedder / writer) is missing.
    pub fn build(self) -> IngestPipeline {
        IngestPipeline {
            reader:    self.reader.expect("IngestPipeline requires a Reader"),
            chunker:   self.chunker.expect("IngestPipeline requires a Chunker"),
            embedder:  self.embedder.expect("IngestPipeline requires an Embedder"),
            writer:    self.writer.expect("IngestPipeline requires a Writer"),
            config:    self.config.unwrap_or_default(),
            hooks:     self.hooks,
            validator: self.validator,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunker::DefaultChunker;
    use crate::config::PipelineConfig;
    use crate::embedder::ModelProviderEmbedder;
    use crate::execution::StageName;
    use crate::reader::TextReader;
    use crate::writer::NoopWriter;
    use valori_models::{ModelError, ModelProvider};

    struct ZeroProvider;

    #[async_trait::async_trait]
    impl ModelProvider for ZeroProvider {
        fn kind(&self) -> &'static str { "zero" }
        fn model_name(&self) -> &str { "zero" }
        fn dim(&self) -> usize { 3 }
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ModelError> {
            Ok(texts.iter().map(|_| vec![0.0; 3]).collect())
        }
        async fn health(&self) -> Result<(), ModelError> { Ok(()) }
    }

    fn build() -> IngestPipeline {
        IngestPipeline::builder()
            .reader(TextReader)
            .chunker(DefaultChunker::default())
            .embedder(ModelProviderEmbedder::new(Box::new(ZeroProvider)))
            .writer(NoopWriter)
            .build()
    }

    // ── run() backward compat ────────────────────────────────────────────────

    #[tokio::test]
    async fn pipeline_produces_one_result_per_chunk() {
        let mut p = build();
        let results = p.run("# Sec A\nContent.\n## Sec B\nMore.", Some("test.md")).await.unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().all(|r| r.record_id.starts_with("noop-")));
    }

    #[tokio::test]
    async fn pipeline_empty_input_does_not_panic() {
        let mut p = build();
        let _ = p.run("", None).await.unwrap();
    }

    #[tokio::test]
    async fn builder_pattern_composes_correctly() {
        let mut p = IngestPipeline::builder()
            .reader(TextReader)
            .chunker(DefaultChunker::new("fixed", 200, 50))
            .embedder(ModelProviderEmbedder::new(Box::new(ZeroProvider)))
            .writer(NoopWriter)
            .build();
        let results = p
            .run("This is a long enough sentence that the fixed chunker will include it.", Some("doc.txt"))
            .await.unwrap();
        assert!(!results.is_empty());
    }

    // ── run_observed ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn observed_result_contains_all_stages() {
        let mut p = build();
        let result = p
            .run_observed("This is a sufficiently long piece of text that the fixed chunker will keep it as a chunk.", Some("doc.md"), None, None)
            .await.unwrap();
        assert!(result.success);
        assert!(result.stage(&StageName::Reader).is_some());
        assert!(result.stage(&StageName::Chunker).is_some());
        assert!(result.stage(&StageName::Embedder).is_some());
        assert!(result.stage(&StageName::Writer).is_some());
        assert_eq!(result.records_written, result.writes.len());
    }

    #[tokio::test]
    async fn observed_result_summary() {
        let mut p = build();
        let r = p.run_observed("Some long content here.", None, None, None).await.unwrap();
        assert!(r.summary().starts_with("ok "));
    }

    // ── Cancellation ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn cancelled_before_run_returns_error() {
        let mut p = build();
        let token = CancellationToken::new();
        token.cancel();
        let err = p
            .run_observed("hello world", None, None, Some(&token))
            .await
            .unwrap_err();
        assert!(matches!(err, IngestError::Cancelled));
    }

    // ── Progress ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn progress_channel_receives_events() {
        use tokio::sync::mpsc;
        let (tx, mut rx) = mpsc::channel(32);
        let mut p = build();
        let _ = p
            .run_observed("This long body contains enough characters that the chunker will keep it.", None, Some(&tx), None)
            .await.unwrap();
        drop(tx);
        let mut events: Vec<ProgressEvent> = vec![];
        while let Some(e) = rx.recv().await { events.push(e); }
        assert!(!events.is_empty());
        // Last event is Done
        assert!(matches!(events.last().unwrap(), ProgressEvent::Done { .. }));
    }

    // ── PipelineConfig / batch_size streaming ─────────────────────────────────

    #[tokio::test]
    async fn batch_size_1_produces_same_writes_as_default() {
        let text = "# A\nHello world content.\n## B\nMore content here.\n";
        let mut p_default = build();
        let mut p_stream = IngestPipeline::builder()
            .reader(TextReader)
            .chunker(DefaultChunker::default())
            .embedder(ModelProviderEmbedder::new(Box::new(ZeroProvider)))
            .writer(NoopWriter)
            .config(PipelineConfig::default().with_batch_size(1))
            .build();
        let r1 = p_default.run(text, None).await.unwrap();
        let r2 = p_stream.run(text, None).await.unwrap();
        assert_eq!(r1.len(), r2.len());
    }

    // ── Hooks ─────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn hook_fires_before_chunk() {
        use std::sync::{Arc, Mutex};
        use crate::hooks::PipelineHook;

        struct Spy(Arc<Mutex<bool>>);
        impl PipelineHook for Spy {
            fn before_chunk(&self, _: &crate::document::Document) {
                *self.0.lock().unwrap() = true;
            }
        }

        let fired = Arc::new(Mutex::new(false));
        let mut p = IngestPipeline::builder()
            .reader(TextReader)
            .chunker(DefaultChunker::default())
            .embedder(ModelProviderEmbedder::new(Box::new(ZeroProvider)))
            .writer(NoopWriter)
            .hook(Spy(fired.clone()))
            .build();
        p.run("# X\nLong content.", None).await.unwrap();
        assert!(*fired.lock().unwrap());
    }

    // ── Validator wired in pipeline ───────────────────────────────────────────

    #[tokio::test]
    async fn validator_rejects_empty_document() {
        use crate::validator::DocumentValidator;
        let mut p = IngestPipeline::builder()
            .reader(TextReader)
            .chunker(DefaultChunker::default())
            .embedder(ModelProviderEmbedder::new(Box::new(ZeroProvider)))
            .writer(NoopWriter)
            .validator(DocumentValidator::new())
            .build();
        let err = p.run("   ", None).await.unwrap_err();
        assert!(matches!(err, IngestError::Validation(_)));
    }
}
