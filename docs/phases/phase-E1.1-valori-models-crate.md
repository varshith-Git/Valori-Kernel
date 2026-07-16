# Phase E1.1 — `valori-models` Standalone Crate

## Goal

Extract model management from `valori-daemon` into a dedicated, reusable
`valori-models` crate so that the daemon, `valori-ingest`, the Python SDK,
and the desktop all share one implementation without duplication.
The daemon stays a pure orchestrator; business logic lives in a focused crate.

## Delivered

| File | Change |
|---|---|
| `crates/valori-models/Cargo.toml` | New crate — `sha2`, `reqwest` (rustls-tls, stream), `futures-util`, `tokio`, `async-trait`, `serde_json`, `tracing` |
| `crates/valori-models/src/lib.rs` | `ModelManager` struct + re-exports; `catalog()`, `get()`, `spec()`, `install()`, `verify()`, `remove()`, `provider()` |
| `crates/valori-models/src/error.rs` | `ModelError` / `ModelResult` — `NotFound`, `AlreadyExists`, `Download`, `Verify`, `Provider`, `Io`, `Json` |
| `crates/valori-models/src/registry/mod.rs` | `ModelSpec` + `Download`; built-in registry of 11 models (OpenAI ×3, Ollama ×3, Voyage ×2, ONNX ×3) |
| `crates/valori-models/src/storage/mod.rs` | `ModelStore` trait (DIP seam) + `InstalledModel`; `JsonModelStore` (write-then-rename, CRUD) |
| `crates/valori-models/src/downloader/mod.rs` | Streaming SHA-256-verified download; `sha256_hex` helper |
| `crates/valori-models/src/verifier/mod.rs` | `VerifyStatus` (`Remote|Ok|Missing|Unverified|Corrupted`) + `VerifyResult`; `verify_model()` |
| `crates/valori-models/src/provider/mod.rs` | `ModelProvider` trait (`kind`, `model_name`, `dim`, `embed`, `health`); `build()` factory |
| `crates/valori-models/src/provider/dummy.rs` | `DummyProvider` — zero vectors, tokio test |
| `crates/valori-models/src/provider/ollama.rs` | `OllamaProvider` — `/api/embed` batch + `/api/embeddings` legacy fallback |
| `crates/valori-models/src/provider/openai.rs` | `OpenAIProvider` — batch, reads `VALORI_EMBED_API_KEY` / `VALORI_EMBED_URL` |
| `crates/valori-models/src/provider/voyage.rs` | `VoyageProvider` — `api.voyageai.com/v1/embeddings` |
| `Cargo.toml` (workspace root) | Added `valori-models` to `members`; added workspace.dependencies entries for `valori-search`, `valori-index`, `valori-rag`, `valori-ingest`, `valori-engine`, `valori-daemon`, `valori-models` |

## Design

- **`ModelProvider` trait** is `async-trait` object-safe; `build()` dispatches
  by `InstalledModel::provider` string (`"ollama"` / `"openai"` / `"voyage"` /
  `"dummy"`). `"onnx"` is a deferred stub that returns a clear error.
- **`ModelStore` DIP seam** — `JsonModelStore` today; `SqliteModelStore` drops
  in later with no `ModelManager` change.
- **Two install paths** — remote-service models (register instantly, no file),
  local ONNX (streaming download + SHA-256 verify).
- **No daemon leakage** — the daemon depends on this crate; not the reverse.

## Findings

- The workspace `[workspace.dependencies]` table was missing seven crates added
  in prior N/E sessions; the build would fail immediately without fixing it.
  Fixed by adding all missing entries to the workspace manifest.
- ONNX inference is a stub (`"onnx"` → clear error message) — deferred to
  E1-full when the ONNX runtime crate is chosen.

## Validation

```
cargo build -p valori-models   → Finished (0 errors, 0 warnings)
cargo test -p valori-models    → 5 passed, 0 failed

  downloader::tests::sha256_known_vector          ok
  provider::dummy::tests::dummy_returns_zero_vectors  ok
  storage::tests::crud_roundtrip                  ok
  verifier::tests::remote_model_is_remote         ok
  verifier::tests::local_model_ok_and_corrupted   ok
```

## E1.2 — Daemon migration (done in this phase)

- Deleted `crates/valori-daemon/src/model.rs` (351 lines — all duplicated).
- Removed `ModelStore` trait from `daemon/src/store.rs` (now lives in `valori-models`).
- Added `From<valori_models::ModelError> for DaemonError` bridge in `error.rs`.
- Updated `daemon.rs` to import `valori_models::{InstalledModel, JsonModelStore, ModelManager, ModelStore}`.
- Removed `sha2` and `futures-util` from daemon's `Cargo.toml` (no longer needed there).
- Removed `pub mod model` and re-exports from `lib.rs`.
- **Result**: `cargo build -p valori-daemon` clean; 11/11 unit tests pass.
  The 1 e2e failure (`supervisor_restarts_crashed_node`) is pre-existing — requires a live `valori-node` binary.

## Follow-ups

| Item | Milestone |
|---|---|
| Wire `valori-ingest` embedder stage to use `ModelProvider` | E2/E2.1 |
| ONNX local inference (`ort`/candle) | E1-full |
| Provider credential management (API keys per provider) | later |
