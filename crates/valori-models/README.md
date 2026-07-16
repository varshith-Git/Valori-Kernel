# valori-models

Model management subsystem for Valori. Shared by the daemon, `valori-ingest`,
the Python SDK, and the desktop — single implementation, no duplication.

## What it does

- **Registry** — curated catalog of 11 models (OpenAI, Ollama, Voyage, BGE-ONNX)
- **PackageStore** (M5) — on-disk `<task>/<sanitized-id>/manifest.json` layout; atomic install (`.tmp/` → verify → rename); exclusive `InstallLock`; per-package versioned `PackageManifest`
- **Install / remove** — remote-service models register instantly; local ONNX models stream-download with SHA-256 verification
- **Verify** — on-demand `VerifyStatus` (`Remote | Ok | Missing | Unverified | Corrupted`)
- **IntegrityManager** (M6) — `verify(id)`, `verify_all()`, `repair()` with `RepairAction` outcome
- **GarbageCollector + RefCounter** (M6.1–M6.2) — scan unreferenced packages, report reclaimable bytes, `safe_delete` guards against in-use models
- **SystemHealth** (M6.3) — per-package health report (Verified / Installed / Missing / Corrupted + ref count + size); surfaced at `GET /v1/models/health`
- **Provider trait** — `ModelProvider::embed()` + `health()` for all remote providers; ONNX deferred to M10

## Providers

| Provider | ID prefix | Notes |
|---|---|---|
| Ollama | `ollama/` | `/api/embed` batch; falls back to `/api/embeddings` legacy |
| OpenAI | `openai/` | OpenAI-compatible; `VALORI_EMBED_URL` overrides base URL |
| Voyage | `voyage/` | `api.voyageai.com/v1/embeddings` |
| Dummy | `dummy` | Zero vectors — tests only |
| ONNX | `onnx/` | Stub — returns error until E1-full lands |

## Usage

```rust
use valori_models::{ModelManager, JsonModelStore};

let store = Box::new(JsonModelStore::open(&home)?);
let mut mgr = ModelManager::new(&home, store)?;

mgr.install("openai/text-embedding-3-small").await?;

let provider = mgr.provider("openai/text-embedding-3-small")?;
let embeddings = provider.embed(&["hello world".into()]).await?;
```

## Directory layout (PackageStore)

```
$VALORI_MODELS_DIR/                  # default: ~/.valori/models
  .locks/                            # RAII install locks
  .tmp/                              # staging (cleaned on open)
  embedding/
    bge-small-en-v1/
      manifest.json                  # PackageManifest (schema_version=1)
  reranker/
    bge-reranker-base/
      manifest.json
```

## Environment variables

| Var | Used by |
|---|---|
| `VALORI_EMBED_URL` | Ollama / OpenAI base URL override |
| `VALORI_EMBED_API_KEY` | OpenAI / Voyage API key |
| `VALORI_MODELS_DIR` | Override package store root (default: `~/.valori/models`) |
