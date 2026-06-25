## Goal

Add an on-node embedding client to valori-node so callers can POST raw text and receive fully inserted, graph-wired, audited vectors in a single HTTP call — no external embed step required from the client.

## Delivered

| File | Change |
|---|---|
| `crates/valori-node/src/embedder.rs` | New module — `EmbedConfig`, `EmbedError`, `embed_batch()` supporting Ollama (`/api/embed` with `/api/embeddings` fallback), OpenAI-compatible, and custom endpoints |
| `crates/valori-node/src/config.rs` | Four new env vars: `VALORI_EMBED_PROVIDER`, `VALORI_EMBED_MODEL`, `VALORI_EMBED_URL`, `VALORI_EMBED_API_KEY` |
| `crates/valori-node/src/engine.rs` | `embed_config: Option<EmbedConfig>` field on `Engine`; `embed_enabled: bool` + `embed_provider: Option<String>` on `EngineHealth` (surfaced via `/health`) |
| `crates/valori-node/src/ingest.rs` | `IngestRequest/Response`, `ingest()` handler — full pipeline: chunk → embed → insert → reranker_insert → doc graph node → chunk nodes + ParentOf edges → metadata sidecar per chunk |
| `crates/valori-node/src/server.rs` | `POST /v1/ingest` route (standalone only; cluster wiring is Phase I4) |
| `crates/valori-node/src/lib.rs` | `pub mod embedder;` |
| `python/valoricore/remote.py` | `SyncRemoteClient.ingest()` and `AsyncRemoteClient.ingest()` |

### Endpoint

```
POST /v1/ingest
{ "text": "...", "source": "report.pdf", "strategy": "auto",
  "collection": "default", "chunk_size": 1000, "chunk_overlap": 200 }
→ { "ok": true, "document_node_id": 42, "strategy_used": "tree",
    "chunk_count": 31, "record_ids": [1,2,...31], "collection": "default" }
→ 422 if VALORI_EMBED_PROVIDER not set (clear error message)
→ 502 if embed provider is unreachable or returns error
```

### Providers

| `VALORI_EMBED_PROVIDER` | Default URL | Notes |
|---|---|---|
| `ollama` | `http://localhost:11434` | One text per call (avoids context-window blow-up); tries `/api/embed` first, falls back to `/api/embeddings` for Ollama <0.1.36 |
| `openai` | `https://api.openai.com` | Batched; requires `VALORI_EMBED_API_KEY` |
| `custom` | `VALORI_EMBED_URL` (required) | OpenAI-compatible shape; if URL already ends in `/v1/embeddings` or `/embed`, used as-is |

### Health response additions

```json
{ ..., "embed_enabled": true, "embed_provider": "ollama" }
```

## Findings

- Ollama sends concatenated text when given a batch — one-at-a-time is mandatory to avoid context overflows on large documents.
- The 6000-char truncation per chunk keeps embedding calls fast even on models with short context windows (e.g. `nomic-embed-text` at 2048 tokens ≈ ~8000 chars).
- `chrono` is not in valori-node's dependency set — timestamps are stored as Unix seconds via `std::time::SystemTime`, not ISO 8601. UI renders as human-readable on display.
- Metadata sidecar stores `embed_model` and `embed_provider` per chunk so that future re-embedding jobs know which model was used.

## Validation

- Cargo tests: **237 passed, 0 failed** (`cargo test -p valori-kernel -p valori-node`)
- TypeScript: `npx tsc --noEmit` — no errors
- Build: `cargo build -p valori-node` — clean

## Follow-ups

- Cluster mode wiring (`POST /v1/ingest` on `cluster_server.rs`) requires passing embed config through `DataPlaneState` — deferred to Phase I4.
- Batching for Ollama (using the new `/api/embed` bulk API in Ollama 0.2+) can reduce latency for large documents; deferred.
- No retry logic on embed provider HTTP failures — a single network blip aborts the whole ingest. Retry with backoff is a future improvement.
