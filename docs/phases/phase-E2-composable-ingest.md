# Phase E2 — Composable Ingest Pipeline

## Goal

Make ingest composable, not just linear. Introduce four independent traits
(`Reader`, `Chunker`, `Embedder`, `Writer`) and the `Document` shared object so
that every stage owns exactly one concern and new formats, providers, or targets
slot in by implementing one trait — not by touching the pipeline.

No new features, no new formats, no new providers. Behavior is identical to
before; only the architecture changed.

## Delivered

| File | Change |
|---|---|
| `src/document.rs` | New — `Document` (BLAKE3 id, source, mime_type, metadata, content); `Chunk` (id, index, title, text, extensible metadata); `Embedding` (chunk_id + values); `WriteResult` (record_id); `IngestError` enum (`Reader` / `Chunk` / `Embed` / `Writer` variants) |
| `src/reader.rs` | New — `trait Reader` + `TextReader` (plain text → `Document`) |
| `src/chunker.rs` | Extended — `trait Chunker` (returns `Vec<Chunk>`) + `DefaultChunker` (named for behavior, not brand); converts `IngestChunk` → `Chunk` internally |
| `src/embedder.rs` | New — `trait Embedder` (takes `&[Chunk]`, returns `Vec<Embedding>`) + `ModelProviderEmbedder` (`Box<dyn ModelProvider>`; zero Ollama/OpenAI awareness) |
| `src/writer.rs` | New — `trait Writer` (takes `&Chunk, Embedding`, returns `WriteResult`) + `NoopWriter` (test helper) |
| `src/pipeline.rs` | New — `IngestPipeline::builder()` with fluent API; `run()` uses typed objects throughout |
| `src/lib.rs` | Re-exports all new types; backward-compat re-exports unchanged |
| `Cargo.toml` | Added `async-trait`, `valori-models` |
| `embed.rs` | Untouched — node still uses `embed_batch` directly (migration in E2.5) |
| `handler.rs` | Untouched — stateless axum handler unchanged |

## Design

- **`Document` as the shared object.** Readers produce it; all downstream stages
  read from it. Format changes are local to the reader — chunker, embedder, and
  writer never see format differences.
- **`Writer` defined here, implemented elsewhere.** `valori-ingest` can't import
  `valori-engine`/`valori-kernel` (wrong direction in the dep graph). `KernelWriter`
  lives in `valori-node`; `RemoteWriter` could live anywhere. `NoopWriter` covers tests.
- **`ModelProviderEmbedder` does not know about Ollama, OpenAI, or Voyage.** It
  calls `Box<dyn ModelProvider>`. The provider selection happens at call site.
- **`embed.rs` kept.** The node's existing embedding path (`embed_batch`) is
  unchanged. The two coexist; migrating the node to `ModelProviderEmbedder` is a
  separate step.
- **`IngestPipeline` named explicitly** — leaves room for `QueryPipeline`,
  `SearchPipeline`, `ExecutionPipeline` without collision.

## Six-question merge check

| Question | Answer |
|---|---|
| Can I delete the old handler after this? | Not yet — node still calls `embed_batch` directly (separate migration) |
| Does behavior stay identical? | Yes — no logic changed in `chunker.rs`, `embed.rs`, or `handler.rs` |
| Did benchmarks change? | No — no hot path touched |
| Did APIs change? | No |
| Did tests change? | Only architecture — new tests added, existing tests unchanged |
| Did we add any user-visible feature? | No |

## Design decisions (from exit checklist review)

- **`DefaultChunker` not `ValoriChunker`** — implementation names describe algorithm, not brand. Future chunkers (`RecursiveChunker`, `MarkdownChunker`, `TokenChunker`) slot in beside it.
- **Typed objects across all stage boundaries** — `Document → Chunk → Embedding → WriteResult` are proper structs with extensible `metadata: Value`. Adding page numbers, byte offsets, or token count later requires zero trait changes.
- **`IngestError` enum** — single hierarchy with stage variants. Retries, logging, and API error responses all key on which stage failed.
- **Builder pattern** — `IngestPipeline::builder().reader(…).chunker(…).embedder(…).writer(…).build()`. Future stages (Cleaner, PIIRedactor) add builder methods without changing existing call sites.
- **`PipelineContext` deferred** — the struct shape is clear (`doc, chunks, embeddings, config, tracing`) but wiring it into trait signatures waits until E3 (first new reader) makes it necessary.

## Validation

```
cargo build -p valori-ingest   → Finished (0 errors, 0 warnings)
cargo build -p valori-node     → Finished (0 errors, 0 warnings)
cargo test -p valori-ingest    → 19 passed, 0 failed

  embed::tests::unknown_provider_errors                 ok
  embed::tests::embed_error_display                     ok
  chunker::tests::tree_strategy_detects_sections        ok
  chunker::tests::sentence_window_produces_chunks       ok
  chunker::tests::fixed_strategy_produces_overlapping_chunks ok
  chunker::chunker_trait_tests::valori_chunker_wraps_existing_impl ok
  reader::tests::text_reader_produces_document          ok
  reader::tests::text_reader_default_source             ok
  embedder::tests::delegates_to_provider                ok
  pipeline::tests::pipeline_produces_one_id_per_chunk   ok
  pipeline::tests::pipeline_empty_input_produces_no_ids ok
  handler::tests::ingest_document_returns_chunks        ok
  handler::tests::ingest_document_rejects_oversized_text ok
  (+ 5 from chunker module)
```

## Follow-ups

| Item | Milestone |
|---|---|
| `KernelWriter` in `valori-node` — wires `IngestPipeline` to the live kernel | E2.5 |
| Migrate node's `embed_batch` call sites to `ModelProviderEmbedder` | E2.5 |
| `MarkdownReader` | E3 |
| `PdfReader` | E3 (after Markdown) |
