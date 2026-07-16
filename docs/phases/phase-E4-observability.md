# Phase E4 — Ingest Pipeline Observability

## Goal

Make every pipeline stage observable (timing, metrics, warnings) and add cancellation, retry, configurable batching, and lifecycle hooks — without adding OCR, parallelism, or any feature beyond what callers can wire today.

## Delivered

### New files in `crates/valori-ingest/src/`

| File | What it provides |
|---|---|
| `cancel.rs` | `CancellationToken` — `Arc<AtomicBool>`, `cancel()`, `check() -> Result<(), IngestError::Cancelled>`, `Clone` |
| `config.rs` | `PipelineConfig { batch_size, retry, timeout_secs }` + builder methods; default = no-op passthrough |
| `execution.rs` | `StageName`, `StageMetrics` (per-stage counters), `StageResult`, `PipelineResult` + `summary()`, `stage()`, `all_warnings()`; `now_unix_ms()` helper |
| `hooks.rs` | `PipelineHook` trait (6 default no-op methods), `NoopHook` |
| `progress.rs` | `ProgressEvent` enum (StageStarted / ChunkProgress / StageCompleted / Done / Failed), `ProgressSender` type alias, `send()` helper |
| `retry.rs` | `RetryPolicy` (Never / Fixed / Exponential) with async `execute<F, Fut, T, E>()` |

### Updated files

| File | Change |
|---|---|
| `pipeline.rs` | Full rewrite: `IngestPipeline` gains `config`, `hooks`, `validator` fields; builder gains `.config()`, `.hook()`, `.validator()`; `run()` stays backward-compatible (calls `run_observed`); `run_observed()` records per-stage `StageResult`, sends `ProgressEvent`s, checks `CancellationToken`, batches chunks per `config.batch_size`, retries embedder |
| `document.rs` | Added `Serialize, Deserialize` derives to `WriteResult` (required by `PipelineResult`) |
| `lib.rs` | Added `pub mod` declarations and top-level re-exports for all 6 new E4 modules |

## Findings

- The fixed chunker silently drops chunks shorter than 30 chars. Tests with "hello world" style inputs will produce empty chunk lists and skip the Embedder stage without error. This is expected behavior — updated tests to use content ≥ 30 chars.
- `WriteResult` was missing `Serialize/Deserialize` despite being stored in `PipelineResult` which derives both. Added the derives.

## Validation

```
cargo test -p valori-ingest
```

**110 tests: 110 passed, 0 failed.**

Tests added in this phase (all in `pipeline.rs`):
- `pipeline_produces_one_result_per_chunk` (backward compat)
- `pipeline_empty_input_does_not_panic`
- `builder_pattern_composes_correctly`
- `observed_result_contains_all_stages`
- `observed_result_summary`
- `cancelled_before_run_returns_error`
- `progress_channel_receives_events`
- `batch_size_1_produces_same_writes_as_default`
- `hook_fires_before_chunk`
- `validator_rejects_empty_document`

## Follow-ups

- **Timeout enforcement** (`PipelineConfig::timeout_secs`) is stored but not yet wired into `run_observed`. Requires wrapping the pipeline in `tokio::time::timeout`. Deferred — no callers set it today.
- **Parallel batches**: `batch_size` currently streams batches sequentially. A future phase could run N embed batches concurrently with `FuturesUnordered`. Not requested yet.
- **`run_observed` in `valori-node`**: `ingest.rs` in the node currently calls `IngestPipeline::run()`. It could be upgraded to `run_observed()` to surface `PipelineResult` in the ingest HTTP response. Deferred to the phase that exposes execution history.
