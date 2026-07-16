# Phase M1–M4 — valori-models Package Manager

## Goal

Transform `valori-models` from a thin embedding wrapper into a proper package manager and runtime registry for AI models. The crate must answer: what models are installed, where, which provider owns them, are they valid, can they embed, what dimensions do they output, are they compatible with a collection. It must not know about documents, chunks, pipelines, HTTP, or desktop UI.

## Delivered

### New files

| File | Phase | What it provides |
|---|---|---|
| `src/types.rs` | M1 | `ModelTask`, `ModelFormat`, `ProviderKind`, `ManifestStatus` — core enums with serde |
| `src/manifest.rs` | M1 | `ModelManifest` — single source of truth, replaces `InstalledModel` + `ModelSpec` split |
| `src/provider/registry.rs` | M2 | `ProviderRegistry` + `ProviderFactory` trait — eliminates all `match kind` dispatch |
| `src/resolver.rs` | M3 | `Resolver` — `resolve(task, dim?)` selects best installed model |

### Updated files

| File | Change |
|---|---|
| `src/storage/mod.rs` | `ModelStore` / `JsonModelStore` now store `ModelManifest` instead of `InstalledModel`; added `update()` |
| `src/registry/mod.rs` | `built_in()` returns `Vec<ModelManifest>` with proper task/format/family/license/homepage/min_ram_mb fields |
| `src/downloader/mod.rs` | M4 state machine: `DownloadState` (Queued/Downloading/Paused/Verifying/Complete/Failed), `DownloadJob` with channel-based progress events and cancellation token; low-level `download_and_verify` preserved |
| `src/verifier/mod.rs` | Operates on `ModelManifest` (was `InstalledModel`); renamed `verify_model` → `verify_manifest` / `verify_manifest_full` |
| `src/provider/mod.rs` | Exposes `ProviderRegistry`; `provider_from_config` is now a thin wrapper over the registry (backward compat for node config path); `from_model(InstalledModel)` constructors deleted |
| `src/provider/{ollama,openai,voyage}.rs` | Removed `InstalledModel` import + `from_model` constructors (now owned by registry factories) |
| `src/lib.rs` | New public surface: all M1–M4 types re-exported; `ModelManager` gains `all_manifests()`, `disk_usage_bytes()`, `catalog_json()`, `resolve()`, `resolve_for_collection()`, `provider_for()`, `provider_from_config()`; `verify()` delegates to new verifier |

### What M1–M4 replaced

| Old | New |
|---|---|
| `InstalledModel { id, name, provider: String, dim, path, size_bytes, sha256 }` | `ModelManifest` (15 fields, typed enums) |
| `ModelSpec { id, name, provider: String, dim, download }` | Merged into `ModelManifest` with `status: ManifestStatus::Available` |
| `provider_from_config(kind: &str, ...)` match statement | `ProviderRegistry::build(kind, ...)` — no match |
| `build(model: &InstalledModel)` match statement | `ProviderRegistry::build_from_manifest(&manifest)` |
| Ad-hoc download in `ModelManager::install` | `DownloadJob::run()` with state tracking and progress channel |
| `verify_model(&InstalledModel)` | `verify_manifest(&ModelManifest)` |

## Findings

- `ProviderKind::AzureOpenAI` has no factory yet (no Valori azure provider implementation). The registry returns `ModelError::Provider` for unknown kinds. Added as a type so the manifest can describe Azure-hosted models.
- `pause()`/`resume()` in M4 today means cancel + re-download from zero. True byte-range resumption needs `Range:` header support on the server and local state persistence — deferred.
- `ManifestStatus` transient states (Downloading, Verifying, etc.) are not persisted to `models.json` — only `Installed` entries are stored. This is intentional.

## Validation

```
cargo test -p valori-models
```

**42 tests: 42 passed, 0 failed** + 1 doc-test.

New tests added:
- `types::tests` — 5 tests (roundtrip, helpers, Display)
- `manifest::tests` — 5 tests (ready/supports_embedding/is_local/serde/optional fields)
- `provider::registry::tests` — 4 tests (list kinds, unknown errors, dummy builds, custom factory)
- `resolver::tests` — 7 tests (by task, exact dim, wrong dim, available excluded, no task match, compatible models, resolve_for_embedding)
- `storage::tests` — 3 tests (crud, update, nonexistent remove)
- `verifier::tests` — 5 tests (remote, missing, valid, corrupted, unverified)
- `registry::tests` — 6 tests (non-empty, all available, no duplicates, remote/local url invariants, dims > 0)

## Follow-ups (M5–M10)

| Phase | What | Priority |
|---|---|---|
| M5 | Storage manager: `disk_usage()`, `free_space()`, `delete_unused()`, `move_model()` | ⭐⭐⭐⭐ |
| M6 | Verification already done (shipped here). Extend with size-check and manifest-match. | ⭐⭐⭐⭐ |
| M7 | Local catalog (`models.json` shipped with crate, browsable offline) — built-in registry IS this. | ✅ done |
| M8 | Runtime selection via `resolve_for_collection()` — already wired in `ModelManager`. | ✅ done |
| M9 | Update check: compare installed `sha256` vs. registry entry | ⭐⭐⭐ |
| M10 | Import local model: verify → create manifest → register | ⭐⭐⭐ |
| — | `AzureOpenAIFactory` implementation | when needed |
| — | True byte-range resumption in downloader | ⭐⭐ |
