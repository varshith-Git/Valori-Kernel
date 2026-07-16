# Phase E2.5 — KernelWriter + IngestPipeline wiring

## Goal

Wire `KernelWriter` (the `valori-node` implementation of `valori-ingest::Writer`) into the `ingest()` HTTP handler, replacing the ~200-line `embed_batch → insert_batch_ns → nodes/edges/metadata` orchestration block with a single `IngestPipeline::run()` call. Deletion of orchestration code is the proof that the abstraction earned its place.

## Delivered

| File | Change |
|---|---|
| `crates/valori-node/src/kernel_writer.rs` | New: `KernelWriter` implements `Writer` — per-chunk vector insert, reranker index, chunk-node, parent edge, chunk metadata |
| `crates/valori-node/src/lib.rs` | Added `pub mod kernel_writer;` |
| `crates/valori-node/src/ingest.rs` | Rewrote `ingest()` handler — sync and async paths both use `IngestPipeline`; deleted ~200 lines of inline orchestration |
| `crates/valori-node/Cargo.toml` | Added `valori-models = { workspace = true }` |
| `crates/valori-models/src/provider/mod.rs` | Added `provider_from_config()` factory |
| `crates/valori-models/src/lib.rs` | Re-exported `provider_from_config` |

## Findings

- `now_unix()` was duplicated between `kernel_writer.rs` and the old handler. Kept private in each file — shared utility would require another crate dep that isn't warranted.
- The async path previously had a deeply-nested `match embed_batch → insert_batch_ns → ...` block. The `tokio::spawn` closure now builds and runs `IngestPipeline`, then sets document-level metadata and emits the receipt — identical shape to the sync path.
- `IngestErrorBody` struct was kept for `ingest_update` (still uses the old direct embed path).
- Three helpers extracted: `state_hash()`, `now_unix()`, `emit_ingest_receipt()`, `err_400()`, `err_422()` — all scoped to `ingest.rs`.

## Validation

```
cargo test -p valori-node   → 263 passed, 0 failed
cargo test -p valori-ingest → 19 passed, 0 failed
cargo build -p valori-node  → 0 errors, 0 new warnings
```

## Follow-ups

- `ingest_update()` still calls `embed_batch` directly — a future phase can migrate it to `IngestPipeline` once a `DiffChunker` or `UpdateReader` is designed.
- E3: `MarkdownReader` (first new reader type) — now that `KernelWriter` proves the adoption, E3 can be opened.
- `PipelineContext` struct — before adding any stateful readers (PDF, URL) that need per-document context passing through stages.
