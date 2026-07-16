# Phase E1 (lite) — Model Manager

## Goal

Answer the first question every AI product faces — "which embedding model?" —
by giving the daemon a model catalog: a registry of known models, install /
remove with SHA-256-verified download + local storage, and disk accounting.
**Management only** — the daemon orchestrates models; it does not run inference
(that is a future `ModelProvider` impl, E1-full). Keeps the daemon an
orchestrator, not an ML runtime.

## Delivered

| File | Change |
|---|---|
| `src/model.rs` | New — `ModelSpec` (registry), `InstalledModel` (persisted), `Download`, `built_in_registry()` (curated: OpenAI / Ollama / BGE-ONNX), `ModelManager` (catalog / install / remove / disk), streaming `download_and_verify` (SHA-256), `JsonModelStore`, `sha256_hex` |
| `src/store.rs` | New `ModelStore` trait (DIP seam, alongside Project/Workspace stores) |
| `src/daemon.rs` | Owns `ModelManager`; `DaemonDeps.models: Box<dyn ModelStore>`; `with_deps` now returns `Result`; `models_catalog` / `model_detail` / `install_model` / `remove_model`; `/v1/system` models count is live |
| `src/http.rs` | Real handlers (replaced the `501` stubs): `GET /v1/models`, `POST /v1/models/install`, `GET|DELETE /v1/models/*id` (catch-all — ids contain slashes) |
| `Cargo.toml` | `sha2`, `futures-util`, reqwest `stream` feature |

## API

- `GET /v1/models` → `{ installed[], available[], disk_bytes }`
- `POST /v1/models/install` `{id}` → installs (remote = register; local = download + verify)
- `GET /v1/models/*id` → installed model detail
- `DELETE /v1/models/*id` → remove + delete files

## Design

- **Two install paths.** Remote-service models (`provider: openai|ollama|…`,
  no `download`) install instantly by registering. Local models (`onnx`, with
  `download { url, sha256, size }`) stream to `<home>/models/<id>/model.bin`,
  hashing as they go, and verify SHA-256 (mismatch → delete + error).
- **DIP seam.** `ModelStore` (impl `JsonModelStore`) mirrors the other stores —
  a `SqliteModelStore` drops in later with no daemon change.
- **The registry is the E2 seam.** Each `ModelSpec.provider` is exactly what the
  document pipeline's embedder stage (E2) and a future local-inference
  `ModelProvider` (E1-full) will dispatch on.

## Findings

- **Local model download is implemented but not e2e-tested** — verifying a real
  multi-GB model file isn't feasible in CI. Instead: the SHA-256 helper is
  unit-tested against the known `SHA-256("abc")` vector, and the remote-install
  path is e2e-tested. The BGE-ONNX registry entry has a placeholder hash until
  the E1-full inference provider pins a specific artifact.
- **Model ids contain slashes**, so detail/delete use a `/*id` catch-all rather
  than `:id`; install takes the id in the JSON body (no URL issue).

## Validation

```
cargo test -p valori-daemon
  → 14 unit (+ model: sha256, remote-install, unknown) + 3 e2e = 17 passed, 0 failed
```

Live HTTP smoke test, all green:
- `GET /v1/models` → 3 available, 0 installed, `disk_bytes: 0`
- install `openai/text-embedding-3-small` → registered (provider `openai`, dim 1536, no file)
- `GET /v1/models/openai/text-embedding-3-small` → detail via catch-all route
- `/v1/system` → `models: 1`; event `model.installed`; delete → `200`
- no orphaned processes.

## Follow-ups

| Item | Milestone |
|---|---|
| `ModelProvider` inference trait + local ONNX runtime (`ort`/candle) | E1-full |
| Real local model download tested against a small fixture over a local HTTP server | E1.1 |
| Wire the pipeline embedder stage to select an installed model | E2 |
| Provider credential management (API keys per provider) | E1.1 |
| `update` (re-verify / re-download) + registry refresh from a remote index | later |
