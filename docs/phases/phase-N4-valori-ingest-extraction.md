# Phase N4 — valori-ingest extraction

## Goal

Extract embedding and chunking logic from `valori-node` into a standalone `crates/valori-ingest/` crate with zero `valori-*` dependencies. The stateless `POST /v1/ingest/document` handler moves into the crate; engine-coupled ingest handlers remain in `valori-node/src/ingest.rs`.

## Delivered

| File | Change |
|---|---|
| `crates/valori-ingest/` | New crate created |
| `crates/valori-ingest/Cargo.toml` | Package manifest; deps: serde, serde_json, blake3, reqwest, axum, tracing; dev-deps: tokio |
| `crates/valori-ingest/src/lib.rs` | Public surface: `embed`, `chunker`, `handler` modules + flat re-exports |
| `crates/valori-ingest/src/embed.rs` | `EmbedConfig`, `EmbedError`, `embed_batch` — moved from `embedder.rs` |
| `crates/valori-ingest/src/chunker.rs` | `IngestChunk`, `chunk_document`, `chunk_content_hash`, 4 strategies, `detect_strategy`, `MAX_INGEST_TEXT_BYTES` |
| `crates/valori-ingest/src/handler.rs` | `IngestDocumentRequest`, `IngestDocumentResponse`, `ingest_document` stateless axum handler |
| `crates/valori-ingest/README.md` | Full README with module table, usage, strategy table, SOLID notes |
| `Cargo.toml` (workspace root) | Added `valori-ingest` to members + default-members + workspace.dependencies |
| `crates/valori-node/Cargo.toml` | Added `valori-ingest = { workspace = true }` |
| `crates/valori-node/src/embedder.rs` | **Deleted** — all content moved to valori-ingest |
| `crates/valori-node/src/ingest.rs` | **Rewritten** — stripped moved code; imports `valori_ingest::{embed_batch, chunk_document, chunk_content_hash}` and `valori_ingest::chunker::MAX_INGEST_TEXT_BYTES`; retains `ingest`, `ingest_update`, `get_ingest_status`, `collect_old_chunks` |
| `crates/valori-node/src/engine.rs` | `embed_config: Option<valori_ingest::EmbedConfig>`; added `embed_config_from_node(cfg)` helper (pub(crate)) |
| `crates/valori-node/src/lib.rs` | Removed `pub mod embedder;`; updated comment on `pub mod ingest` |
| `crates/valori-node/src/server.rs` | `crate::ingest::ingest_document` → `valori_ingest::ingest_document`; embed path references updated |
| `crates/valori-node/src/cluster_server.rs` | Same replacements; `EmbedConfig::from_node_config` → `crate::engine::embed_config_from_node` |
| `crates/valori-node/src/capabilities.rs` | `use crate::embedder::{...}` → `use valori_ingest::{...}` |

## Findings

- **Recursive stack overflow in chunker**: `tree` strategy falling back to `"auto"` could re-detect `"tree"` on the same text → infinite recursion → SIGABRT. Fixed by falling back directly to `"fixed"` when tree produces fewer than 2 chunks. Strategy label updated from `"tree->auto"` to `"tree->fixed"`.
- **`EmbedConfig::from_node_config` placement**: `NodeConfig` lives in `valori-node`, so the constructor cannot move to `valori-ingest` (would be circular). Solved by adding `embed_config_from_node(cfg: &NodeConfig)` as a `pub(crate)` helper in `engine.rs`.
- **`chunk_document` / `chunk_content_hash` still referenced in cluster_server.rs**: These were ingest_update helpers still on `crate::ingest::` — updated to `valori_ingest::` by sed pass.

## Validation

```
cargo build -p valori-ingest -p valori-node   # 0 errors
cargo test -p valori-ingest                    # 12 passed, 0 failed
cargo test -p valori-node                      # 2 passed, 0 failed
```

## Follow-ups

- **Phase N5 (valori-engine)**: Extract `Engine` itself into a standalone crate, completing the decomposition. Depends on N1–N4 all landing cleanly.
- The `ingest` and `ingest_update` handlers in `ingest.rs` still call engine methods directly via `SharedEngine` write lock — these will become trait calls in N5 when `Engine` moves out.
