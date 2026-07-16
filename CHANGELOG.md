# Changelog

All notable changes to Valori are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added (Phase P8 — CI hardening — 2026-07-16)

- **`.github/workflows/ci.yml`** — two new parallel jobs:
  - `coverage` — installs `cargo-llvm-cov` via `taiki-e/install-action` (prebuilt, no compile), runs `cargo llvm-cov --package valori-kernel --lcov`, uploads `lcov.info` as a 14-day artifact, writes a `--summary-only` table to `$GITHUB_STEP_SUMMARY`. Does not gate on a threshold (baseline tracked in K3 doc).
  - `miri` — nightly toolchain + `miri` component; runs `cargo miri test -p valori-kernel --test fxp` (Q16.16 arithmetic UB) and `--test proof` (Merkle root + InsertReceipt UB) with `MIRIFLAGS=-Zmiri-disable-isolation`. Blocks merge on Miri errors.
- **`.github/actions/rust-setup/action.yml`** — composite action extended with `toolchain` (default `stable`) and `components` inputs. Switches from `dtolnay/rust-toolchain@stable` to `@master` with the configurable channel. All existing callers are unaffected (they omit both new inputs and get the same stable/no-components behavior as before).

### Added (Phase P6 — InsertReceipt cryptographic receipts — 2026-07-16)

- **`valori-kernel/src/proof.rs`** — `InsertReceipt` struct: `{ record_id, old_root, new_root, proof, sequence, timestamp, state_hash }`. `build()` computes `proof` via `generate_proof_bytes` (Merkle root of Q16.16 FXP values) and `state_hash` as `BLAKE3("valori-insert-receipt-v1" ‖ fields)`. `verify()` recomputes the self-hash and returns `true` iff the receipt is unaltered.
- **`valori-node/src/api.rs`** — `InsertReceiptJson` (hex-string HTTP form) + `From<InsertReceipt>` impl; `InsertRecordResponse { id, receipt }` (backward-compatible: old clients that only read `id` are unaffected).
- **`valori-node/src/server.rs`** (standalone) — `POST /v1/records` now returns the full receipt: `old_root` captured before insert, FXP values converted from `payload.values`, `new_root` and `sequence` captured from a post-insert read lock.
- **`valori-node/src/cluster_server.rs`** (cluster) — same receipt in `InsertResponse`; `sequence` = `resp.log_index`, `new_root` = `resp.state_hash` from `ClientResponse`.
- **`python/valoricore/remote.py`** — `insert_with_receipt(vector, ...)` on both `SyncRemoteClient` and `AsyncRemoteClient`; returns the `receipt` dict from the HTTP response.
- **Tests** — 5 new `InsertReceipt` tests in `crates/valori-kernel/tests/proof.rs`: `verify_roundtrip`, `verify_detects_tampering` (record_id / sequence / new_root), `deterministic`, `proof_field_matches_generate_proof_bytes`, `state_hash_differs_from_roots`. Kernel test count: 153 (was 148).

### Added (Phase K4 — Snapshot version migration tests — 2026-07-16)

- **`crates/valori-kernel/tests/snapshot_version_migration.rs`** (new) — 10 tests covering every `schema_ver` 1–6 backward-compat branch in `decode_state`, which were previously untested dead code under the test suite (the encoder always writes the current version). Includes: `v1_decodes_correctly`..`v6_decodes_correctly` (per-version field assertions), `v1_hole_slot_decodes_as_absent_without_shifting_ids`, `cross_version_decode_reencode_chain_is_hash_stable` (decode → hash vs. reference → reencode → decode → fixed-point for every V1–V7), `v6_out_of_range_namespace_head_is_rejected`, `schema_version_zero_is_rejected`. Mutation-tested: disabling the V1–V3 incoming-edge reconstruction block in `decode.rs` causes exactly the right 4 failures; `v4/v5/v6` stay green. Kernel test count: 148 (was 138).

### Added (Phase D1.3 — Installers + clean-machine validation groundwork — 2026-07-13)

- **Fixed**: two API route handlers (`api/records/[id]/route.ts`, `.../metadata/route.ts`) used Next.js 14's synchronous `params` signature, which blocked `next build` outright on this repo's Next.js version. Fixed to `params: Promise<...>` + `await`, matching the convention already used elsewhere.
- **`ui/` bundled as a Node sidecar**: `desktop/scripts/prepare-ui-server.mjs` (new, packages `ui/`'s `next build --output standalone` as a Tauri bundle resource, including the manual `.next/static` copy Next's standalone output omits); `desktop/src-tauri/src/ui_server_manager.rs` (new, release-only — spawns the bundled `node` sidecar against it on a fixed loopback port, then navigates the main window from a "Starting Valori…" loading page to the real app once healthy). `tauri dev` is unaffected.
- **First real `tauri build` in this project** — produced and verified `Valori.app` + `Valori_0.1.0_aarch64.dmg` (checksum-verified via `hdiutil verify`). All 4 sidecars (`valori-desktop`, `valori-daemon`, `valori-node`, `node`) and the bundled `ui-server` resource confirmed correctly placed; confirmed via real launch that the bundled ui-server actually serves the app (not just that the build succeeded).
- **Fixed two real shutdown bugs found via launch-testing, not inspection**: (1) a raw SIGTERM (session logout, `killall`, force-quit) bypassed the graceful `ExitRequested` handler entirely, orphaning the bundled ui-server process and leaving its port held — fixed with a `#[cfg(unix)]` SIGTERM handler; (2) the existing `ExitRequested` handler's own call to `AppHandle::exit()` re-triggers `ExitRequested` (per Tauri's docs) — a real infinite-loop-on-quit risk that had never been exercised until now — fixed with a shared `Arc<AtomicBool>` shutdown guard.
- **`.github/workflows/desktop-build.yml`** (new) — macOS/Windows/Linux matrix build producing each platform's installer (`.dmg`/`.msi`/`.AppImage`) as a CI artifact. Signing/notarization explicitly deferred to Phase D1.4.
- **`docs/architecture/desktop-layout.md`** (new) — real app-bundle and workspace directory layout, startup sequence, fixed ports.
- **`docs/DESKTOP_RELEASE_CHECKLIST.md`** (new) — manual clean-machine smoke test steps, deliberately not automated this phase.

### Added (Phase D3.1 — Bundle the daemon and node as Tauri sidecars — 2026-07-13)

- **`desktop/scripts/prepare-sidecars.mjs`** (new) — resolves host target triple, locates/builds `valori-daemon` + `valori-node`, copies them into `src-tauri/binaries/<name>-<triple>[.exe]` per Tauri's `externalBin` naming convention. `--release` always rebuilds in release mode; dev mode reuses whatever's already built.
- **`desktop/scripts/dev.mjs`** (new) — new `beforeDevCommand`: preps sidecars synchronously, then starts `ui/`'s dev server. Required because Tauri's build script validates `externalBin` resource paths on every cargo build, not just `tauri build`.
- **`bundle.externalBin`** in `tauri.conf.json` — bundles both binaries into the app; `beforeBuildCommand` now runs `prepare-sidecars.mjs --release`.
- **`desktop/src-tauri/src/daemon_manager.rs`** — rewritten around exactly two code paths (per explicit user direction, no env-var override): dev-mode `target/{release,debug}` search vs. release-mode Tauri sidecar spawn (`tauri-plugin-shell`). Adds a version handshake (`GET /version` api-level check, `UnsupportedVersion` error on mismatch instead of a later mysterious failure) and `VALORI_NODE_BIN` wiring so the daemon sidecar can find its bundled `valori-node` sidecar with no Cargo/target-dir assumption on the end user's machine.
- `tauri-plugin-shell = "2"` added to `desktop/src-tauri/Cargo.toml`.

### Added (Phase D3 — Desktop launches and manages the daemon — 2026-07-13)

- **`POST /v1/shutdown`** (`valori-daemon`) — graceful, cross-platform daemon shutdown over HTTP; snapshots every running project before the process exits. Exists because OS signal semantics aren't uniform across macOS/Linux/Windows for a process spawned and supervised by another process (the desktop app).
- **Fixed:** `Runtime::stop_all()` previously hard-killed every supervised node with no snapshot on daemon shutdown (Ctrl-C or desktop close) — a real durability gap, since `stop()` for a single project always snapshotted first. Now `stop_all()` does the same snapshot-then-terminate for every node.
- **`desktop/src-tauri/src/daemon_manager.rs`** — desktop now supervises the `valori-daemon` process directly: `start_daemon` (spawns it with `VALORI_HOME` from the user's chosen workspace, polls `/health`, no-ops if already running), `stop_daemon` (calls `POST /v1/shutdown`, falls back to a hard kill if it doesn't exit), `daemon_status`. An `ExitRequested` hook calls the graceful shutdown before the desktop window is allowed to close.
- **`ui/src/lib/native.ts`** — `startDaemon`/`stopDaemon`/`daemonStatus` bridge functions.
- Welcome wizard's workspace folder choice now actually becomes `VALORI_HOME` (`Welcome.tsx` calls `startDaemon(workspaceDir)` on finish); returning users get the daemon started automatically against their persisted workspace on every launch (`AppShellGate.tsx`).
- **Fixed:** `crates/valori-daemon/tests/lifecycle.rs::supervisor_restarts_crashed_node` had a pre-existing race (asserted the crash would be visible on the very first `supervise_tick()` after `kill -9`, which isn't guaranteed) — now polls until the restart lands instead of asserting on a specific tick.

### Added (Phases M5–M6 — Package Store + Integrity Manager — 2026-07-13)

- **`PackageStore`** — on-disk package manager with `<root>/<task>/<sanitized-id>/manifest.json` layout; `register()` (remote/no-download), `install()` (atomic download + rename), `commit_staged()`, `remove()`, `repair()`, `list()`, `find_by_task()`, `disk_usage()`, `exists()`, `get()`, `acquired_lock()`.
- **`PackageManifest`** (M5.3) — versioned per-package manifest: `schema_version`, `package_version`, `created`, `updated`, `size`; wraps `ModelManifest`.
- **`InstallLock`** (M5.2) — RAII exclusive lock via `OpenOptions::create_new`; prevents concurrent installs from two processes; released on drop.
- Atomic install (M5.1): download → `.tmp/<timestamp>/model.bin` → SHA-256 verify → `fs::rename` → write `manifest.json`; stale `.tmp/` entries cleaned on `PackageStore::new()`.
- **`IntegrityManager`** (M6) — `verify(id)` + `verify_all()` → `Vec<IntegrityReport>` with `IntegrityStatus`: Verified / Remote / Missing / Unverified / Corrupted.
- **`repair_package(store, id)`** (M6) — returns `RepairAction`: AlreadyHealthy / SizeRepaired / NeedsReinstall { download_url }.
- **`RefCounter`** (M6.2) — in-memory model→project reference tracking; `add_ref`, `remove_ref`, `ref_count`, `can_delete`, `all_referenced_ids`, `referencing_projects`.
- **`GarbageCollector`** (M6.1) — `scan(&refs)` → `GcReport { unreferenced, reclaimable_bytes }`; `clean(&refs)` → removes all unreferenced; `safe_delete(id, &refs)` → errors if model in use.
- **`SystemHealth`** / **`PackageHealth`** (M6.3) — per-package health (Verified / Installed / Missing / Corrupted + size + ref_count); aggregate totals (total_installed, verified, corrupted, missing, disk_used_bytes, reclaimable_bytes).
- **`GET /v1/models/health`** — added to both standalone and cluster routers; reads `VALORI_MODELS_DIR` (default: `~/.valori/models`); returns `SystemHealth` JSON.
- `ModelError::InstallConflict` — new error variant for lock contention.
- `dirs = "5"` added to `valori-node` deps for home-dir resolution.

### Added (Phases M1–M4 — valori-models Package Manager — 2026-07-13)

- **`ModelManifest`** — replaces `InstalledModel` + `ModelSpec`; 15 typed fields: `provider: ProviderKind`, `task: ModelTask`, `format: ModelFormat`, `status: ManifestStatus`, `family`, `quantization`, `min_ram_mb`, `license`, `homepage`, `download_url`.
- **`ModelTask`** — `Embedding | Generation | Reranker | Vision | Speech`
- **`ModelFormat`** — `Onnx | Gguf | Safetensors | Remote`
- **`ProviderKind`** — `OpenAI | Ollama | Voyage | Anthropic | AzureOpenAI | Custom | Local | Dummy`; `as_str()` / `from_str()`.
- **`ManifestStatus`** — `Available | Queued | Downloading { progress_bytes, total_bytes } | Paused | Verifying | Installed | Failed { reason }`
- **`ProviderRegistry`** + **`ProviderFactory`** — eliminates all `match kind` dispatch; `register()`, `build()`, `build_from_manifest()`, `provider_kinds()`; pre-loaded with Ollama / OpenAI / Voyage / Custom / Dummy factories.
- **`Resolver`** — `resolve(task, dim?)` selects best installed model; `compatible_embedding_models(dim)`, `resolve_for_embedding(dim)`.
- **`DownloadJob`** + **`DownloadState`** + **`DownloadEvent`** — M4 download state machine with channel-based progress events and cancellation token.
- **`ModelStore::update()`** — in-place manifest update (status, path, sha256 after install).
- **Built-in catalog** enriched with `family`, `license`, `homepage`, `min_ram_mb`, `download_url` for all 11 entries.
- **`ModelManager`** gains: `all_manifests()`, `disk_usage_bytes()`, `catalog_json()`, `resolve()`, `resolve_for_collection(dim)`, `provider_for(id)`, `provider_from_config()`.
- `provider_from_config` now delegates to `ProviderRegistry` (backward compat shim for node env-var path).

### Added (Phase E4 — Ingest Pipeline Observability — 2026-07-13)

- **`CancellationToken`** — `Arc<AtomicBool>`-backed, `Clone`; `check()` returns `Err(IngestError::Cancelled)` when triggered; checked between each pipeline stage.
- **`RetryPolicy`** — `Never | Fixed { attempts, delay_ms } | Exponential { max_attempts, base_delay_ms, max_delay_ms }`; async `execute(FnMut() -> Fut)`; applied to the embedder stage.
- **`PipelineConfig`** — `{ batch_size, retry, timeout_secs }` with builder methods; default = original behavior (no retry, one batch, no timeout); `batch_size` enables streaming (embed+write N chunks before moving to the next N).
- **`PipelineHook`** — observer trait with 6 default no-op methods (`after_read`, `before_chunk`, `after_chunk`, `before_embed`, `after_embed`, `after_write`); multiple hooks stack; `NoopHook` for tests.
- **`ProgressEvent`** — typed channel events: `StageStarted`, `ChunkProgress { completed, total }`, `StageCompleted { stage, duration_ms }`, `Done`, `Failed`; optional `ProgressSender` passed to `run_observed`.
- **`StageMetrics` / `StageResult` / `PipelineResult`** — per-stage timing, counters, and warnings; `PipelineResult::summary()`, `stage()`, `all_warnings()`.
- **`IngestPipeline::run_observed()`** — full observable entry point; `run()` stays backward-compatible.
- **`WriteResult`** — added `Serialize/Deserialize` (required by `PipelineResult`).

### Added (Phases E3.1–E3.6 — Extractor Framework — 2026-07-13)

- **`Extractor` trait** — bytes-in / `Document`-out; synchronous (no I/O); separates parsing from file access.
- **Five `Extractor` impls** — `TextExtractor`, `MarkdownExtractor`, `HtmlExtractor`, `PdfExtractor`, `DocxExtractor` in `src/extractors/`.
- **`ExtractorRegistry`** — `extractor_for_extension`, `extractor_for_mime`, `extractor_for_path`, `extractor_for_bytes` (magic-byte MIME detection via `infer`), `all_capabilities()`.
- **`DocumentMetadata`** — typed struct replacing `metadata: Value` on `Document`; fields: `title`, `author`, `language`, `created_at`, `modified_at`, `page_count`. All readers updated.
- **`DocumentValidator`** — checks: empty, too-large, page limit, malformed UTF-8, protected PDF. Standalone; not yet wired into pipeline.
- **`DocumentSource`** — typed origin enum: `File`, `Url`, `Memory`, `GitHub { repo, branch, file }`, `S3 { bucket, key }`.
- **`ReaderCapabilities`** — `extensions`, `mime_types`, `supports_streaming/metadata/images`; exposed on every `Extractor` via `capabilities()` and aggregated by `ExtractorRegistry::all_capabilities()`.

### Added (Phase E3.5 — ReaderRegistry — 2026-07-13)

- **`ReaderRegistry`** — `reader_for_extension(ext)` and `reader_for_path(path)` return `Arc<dyn Reader>`; all extension-to-reader mapping lives in one place; unknown extension returns `IngestError::Reader`.

### Added (Phase E3 — Format Readers — 2026-07-13)

- **`MarkdownReader`** — CommonMark → plain text via `pulldown-cmark`; H1 heading promoted to `metadata.title`.
- **`HtmlReader`** — visible-text extraction via `scraper`; `<script>`/`<style>` subtrees pruned; `<title>` and `<meta name="author">` surfaced as metadata.
- **`PdfReader`** — file-path input; text via `pdf-extract`, page count via `lopdf`; runs in `spawn_blocking`.
- **`DocxReader`** — file-path input; unzips, parses `word/document.xml` `<w:t>` runs + `docProps/core.xml` core properties via `quick-xml`; runs in `spawn_blocking`.
- All four readers implement the existing `Reader` trait and return the existing `Document` type — no pipeline changes required.

### Changed (Phase E2.5 — KernelWriter wiring — 2026-07-13)

- `POST /v1/ingest` sync and async paths now delegate to `IngestPipeline::run()` + `KernelWriter`; ~200 lines of inline `embed_batch → insert_batch_ns → nodes/edges/metadata` orchestration removed from the handler.
- `KernelWriter` (in `valori-node`) implements `valori-ingest::Writer` — per-chunk vector insert, reranker index, chunk-node, parent edge, and chunk metadata in one place.
- `provider_from_config()` factory added to `valori-models` — builds `Box<dyn ModelProvider>` from raw env-var strings without the `InstalledModel` registry.
- HTTP API surface unchanged; `ingest_update` path untouched.

### Added (Phase E2 — Composable Ingest Pipeline — 2026-07-13)

- **`Document`** — shared data object (BLAKE3 id, source, mime_type, metadata, content) that flows through every ingest stage.
- **`trait Reader`** + `TextReader` — first stage; converts raw input to `Document`. Format changes are local to this stage.
- **`trait Chunker`** + `ValoriChunker` — wraps existing `chunk_document`; no logic changed.
- **`trait Embedder`** + `ModelProviderEmbedder` — delegates to `Box<dyn ModelProvider>` (from `valori-models`); no Ollama/OpenAI awareness in the stage itself.
- **`trait Writer`** + `NoopWriter` — final stage contract; `KernelWriter` implementation lives in `valori-node` (separate migration).
- **`IngestPipeline`** — `Reader → Chunker → Embedder → Writer`, returns one record ID per chunk. Named `IngestPipeline` to leave room for `QueryPipeline`, `SearchPipeline`, etc.
- `embed.rs` and `handler.rs` unchanged — existing node call sites unaffected.
- **Tightened (E2 exit checklist)**: `ValoriChunker` renamed to `DefaultChunker` (names describe behavior, not brand); stage boundaries use typed objects (`Chunk`, `Embedding`, `WriteResult`) not raw primitives; `IngestError` is an enum with stage variants (`Reader`/`Chunk`/`Embed`/`Writer`); `IngestPipeline::builder()` fluent API replaces positional constructor.
- 19 crate tests (was 13); `valori-node` still builds clean.

### Added (Phase E1.1 — `valori-models` Standalone Crate — 2026-07-13)

- **New `valori-models` crate** — shared model management subsystem used by the daemon, `valori-ingest`, the Python SDK, and the desktop without duplication.
- **`ModelProvider` trait** (`kind`, `model_name`, `dim`, `embed`, `health`) with provider implementations: `OllamaProvider` (batch + legacy fallback), `OpenAIProvider` (OpenAI-compatible), `VoyageProvider`, `DummyProvider` (zero vectors for tests). `build()` factory dispatches by provider string.
- **`ModelStore` DIP seam** — `JsonModelStore` backed by `<home>/models.json` (write-then-rename); `SqliteModelStore` drops in later with no change to `ModelManager`.
- **Built-in registry** of 11 models: OpenAI ×3, Ollama ×3, Voyage ×2, BGE-ONNX ×3.
- **`VerifyStatus`** (`Remote | Ok | Missing | Unverified | Corrupted`) + `verify_model()` for on-demand re-verification.
- Fixed workspace `Cargo.toml` — added 7 missing `[workspace.dependencies]` entries for crates added in prior N/E sessions.
- 5 crate tests: SHA-256 known-vector, dummy provider, storage CRUD, verifier (remote + local).
- **E1.2**: Deleted `valori-daemon/src/model.rs` (351 lines, fully duplicated); daemon now imports from `valori-models`. Removed `ModelStore` trait from `daemon/store.rs`. Added `From<ModelError> for DaemonError` bridge. Removed `sha2`/`futures-util` from daemon deps. Single source of truth established; 11 daemon unit tests pass.

### Added (Phase E1 lite — Model Manager — 2026-07-13)

- **Daemon model catalog**: `GET /v1/models` (installed + available from a curated registry + total disk usage), `POST /v1/models/install` `{id}`, `GET|DELETE /v1/models/*id`. Replaces the previous `501` stubs.
- **Two install paths**: remote-service models (OpenAI/Ollama/…) install by registering; local models (ONNX/…) stream-download to `<home>/models/<id>/` with **SHA-256 verification** (mismatch → delete + error) and disk accounting.
- **`ModelStore` DIP seam** (impl `JsonModelStore`, `<home>/models.json`) alongside the project/workspace stores — a `SqliteModelStore` drops in later with no daemon change. `DaemonDeps` now injects the model store too.
- Management only — the daemon orchestrates models; local inference is a future `ModelProvider` (E1-full). Each model's `provider` is the seam the document-pipeline embedder (E2) will dispatch on. New event: `model.installed` / `model.removed`.

### Added (Phase D2.2 — Restart Loop & Health FSM — 2026-07-13)

- **Self-healing supervision**: a background monitor detects crashed nodes and restarts them per an operator-set `RestartPolicy` (`never` (default) / `on_failure` / `always`) with capped exponential backoff (2→60s). Crash count and last crash reason are tracked and surfaced under `supervision` in project responses; `restart_policy` is settable on project create and persisted in the manifest.
- **Operational/runtime split** (review point 3): the `Runtime` detects exits (`poll_exits`, via a new non-blocking `RunningProcess::has_exited`) and executes start/stop; a separate operational `Supervisor` decides *whether* to restart (policy + backoff) and owns crash bookkeeping. The daemon's monitor tick wires them.
- **Richer `RuntimeState`**: adds `Recovering` (auto-restart after a crash — distinct from a fresh `Starting`, since Valori replays its event log on recovery), with the corresponding legal transitions.
- Lifecycle events now include `project.crashed`, `project.recovering`, `project.restarted`.

### Changed (Phase D2.1 — Dependency-Inversion Seams — 2026-07-13)

- **The daemon now runs entirely on injected trait objects** (Dependency Inversion). `Daemon` holds `Box<dyn ProjectStore>`, `Box<dyn WorkspaceStore>`, `Box<dyn Runtime>`, `Box<dyn EventStore>` and constructs nothing durable itself — a `DaemonDeps` struct + `with_deps()` inject everything; `new()` wires the defaults. Swapping to a SQLite store or Docker runtime needs no daemon change.
- **New seams**: `ProjectStore`/`WorkspaceStore` (impl `JsonProjectStore`/`JsonWorkspaceStore`), `EventStore` (impl `MemoryEventStore`), and `Launcher` + `RunningProcess` (impl `LocalLauncher`/`LocalProcess`). The `Runtime` now *orchestrates* (health, state, resources) while the `Launcher` *launches* — so a future `DockerLauncher` returns a container handle without the runtime touching `std::process`.
- **`RuntimeState` state machine**: node lifecycle is now `Stopped → Starting → Running → Stopping → Stopped` (plus `Failed`) with illegal transitions returning an error instead of corrupting state; `NodeInfo.status` is the typed state, not a bespoke enum.
- **`RestartPolicy` moved out of `runtime/`** to a top-level operational module — whether a node *should* exist is an operator decision, not the runtime's.

### Added (Phase D2 — Node Runtime — 2026-07-13)

- **`Runtime` trait + `LocalRuntime`**: the daemon now runs nodes through a pluggable `Box<dyn Runtime>` (async-trait) instead of a hard-coded supervisor, so `DockerRuntime` / `SshRuntime` / `RemoteRuntime` slot in later with no change to the daemon, API, or desktop. The monolithic `Supervisor` was decomposed (SRP) into focused components: `PortAllocator`, `ResourceMonitor`, `RestartPolicy`, plus health polling and log capture in `LocalRuntime`.
- **`GET /v1/events`**: Docker-style lifecycle event stream (in-memory ring buffer) — `project.created`, `project.started`, `project.stopped`, `workspace.created/deleted`. Poll today; SSE/WebSocket push later (same shape).
- **`GET /v1/projects/:name/runtime`**: live per-node resource stats (CPU %, resident MB, threads on Linux, uptime) sampled via `ps` — no platform crate.
- **Stable resource IDs**: projects and workspaces now carry a UUID `id` (names become mutable labels). `GET /v1/config` reports the runtime descriptor (`kind: "local"`, binary, port range).

### Added (Phase D1.1 — Stabilize the Daemon API — 2026-07-13)

- **System / discovery endpoints**: `GET /v1/system` (version, platform, daemon PID, uptime, and live counts of projects/running/workspaces/models — the endpoint every client calls first), `GET /version`, `GET /v1/config`. Whole API is versioned under `/v1` from day one.
- **Workspaces** — the grouping layer above projects (RFC-0006): `GET|POST /v1/workspaces`, `PATCH|DELETE /v1/workspaces/:name`. A `default` workspace always exists; deleting a workspace that still has projects is refused. Projects carry a `workspace` field (serde-defaulted, so older manifests still load).
- **Collections** proxied through the running node: `GET|POST /v1/projects/:name/collections`, `DELETE …/:collection` → the node's `/v1/namespaces`.
- **Node logs + uptime**: node stdout/stderr captured to `<project>/node.log`, exposed via `GET /v1/projects/:name/logs?tail=N`; node status now includes `uptime_secs`.
- **Model manager stubs** (D4 placeholder): `GET /v1/models` (empty), `POST /v1/models/install` and `DELETE /v1/models/:id` return `501`.

### Added (Phase D1 — Valori Daemon, Milestone 1 — 2026-07-13)

- **New crate `valori-daemon`** + `valori-daemon` binary: the control-plane daemon that owns project lifecycle and supervises `valori-node` instances (RFC-0006 "Docker Desktop for AI Memory"). Rust successor to the TypeScript process manager in `ui/src/lib/server/`.
- **Project lifecycle HTTP API** (Milestone 1): `GET /health`, `GET|POST /v1/projects`, `GET|DELETE /v1/projects/:name`, `POST /v1/projects/:name/{start,stop,restart}`. Projects are directories under `$VALORI_HOME/projects/<name>/` with a `project.json` manifest; one project → one supervised `valori-node`.
- **Process supervision**: internal port allocation (8100–8999, hidden from clients — projects are addressed by name), `/health`-gated startup, best-effort graceful stop (snapshot then terminate; hard kill is still safe via event-log replay), and "no delete while running" enforcement.
- **New crate `valori-daemon`** added to the workspace (members + default-members); the Tauri desktop shell (`desktop/`) is deliberately excluded from the Cargo workspace.
- **`desktop/`** — Tauri 2 scaffold (native control-plane shell), separate from `ui/` so `cd ui && npm run dev` is unaffected. Runs in dev against the Next.js UI; production bundling waits on the daemon absorbing `ui/`'s server API routes.
- **RFC-0006** (`rfcs/0006-desktop-daemon-architecture.md`): daemon architecture — three execution modes (embedded/supervised/remote), path-as-truth + project-scoped-token-as-sugar, workspace layer, collections-are-namespaces scaling model.
- **`_execution` observability block** extended with `operation_hash` + measured `duration_ms` (opt-in via `?explain=true` on `POST /v1/memory/search_vector`).

### Added (Phase N5 — valori-engine extraction — 2026-07-12)

- **New crate `valori-engine`**: the `Engine` struct (1 743-line engine.rs) and all supporting types extracted from `valori-node` into a standalone orchestration crate. Five modules: `config` (`IndexKind`, `QuantizationKind`, `EngineConfig`), `error` (`EngineError`, `CommitError`), `metadata` (`MetadataStore`), `persistence` (`Persistence` enum — Phase E1 durability funnel), `engine` (`Engine::with_config`, `RecoveryMode`, `EngineHealth`, `PoolStats`, `ExecutionResources`).
- **`EngineFromNodeConfig` extension trait**: defined in `valori-node/src/engine.rs`, bridges `NodeConfig → EngineConfig` so all existing `Engine::new(&cfg)` call sites in tests, main.rs, and examples keep compiling with one added `use valori_node::EngineFromNodeConfig;` per file.
- **Dependency Inversion**: `EngineConfig` injects `Arc<dyn KeyVault>` and `Option<Arc<ObjectStoreBackend>>`; `valori-engine` never constructs `AesGcmVault` or calls `ObjectStoreBackend::from_env()` — those remain in `valori-node`.
- **Re-export shims**: `valori-node/src/errors.rs`, `metadata.rs`, `commit/persistence.rs`, and `config.rs` (for `IndexKind`/`QuantizationKind`) now delegate to `valori-engine` via `pub use`, keeping all existing `crate::*` imports across server.rs, cluster_server.rs, routes/, etc. unchanged.

### Added (Phase N4 — valori-ingest extraction — 2026-07-12)

- **New crate `valori-ingest`**: embedding client and chunking logic extracted from `valori-node/src/embedder.rs` and `ingest.rs` into a standalone crate with zero `valori-*` dependencies. Three modules: `embed` (`EmbedConfig`, `embed_batch` supporting Ollama/OpenAI/custom), `chunker` (`chunk_document`, `chunk_content_hash`, 4 strategies + auto-detection, `MAX_INGEST_TEXT_BYTES`), `handler` (`ingest_document` stateless axum handler for `POST /v1/ingest/document`).
- **`embed_config_from_node` helper**: added as `pub(crate)` in `engine.rs` — constructs `valori_ingest::EmbedConfig` from `NodeConfig` without requiring `valori-ingest` to depend on `valori-node`.
- **Recursion bugfix in chunker**: tree strategy falling back to `"auto"` could infinite-recurse (auto re-detects tree → loop → SIGABRT). Fixed by falling back directly to `"fixed"` instead.

### Added (Phase N3 — valori-rag extraction — 2026-07-12)

- **New crate `valori-rag`**: GraphRAG, Tree-RAG, and Community Layer extracted from `valori-node` into a standalone crate. Three modules: `graph` (`resolve_seed_nodes`, `expand_subgraph`), `tree` (`TreeIndex`, `Receipt`, `verify_chain`, stateless axum handlers), `community` (Label Propagation, centroid ranking, request/response types). New `llm` module holds `LlmConfig` + `extract_entities_via_llm`, decoupled from `EmbedConfig` via a 4-field config struct.
- **`LlmConfig`**: minimal credentials struct in `valori_rag::llm` that breaks the circular dependency between entity extraction and `valori-node`'s `EmbedConfig`. Node constructs `LlmConfig` at the call site; `valori-rag` has no `valori-node` dependency.

### Added (Phase N2 — valori-index extraction — 2026-07-12)

- **New crate `valori-index`**: all vector index structures extracted from `valori-node/src/structure/` into a standalone crate behind a single `VectorIndex` trait. Includes `BruteForceIndex`, `HnswIndex`, `IvfIndex`, `BqIndex`, quantizers (`NoQuantizer`, `ScalarQuantizer`, `ProductQuantizer`), and `deterministic_kmeans`. NEON SIMD kernels and determinism guarantees preserved.
- **`VectorIndex` trait is now a public crate interface**: integration test files in `valori-node/tests/` and `engine.rs` import from `valori_index::` directly; the old `crate::structure::*` internal module is deleted.

### Added (Phase N1 — valori-search extraction — 2026-07-12)

- **New crate `valori-search`**: post-retrieval search primitives extracted from `valori-node` into a standalone crate with no kernel or node dependency. Three modules: `decay` (time-decay re-ranking), `reranker` (BM25 hybrid), `filter` (metadata predicate matching).
- **O(1) IDF lookup in `ValoriReranker`**: added `doc_freq: HashMap<String, usize>` inverted index updated incrementally on every `insert`/`remove`. Previous implementation scanned the full corpus per query term — O(|corpus| × |query_terms|).
- **`restore_corpus` is now deterministic**: rebuilds `doc_freq` from the restored corpus instead of trusting the snapshotted `total_tokens` value, which could be stale after tokeniser changes.


### Fixed (Phase A14 — valori-node audit bug fixes — 2026-07-10)

- **P0 — `RaftKernelCapability::state_hash()` always returned zeros**: Now uses `tokio::task::block_in_place` to call the async `ValoriStateMachine::with_state()` from a sync trait method, computing the real BLAKE3 hash per shard.
- **P0 — `cluster_snapshot_save` only saved shard 0 and read wrong field**: Handler now loops all shards `0..shard_count` and reads `"state_hash"` (not `"hash"`) from `SnapshotArtifactTask` output.
- **P0 — `/health` and `/metrics` gated behind `cluster_auth_guard`**: Middleware restructured so the public sub-router (health, metrics) is merged without auth; only the v1 protected sub-router gets the auth layer.
- **P1 — Namespace truncation in standalone shard routing**: `(ns as u8).wrapping_rem(shard_count)` silently truncated 16-bit namespace IDs before modulo, misrouting namespaces ≥ 256. Fixed to `((ns as u32) % (shard_count as u32).max(1)) as u8` at all 3 callsites in `server.rs`.
- **P1 — `cluster_community_search` hardcoded shard 0**: Handler now resolves `payload.namespace` via `s.sm.resolve_namespace()` and routes to the correct shard via `shard_for_namespace()`, matching `cluster_community_detect` behavior.
- **P1 — `cluster_community_detect` swallowed planner errors with `.ok()`**: Return type changed to `Result<Json<DetectResponse>, (StatusCode, Json<Value>)>`; planner errors now surface as 500 INTERNAL_SERVER_ERROR.
- **P1 — Decay sort inverted in `RaftKernelCapability::memory_search`**: `score * decay_factor` ascending ranked older records better; fixed to `score / decay_factor` ascending, matching the standalone `decay.rs::rerank` formula.

### Added (Phase A13.1 — cluster planner wiring — 2026-07-10)

- **`RaftKernelCapability` extended** with 8 new methods: `save_snapshot`, `graph_rag`, `memory_search`, `community_detect`, `community_search`, `tree_build`, `tree_query`, `tree_hybrid` — backed by `ValoriStateMachine` (`with_state()` / `with_state_and_timestamps()` / `get_meta_json()`).
- **`CapabilityRegistryBuilder::build_cluster()`** now takes `tree_cache` and `community_store` to pass shared state into the cluster capability.
- **7 cluster handlers wired through `run_graph_inline`**: `cluster_graphrag`, `cluster_snapshot_save`, `cluster_tree_build`, `cluster_tree_query`, `cluster_tree_hybrid`, `cluster_community_detect`, `cluster_community_search` in `cluster_server.rs`. Both execution paths now follow the identical `HTTP → ExecutionGraph → TaskRunner → KernelCapability → Response` contract.

### Added (Phase A13 — planner migration — 2026-07-10)

- **8 new `KernelCapability` default methods** in `valori-effect`: `save_snapshot`, `graph_rag`, `memory_search`, `community_detect`, `community_search`, `tree_build`, `tree_query`, `tree_hybrid` — all default to `CapabilityUnavailable`.
- **`EngineKernelCapability` overrides** for all 8 methods in `valori-node/src/capabilities.rs`: each delegates to the live engine subsystem (search, community, tree-RAG, snapshot).
- **5 new Task files** under `valori-effect/src/tasks/`: `snapshot.rs`, `graph_rag.rs`, `memory_search.rs`, `community.rs`, `tree_rag.rs` — 8 concrete `Task` implementations.
- **6 new `TaskKind` variants**: `MemorySearch`, `CommunityDetect`, `CommunitySearch`, `TreeBuild`, `TreeQuery`, `TreeHybrid` in `valori-planner`.
- **Standalone path wired**: `snapshot_save`, `graphrag`, `memory_search_vector`, `community_detect`, `community_search`, `tree_build`, `tree_query`, `tree_hybrid` in `server.rs` all dispatch through `run_graph_inline`. No behavior change — same outputs, same HTTP contract.
- **`Deserialize` added** to `HybridHit`, `HybridResponse` (tree_rag.rs), `CommunitySummary`, `DetectResponse`, `CommunityHit`, `SearchResponse` (community.rs), `MemorySearchHit`, `MemorySearchResponse` (api.rs) — needed for task output round-trip.

### Removed (valori-storage/state dead API pass — 2026-07-10)

- **`EventProof` struct and `generate_proof()`** deleted from `valori-storage::events::event_proof`. Both were superseded by `valori-verify` which owns the full audit path. `compute_event_log_hash()` (the only production caller, used by `/v1/proof/event-log`) is kept.
- **`read_event_log()`** deleted from `valori-storage::events::event_replay`. It dropped namespace information silently and was strictly weaker than `read_all_segments()`. Two `cluster_boot.rs` tests migrated to `read_all_segments()`.
- **`StateManifest`**, **`StateLifecycle`**, **`shutdown_snapshot()`** deleted from `valori-state`. None had any external callers; all were speculative scaffolding from an orchestration layer that was never built.
- **`bootstrap::{has_wal, has_event_log, load_snapshot, validate_snapshot, replay_wal}`** changed `pub` → `pub(crate)`. Retained as internal helpers for future bootstrap orchestration; removed from the public surface.

### Added (persistence contract corpus — 2026-07-10)

- **Snapshot compatibility corpus** (`valori-kernel/tests/snapshot_compat.rs`) — committed V7 binary fixtures (`snapshot_v7_empty.bin`, `snapshot_v7_single.bin`, `snapshot_v7_multi.bin`) paired with pinned state hashes. Four forever-decode tests lock the snapshot encoder, decoder, and `hash_state_blake3` contract against accidental format drift. A fifth test (`snapshot_v7_multi_can_continue_after_restore`) verifies that restored state produces the same hash as replay-from-scratch after a subsequent event.
- **WAL compatibility corpus** (`valori-storage/tests/wal_compat.rs`) — committed `wal_v1_inserts.wal` and `wal_v1_namespace.wal` fixtures with pinned `.hash` files. Two forever-replay tests lock `WalWriter` → `WalReader` → `apply_event_ns` → `hash_state_blake3` against format regressions.
- **Event-log end-to-end corpus** (`valori-state/tests/event_log_compat.rs`) — committed event log fixtures with TOML manifests pinning four independent invariants: `event_count`, `record_count`, `chain_head`, `state_hash`. Tests exercise both `recover_from_event_log` (bootstrap path) and `valori_verify::verify_log_file` (audit path). Three malformed-artifact tests (`bad_magic`, `truncated`, `chain_tampered`) assert that corrupted input is detected and handled, not panicked on.

### Refactored (valori-storage / valori-state cleanup — 2026-07-10)

- **`valori-storage::recovery` deleted** — the module was a dead duplicate of `valori-state::bootstrap` left behind when recovery orchestration was migrated in Phase A3. Zero external callers; `valori-node` and `valori-state` already routed through `valori_state::bootstrap`. `StorageError` preserved in a new `error.rs` module so the public path `valori_storage::StorageError` is unchanged and no callers required updating.
- **Crate responsibilities clarified** — `valori-storage` = persistence primitives (WAL, event log, object store); `valori-state` = recovery orchestration. Docs in `lib.rs`, `CLAUDE.md`, and `AGENTS.md` updated to match.
- **Dependency graph confirmed acyclic**: `valori-core → valori-kernel → valori-wire → valori-storage → valori-state → (valori-consensus, valori-node)` with no back edges.

### Fixed (valori-consensus cleanup — 2026-07-10)

- **`ShardId` deduplicated** — valori-consensus now re-exports the shared valori-core type (via valori-kernel) instead of defining a structurally identical local duplicate; wire encoding unchanged. Stale "namespace routing does not exist yet" doc replaced with shipped S3–S9 behavior.
- **Snapshot IDs derived, not counted** — `snapshot_id` is now `(last_applied index, state-hash prefix)`; the old in-memory counter reset on restart and could reissue a previous ID.
- **Dead `thiserror 1.x` dependency removed**; stale V5→V6 snapshot docs and the `created_at` "replicas agree" claim corrected; `serve_raft_single`/`serve_raft_tls_single` marked as test helpers; obsolete `placeholder.rs` deleted.

### Fixed (valori-wire audit — 2026-07-10)

- **Phantom hardening guards now real** — `METADATA_CAP` enforced at `encode_entry` (write-side; pre-cap logs stay readable), `MAX_ENTRIES_PER_SEGMENT` enforced in the valori-verify replay loop, `MAX_ENTRY_DECODE_BYTES` unified with the applied decode limit. `MAX_SEGMENT_DECOMPRESSED_BYTES` remains reserved for upcoming zstd support and its doc now says so honestly.
- **V4 evolution fixture added** — `segment_v4.bin` + forever-decode test; the current production write format previously had no CI fixture. `make_demo_log` now emits V4 per policy rule 4.
- **Wire cleanups** — stale "understands v2 and v3" error message fixed; `parse_header` V3/V4 arms collapsed; bincode limit errors matched by enum variant instead of display-string substring; `thiserror` 1.0→2.0; `encode_header_v3` marked legacy/fixture-only.

### Fixed (valori-core audit — 2026-07-10)

- **`ExecutionId::new_random()` collision bug (release blocker)** — the old time+stack-address scheme produced ~93% duplicate IDs under sequential calls (937,202 dups measured in 1M); planner operation IDs and async-ingest `job_id`s could collide across clients. Now uses OS RNG via `getrandom` (std-gated). Regression tests: 100k sequential, 80k cross-thread, and a `#[ignore]`d 1M stress test.
- **`ExecutionId: FromStr` added** — parses the 32-hex-digit `Display` form, so `job_<id>` strings round-trip.
- **valori-core dead API trimmed** — `CoreError` reduced to `InvalidInput`; unused `Version::is_compatible_with` removed (its exact-match policy contradicted the actual V5→V6 snapshot compatibility); `Version::next`/`ClusterEpoch::next` use `checked_add` for consistent overflow behavior; docs corrected from "zero-dependency" to "minimal-dependency".

### Internal (Command removal + ValoriKernel deletion — 2026-07-10)

- **`Command` enum deleted from `valori-kernel`** — no kernel code creates or processes `Command` anymore. `state/command.rs` is gone.
- **WAL format upgraded to v2** — `WalWriter` now writes `(KernelEvent, namespace_id)` bincode pairs (header version=2). `WalReader` handles both v1 (Command, backward compat) and v2 transparently; callers always receive `(KernelEvent, u16)`.
- **`LegacyWalCommand`** lives in `valori-storage/src/wal_compat.rs` (private to storage) as the only remaining Command-shaped type — used exclusively for reading pre-K2 WAL files.
- **`ValoriKernel` struct deleted** — the legacy HNSW prototype (`kernel.rs`) and its CRC64 `state_hash()` / binary-payload `apply_event(&[u8])` are gone. `crc64fast` dependency removed from `valori-kernel/Cargo.toml`.
- **Bench bins deleted** — `bench_filter`, `bench_ingest`, `bench_recall` all depended on `ValoriKernel`; removed from `valori-cli`. `bench_1m` and `bench_persistence` (which already used the production path) are retained.
- **`command_for()` deleted from `persistence.rs`** — `Persistence::Wal` arm now calls `w.append_event(event, namespace_id)` directly, no translation layer.

### Internal (coverage tests — 2026-07-10)

- **43 new tests for zero-coverage kernel modules** — `tests/fxp.rs` (22 tests: `fxp_add/sub/mul`, `from_f32`/`to_f32` with saturation, NaN, infinity), `tests/proof.rs` (12 tests: `merkle_root` empty/single/even/odd/order-sensitive, `generate_proof_bytes`, `DeterministicProof` bincode roundtrip), inline tests in `verify.rs` (5: `snapshot_hash`/`wal_hash` against `blake3::hash` directly), inline tests in `adapters/ivecs.rs` (4: single row, multi-row, empty file, zero-dim row). Total kernel tests: **134**.
- **Dead binary-protocol types deleted** — `InsertPayload`, `DeletePayload`, `CMD_INSERT`, `CMD_DELETE`, `FixedPointVector` removed from `types/mod.rs`; exclusively used by the deleted `ValoriKernel::apply_event(&[u8])`.

### Internal (coverage audit — 2026-07-10)

- **`cargo-tarpaulin 0.37.0` installed** — baseline coverage established for `valori-kernel`: **36.24%** (963/2657 lines). Zero-coverage modules ranked by risk: `hnsw.rs` (265L, untested), `proof.rs` (24L), `fxp/ops.rs` (21L), `types/mod.rs` (48L), `verify.rs` (4L), `adapters/ivecs.rs` (11L). Full audit in `docs/phases/phase-K3-coverage-audit.md`.

### Internal (replay unification — 2026-07-10)

- **`KernelEvent` → `apply_event_ns` is now the single authoritative mutation path** — eliminated the `Command` intermediate type from the kernel's internal apply loop. `apply_event_ns` directly contains the logic for every mutation; there is no translation layer.
- **`replay.rs` deleted** — `replay_and_hash` (legacy bincode-Command WAL replay) had zero external callers. `WalHeader` moved to `valori-storage/src/wal_reader.rs` where it belongs.
- **Version-bump omission fixed** — `UpdateRecordMetadata`, `SetMeta`, `InsertRecordEncrypted`, and `ShredKey` previously did not bump `KernelState::version` when applied via `apply_event_ns` directly (the cluster path). Fixed by a single version bump at the end of `apply_event_ns`.
- **`apply_raw_for_test` → `apply_event_for_test`** — engine test helper now takes `&KernelEvent` instead of `&Command`.
- **WAL recovery updated** — `valori-storage::recovery::replay_wal` and `valori-state::bootstrap::replay_wal` both translate legacy `Command` entries to `KernelEvent` before applying, keeping backward-compatible WAL recovery on the canonical path.

### Performance (HNSW wired into namespace search — 2026-07-08)

- **HNSW/IVF/BQ now applies to all named collections** — `Engine::search_l2_ns` previously always called the kernel's brute-force linked-list walk regardless of `VALORI_INDEX`. It now routes through the `VectorIndex` (HNSW, IVF, or BQ) when a non-brute index is active, with namespace post-filtering on the candidates. Measured speedup: 9× at N=1k, 43× at N=10k, 183× at N=50k (in-process, dim=384, k=10).
- **HNSW sort-order bug fixed** (`hnsw.rs`) — `BinaryHeap::into_sorted_vec()` on a MaxHeap returns descending (worst-first). Without `.reverse()`, `select_neighbors` was connecting every node to its M *farthest* neighbors, producing an inverted graph and O(N) traversal.
- **over_fetch reduced from k×20 to k** (`engine.rs`) — the previous `(k * 20).max(200)` multiplier forced ef=200 in HNSW, expanding the beam search to O(N) candidates. Using `k` directly lets ef fall to ef_search (default 50), keeping search sub-millisecond.
- **All records enter the global index** — inserts and `build_index` previously skipped non-default-namespace records. All namespaces now feed `self.index`, enabling the HNSW path above.
- **`drop_collection` cleans the global index** — records in a dropped namespace are now explicitly removed from `self.index`, preventing stale HNSW entries from polluting future searches.
- **`search_l2` delegates to `search_l2_ns(DEFAULT_NS)`** — removes code duplication and ensures the default-collection path also benefits from HNSW automatically.

### Performance (kernel SIMD + algorithmic fixes — 2026-07-08)

- **HNSW uses SIMD distance** — `hnsw.rs` was importing `dist::euclidean_distance_squared` (scalar, `saturating_mul`); now calls `math::l2::l2_sq_i32` which dispatches to NEON (aarch64), AVX2 or SSE4.1 (x86_64). All candidate comparisons in insert and search now run at 4–8× lane width.
- **`fxp_dot` SIMD implementation** — `math/dot.rs` added NEON (`vmull_s32` widening), AVX2, and SSE4.1 paths mirroring `math/l2.rs`. Cosine similarity (contradict, consolidate, memory search) now runs at SIMD speed.
- **HNSW `determine_level` fix** — was hashing the full 384-dim vector (1536 bytes) for deterministic level assignment; now hashes only the 8-byte record ID (~48× less data per insert).
- **Brute-force top-K: insertion sort → max-heap** — `BruteForceIndex::search` replaced O(k) insertion sort with `BinaryHeap` O(log k) per candidate. At k=100 this is ~7× fewer comparisons per candidate.
- **`dist.rs` deleted** — dead scalar-only distance file (`euclidean_distance_squared`, `dot_product`, `euclidean_distance_fxp`) removed. All call sites redirected to `math::l2` / `math::dot`. Prevents future regression to scalar paths.
- **HNSW startup allocation eliminated** — `Vec::with_capacity(1_000_000 × dim)` replaced with `Vec::new()`. Removes up to 1.5 GB of committed virtual memory at startup for dim=384.
- **HNSW `id_map`: `HashMap` → `FxHashMap`** — uses identity-like hashing for integer keys; ~5–15% insert throughput improvement.
- **`dist::dot_product` callers migrated** — `engine.rs` and `cluster_server.rs` cosine-similarity helpers now call `math::dot::dot_i32` (SIMD) instead of the deleted scalar function.

### Internal (engine decomposition — not user-facing)

- **ExecutionResources (E4)** — `tree_cache` and `community_store` extracted
  from `Engine` into `pub resources: ExecutionResources`; application-layer
  boundary is now explicit in the type.
- **Hide pub state (E3)** — `Engine.state` changed to `pub(crate)`; 10 public
  read accessor methods added. Stale pre-E1 dual-branch patterns in valori-ffi
  removed. FFI `create_node` now routes through `create_node_for_record`.
- **NamespaceRegistry → CollectionRegistry (E2)** — duplicate `NamespaceRegistry`
  struct deleted from engine.rs; `valori-metadata::CollectionRegistry` is the
  single implementation. `list()` added to `CollectionRegistry`.
- **Single persistence funnel (E1)** — `Engine` now owns one `Persistence` enum
  (`EventLog` / `Wal` / `Ephemeral`); every mutation flows through one
  `commit_and_apply_ns` path. Behavior fix: event-log batch inserts now run the
  auto-tier index check (previously WAL-only).
- **Dead storage-layer duplicates removed (E0)** — 10 stale files in
  `valori-node/src/` deleted; `tests/architecture.rs` tripwire added to prevent
  re-introduction.

### Added
- **Dual-path unification, all mechanical domains (Phase R2)** — graph
  (7 endpoints), record deletion, metadata sidecar, and version handlers now
  share one body in `valori-node/src/routes/` served by both routers. Two new
  endpoints fell out of the unification: `POST /v1/soft-delete` on standalone
  (the engine always supported it; the route was missing) and
  `DELETE /v1/graph/node/:id` on cluster (commits `KernelEvent::DeleteNode`
  via Raft). The parity test's METHOD_GAPS list is now empty. Also fixed by
  construction: cluster `GET /v1/graph/nodes` no longer lists every
  namespace's nodes when `collection` is absent (tenant-isolation leak — now
  scopes to "default" like standalone); invalid node/edge kinds are 400 on
  both paths (standalone silently coerced them before); cluster `meta/set`
  answers `{"success":true}` (was `{"ok":true}`); unknown collections are 404
  on graph/delete endpoints on both paths. See
  `docs/phases/phase-R2-dual-path-domains.md`.
- **Dual-path unification (Phase R1)** — new `valori-node/src/routes/` module:
  shared HTTP handler bodies served by BOTH the standalone and cluster routers,
  starting with the collection endpoints (`/v1/namespaces*`). A new
  `tests/route_parity.rs` guard asserts the two routers expose identical `/v1`
  route sets (paths and methods) modulo explicit, documented allowlists — an
  endpoint added to only one router is now a test failure instead of a silent
  404. See `docs/phases/phase-R1-dual-path-unification.md`.

### Changed
- **`DELETE /v1/namespaces/:name` on an unknown collection now returns 404 on
  both paths** (standalone previously returned 400 while cluster returned 404).
- **Cluster `POST /v1/namespaces` now enforces the same name validation as
  standalone** (non-empty, ≤64 chars, `[a-zA-Z0-9_-]` only) — previously the
  cluster path committed unvalidated names straight through Raft.

- **Snapshot autosave + cluster lifecycle hardening (Phase 6.2)** — UI-launched project
  nodes now pass `VALORI_SNAPSHOT_INTERVAL=60` so a periodic snapshot is written even if
  the node is killed without a graceful close (the WAL was always durable; this keeps the
  next open instant and survives WAL-file loss). The deprecation warning on
  `VALORI_SNAPSHOT_INTERVAL` was removed — the replacement knobs
  (`VALORI_SNAPSHOT_EVERY_EVENTS/BYTES`) were parsed but never implemented, so the
  interval knob is the supported cadence control. Cluster mode gained a graceful-shutdown
  handler (SIGTERM/Ctrl-C drains axum and lets redb close cleanly). The UI close route
  now records the final record count in the manifest so at-rest project cards stay accurate.
  Verified end-to-end: standalone and 3-node cluster projects survive
  create → insert → close → reopen with records, collections, and search intact.
  See `docs/phases/phase-6.2-snapshot-autosave.md`.

### Fixed
- **Cluster search returned raw Q16.16 fixed-point scores** — `/search` on the cluster
  path serialized `score` as the raw `i64` kernel distance (e.g. `42954916`) instead of
  the float conversion the standalone path applies (`0.0100…`). The cluster `SearchHit`
  now divides by SCALE², matching standalone byte-for-byte across the plain, reranked,
  and decay-ranked paths. One SDK client now sees identical score scales on both.
- **Effect bus wiring for `POST /v1/records` (Phase A12)** — the standalone insert handler
  now routes through `EffectBus → EngineKernelCapability → Engine` via `run_graph_inline`,
  making the effect/planner pipeline live for the first time. `CapabilityRegistry` and
  `TaskRegistry` are built at router startup and injected as axum Extensions.
  `EffectError::Capacity` added so HTTP 507 (pool full) is still propagated correctly.
  `capabilities.rs` updated to the final `apply_command(body: &KernelCommandBody) → serde_json::Value`
  signature across all three kernel capability impls (`EngineKernelCapability`,
  `RaftKernelCapability`, `NoRaftKernelCapability`).
  See `docs/phases/phase-A12-effect-bus-wiring.md`.
- **Cross-shard timeline ordering validation (Phase S19)** — `GET /v1/timeline` on a
  multi-shard cluster now tags every event with `shard_id`, merges all shards' logs
  with a deterministic composite sort key `(timestamp_unix, shard_id, log_index)`,
  and actively rejects any shard log whose `log_index` sequence is non-monotonic in
  the merged output (HTTP 500 with a descriptive error). Standalone path unchanged
  (single shard, `shard_id: 0`). Covered by 1 new integration test.
  See `docs/phases/phase-S19-cross-shard-ordering.md`.
- **V4 event-log format with per-entry CRC32 (Phase S18)** — closes the silent
  corruption window where a bit-flipped entry decoded as valid bincode and was applied
  silently. New `VERSION_V4` segment: `encode_entry` appends a 4-byte LE CRC32 of the
  bincode payload; `decode_entry` rejects on mismatch with a descriptive error before
  the entry reaches the kernel. Chain hash is unchanged (CRC is transport-only, not
  part of the BLAKE3 chain formula). V2/V3 segments decode unchanged. 6 new hardening
  tests cover clean roundtrip, payload bit-flip, CRC tamper, and truncation.
  See `docs/phases/phase-S18-v4-per-entry-crc32.md`.
- **CRTS/BCRP snapshot roundtrip tests (Phase S17)** — 5 new tests in
  `engine_snapshot_roundtrip.rs` covering: decay timestamps (`created_at`) survive
  snapshot/restore; BM25 reranker corpus survives; both sections forward-compatible with
  snapshots that predate them (silent skip, no panic). Added `Engine::reranker_corpus_len()`,
  `Engine::reranker_rerank()`, `ValoriReranker::corpus_len()`.
  See `docs/phases/phase-S17-crts-bcrp-snapshot-tests.md`.
- **Multi-shard audit surface (Phase S16)** — `/v1/proof/event-log` now returns
  BLAKE3 hashes for every shard under `shards: { "0": {...}, "1": {...} }` (top-level
  `event_log_hash` is shard 0 for backward compat); `/v1/timeline` reads and merges
  all shards' audit logs sorted by wall-clock time; root cause fixed:
  `DataPlaneState.event_log_path` (shard 0 only) replaced with
  `shard_event_log_paths: BTreeMap<ShardId, PathBuf>`.
  See `docs/phases/phase-S16-multi-shard-audit-surface.md`.
- **Real `OperationHash` + extended write coverage (Phase A11)** — receipt bridge now
  uses the canonical RFC-0003 `OperationHash = BLAKE3(kind_discriminant ‖ bincode(inputs) ‖
  bincode(policy))` — reproducible from planning parameters, no timestamps involved.
  New `OperationKind`/`OperationInputs` variants: `Delete` and `BatchInsert`.
  Receipt emission extended to `batch_insert`, `delete_record`, and `soft_delete_record`
  on both standalone and cluster paths; cluster `delete_record` and `soft_delete_record`
  switched to `raft_write_data` to capture `log_index` as `committed_height`.
  See `docs/phases/phase-A11-real-op-hash.md`.
- **Receipt bridge wired into live handlers (Phase A10)** — `GET /v1/proof/receipt` now
  returns real per-operation receipts from actual HTTP traffic:
  - New `receipt_bridge.rs` — `emit_write()` (mutating ops) and `emit_read()` (read-only
    ops); each assembles a `Receipt` via `ReceiptAssembler` and pushes it into `ReceiptStore`.
  - Standalone `insert_record` — captures `state_before`/`state_after` via `hash_state_blake3`
    while holding the write lock; emits receipt after every successful insert.
  - Standalone `search` — captures current state hash at entry; emits read receipt on both
    no-decay and decay exit paths.
  - Cluster `insert_record` — gets `state_before` from `sm.state_hash().await`; switches to
    `raft_write_data` to read `resp.state_hash` + `resp.log_index` from the committed
    `ClientResponse`; emits receipt with real Raft log index as `committed_height`.
  - Cluster `search` — emits read receipt with shard state hash after results are computed.
  See `docs/phases/phase-A10-receipt-bridge.md`.
- **`RaftKernelCapability` in `valori-node` (Phase A9)** — real cluster `KernelCapability`
  backed by `raft.client_write()`. `apply_command()` deserializes `event_json → KernelEvent`,
  wraps in `ClientRequest { CURRENT_SCHEMA_VERSION, namespace_id, event, request_id }`, submits
  via Raft, and returns the post-apply `state_hash` hex from `ValoriStateMachine::state_hash()`.
  `NoRaftKernelCapability` renamed to a test-only stub (`is_available = false`).
  See `docs/phases/phase-A9-node-cleanup.md`.
- **`ReceiptAssembler` + `/v1/proof/receipt` (Phase A8)** — unified RFC-0003 proof type:
  - `Receipt` — identity, what ran, execution contract, state transition, Merkle DAG.
  - `ReceiptHash = BLAKE3(op_hash ‖ graph_hash ‖ state_before ‖ state_after ‖ sorted(parent_hashes) ‖ shard_id ‖ committed_height)` — `produced_at` excluded for determinism.
  - `ReceiptAssembler` — collects `ReceiptFragment`s per execution, sorts by `task_index`, assembles the final `Receipt`.
  - `verify_receipt()` — offline verifier: recompute hash, check fragment state chain, outer consistency.
  - `ReceiptStore` — in-process last-256 cache; evicts oldest on overflow.
  - `GET /v1/proof/receipt` — latest assembled receipt (both standalone and cluster).
  - `GET /v1/proof/receipt/:id` — receipt by receipt_id (both standalone and cluster).
  - `ReceiptStore` injected as `axum::Extension` into both routers.
  See `docs/phases/phase-A8-receipt-assembler.md`.
- **TaskRunner + real capabilities in `valori-node` (Phase A7)** — wires the effect
  system into the live node:
  - `EngineKernelCapability` — implements `KernelCapability` against `SharedEngine`:
    deserializes `event_json → KernelEvent`, calls `apply_committed_event_ns()`,
    returns the BLAKE3 state hash. Non-blocking `state_hash()` via `try_read()`.
  - `HttpEmbedCapability` — implements `EmbedCapability` by delegating to the
    existing `embed_batch()` HTTP client (Ollama / OpenAI / custom).
  - `PassthroughHttpCapability` — implements `HttpCapability` for outbound fetches.
  - `CapabilityRegistryBuilder` — assembles a `CapabilityRegistry` for standalone mode.
  - `TaskRegistry` — maps all 12 `TaskKind`s to `Arc<dyn Task>` (Embed/InsertRecord/Search
    are real; remaining kinds use `NoOpTask` until A8).
  - `TaskRunner` — drives one `ExecutionGraph` in topological order: builds `TaskContext`,
    resolves predecessor outputs, retries `TaskFailed` up to `policy.retry_limit`, marks
    `ExecutionHandle` at each step.
  - `run_graph()` — spawns a `TaskRunner` on the tokio runtime, returns `ExecutionHandle`.
  3 unit tests; 0 failures. All prior tests unaffected.
  See `docs/phases/phase-A7-task-runner.md`.
- **`valori-effect` effect system crate (Phase A6)** — defines the single routing
  layer between task execution and subsystems.
  - `EffectId = BLAKE3(execution_id ‖ task_topological_index ‖ effect_index)` — stable
    across retries; the bus deduplicates by this id, preventing double-writes.
  - `EffectDurability`: `Durable` (bus awaits completion) vs `Ephemeral` (fire-and-forget).
  - `EffectPayload` variants: `KernelWrite`, `Receipt`, `Audit`, `Counter`, `Gauge`.
  - `EffectBus`: `dispatch()` (dedup-checked for Durable) + `dispatch_all()` (skips
    duplicates silently). Routes `KernelWrite` → `KernelCapability::apply_command`,
    `Receipt`/`Audit` → `ProofCapability::append_fragment`.
  - 7 capability traits: `KernelCapability`, `EmbedCapability`, `LlmCapability`,
    `StorageCapability`, `HttpCapability`, `ProofCapability`, `SchedulerCapability`.
  - `CapabilityRegistry` — optional capabilities return `Err(CapabilityUnavailable)`.
  - `Task` async trait + `TaskContext` (bus, capabilities, budget) + `TaskOutput`.
  - Concrete tasks: `EmbedTask`, `InsertRecordTask` (Durable KernelWrite), `SearchTask`
    (Durable ReceiptFragment, read-only proof), `NoOpTask`.
  - `NoOpKernelCapability` for tests. 9 tests; 0 failures.
  See `docs/phases/phase-A6-valori-effect.md`.
- **`valori-planner` execution planning crate (Phase A5)** — converts `Operation`
  + `PlanningContext` into a deterministic `ExecutionGraph` DAG.
  - `Operation` — immutable unit of user intent: `hash = BLAKE3(kind ‖ inputs ‖ policy)`.
    `OperationInputs` captures planning parameters only (k, collection, shard_id,
    rerank, embed flags) — not actual data — so two searches with the same config
    share the same cached graph.
  - `PlannerFingerprint` — `BLAKE3(version ‖ routing_config_hash ‖ feature_flags_hash ‖ schema_version)`.
    Changes when planner behavior changes.
  - `PlanningContext` — fully-typed (no HashMap), deterministically serializable.
    `PlanningContextHash = BLAKE3(bincode(context))`.
  - `ExecutionGraph` — DAG of `TaskSpec`s. `GraphHash = BLAKE3(op_hash ‖ fp.hash ‖ ctx_hash ‖ topo_order)`.
    Built with Kahn's topological sort; equal inputs always produce equal hash.
  - `ExecutionCache` — bounded in-process `RwLock<HashMap>` cache.
  - `ExecutionHandle` — `tokio::watch` channel wrapping `ExecutionStatus` lifecycle.
  - `ExecutionRegistry` — top-level cache + active-handle index with `retire()`.
  - `NoOpPlanner` + `IngestPlanner` — concrete `Planner` implementations.
  - `plan_with_cache()` — two-layer cache lookup (in-process → durable `MetadataDb`) before fresh planning.
  16 tests; 0 failures. See `docs/phases/phase-A5-valori-planner.md`.
- **`valori-metadata` control-plane crate (Phase A4)** — redb-backed persistent
  store for all control-plane types: `Project` (name, dir, port, dim, index,
  shard_count, node_count, mode), `Collection` + `CollectionRegistry` (elevated
  form of the node's inline `NamespaceRegistry`), `ShardTopology`, `SnapshotCatalog`
  with `prunable(keep)` policy enforcement, `ExecutionRecord` + `ExecutionRetentionPolicy`
  (stub), `PlannerCacheKey/Entry` (stub). `MetadataDb` uses 5 typed redb tables.
  `valori-metadata` has no dependency on `valori-kernel` or `valori-storage` —
  pure control-plane. 13 tests.
  See `docs/phases/phase-A4-valori-metadata.md`.
- **`valori-state` state lifecycle crate (Phase A3)** — corrects the Phase A2
  placement error (`recovery.rs` was in `valori-storage` but orchestrates state
  lifecycle, not raw I/O). New crate owns: `bootstrap` (crash recovery via event
  log, WAL, or snapshot), `manifest` (`StateManifest` — which files make up
  durable state), `lifecycle` (`StateLifecycle`: Recovering/Ready/Snapshotting),
  `shutdown` (`shutdown_snapshot` — synchronous snapshot-on-close). `StateError`
  wraps `StorageError` and `KernelError`. `valori-node` re-exports
  `valori_state::bootstrap as recovery` — zero call-site changes.
  See `docs/phases/phase-A3-valori-state.md`.
- **Architecture specification (RFC-0)** — six RFC documents freeze the Valori
  execution model before further crate creation:
  - `rfcs/0000-glossary.md` — 16 canonical terms (Operation, ExecutionGraph,
    Task, Effect, EffectBus, EffectDurability, KernelCommand, KernelEvent,
    KernelABI, Receipt, KernelSnapshot, ExecutionSnapshot, KnowledgeGraph,
    KernelState, ClusterState, PlannerFingerprint, PlanningContextHash,
    Collection, Shard) each with Definition, Owner, Lifetime, and Invariant.
  - `INVARIANTS.md` — 15 numbered system invariants (I-01 through I-15)
    covering immutability, content-addressing, determinism, apply protocol,
    task isolation, effect routing, shard atomicity, receipt assembly order,
    and `no_std` boundary. Each tagged with the crates it governs.
  - `COMPATIBILITY.md` — version policy for KernelABI, snapshot format (V5/V6),
    event log format (v2/v3), PlannerFingerprint, wire types, HTTP API, and
    rolling upgrade (two consecutive minor versions allowed simultaneously).
  - `rfcs/0001-operation-lifecycle.md` — Operation, PlanningContext,
    PlannerFingerprint, ExecutionGraph, ExecutionHandle, ExecutionRegistry
    (split into Cache + History + Analytics), planner cache, lifecycle diagram.
  - `rfcs/0002-kernel-contract.md` — KernelCommand, CommandId, exactly-once
    dedup, apply protocol (DEDUP→APPLY→AUDIT), namespace isolation (3 points),
    no_std boundary, one-Task-one-transaction, verifier contract, valori-state scope.
  - `rfcs/0003-receipt-spec.md` — unified Receipt schema (KernelABI +
    PlannerFingerprint + CapabilitySet + state_hash_before/after + Merkle DAG
    parent_receipts), ReceiptFragment, ReceiptAssembler (topological sort, not
    completion order), offline verification algorithm, migration path from
    EventProof / MCP receipt / Tree-RAG receipt.
  - `rfcs/0004-capability-model.md` — Capability trait hierarchy
    (Kernel/Embed/Llm/Storage/Http/Proof/Scheduler), Effect enum variants with
    EffectDurability, EffectBus (dispatch + dedup), Task trait + TaskContext,
    capability checking at plan time.
  - `rfcs/0005-crate-boundaries.md` — full dependency graph, per-crate ownership
    table, no_std boundary line, phase sequencing constraints (A3→A9),
    cargo-deny enforcement rules.
- **`valori-storage` durable storage crate (Phase A2)** — WAL, event log,
  event journal, crash recovery, and object store (S3/file) extracted from
  `valori-node` into a new `valori-storage` crate. All 2,400+ lines of
  storage code now live in one place with their own 23 tests. `valori-node`
  re-exports all modules via `pub use valori_storage::*` so no existing
  imports change. `StorageError` defined; `From<StorageError> for EngineError`
  added for ergonomic propagation. See `docs/phases/phase-A2-valori-storage.md`.
- **`valori-core` zero-dependency type crate (Phase A1)** — all platform
  identity types (`RecordId`, `NodeId`, `EdgeId`, `NamespaceId`,
  `CollectionId`, `ExecutionId`, `ShardId`, `ClusterEpoch`), domain enums
  (`NodeKind`, `EdgeKind`), `Version`, and `CoreError` extracted into a new
  `no_std` crate. `valori-kernel` re-exports from it; every other crate will
  follow in subsequent phases. `valori-core` builds for
  `wasm32-unknown-unknown` with no OS dependencies.
  See `docs/phases/phase-A1-valori-core.md`.
- **Document update with chunk-level diffing (Phase I8)** — new
  `POST /v1/ingest/update` endpoint accepts a `document_node_id` (from a
  prior `/v1/ingest` response) plus new text. Diffs old vs new chunks by
  BLAKE3 content hash: unchanged chunks are kept in place (no re-embed),
  removed chunks are soft-deleted (vector + graph node), and only genuinely
  new or changed chunks hit the embedding provider. The document graph
  node is reused so external edges remain valid. Works in both standalone
  and cluster mode (shard-routed, all writes via Raft). Python SDK:
  `ingest_update()` on both `SyncRemoteClient` and `AsyncRemoteClient`.
  See `docs/phases/phase-I8-document-update.md`.
- **Replication factor in the project-creation wizard (Phase 6.1)** — the
  UI's "New Project" dialog now offers "Single Node" or "3-Node Cluster"
  (Raft-replicated, tolerates 1 node down) as a first-class creation
  choice, instead of clustering living only on the separate `/launch`
  power-user page. Cluster projects get a `nodes[]` manifest entry (legacy
  single-port manifests migrate automatically), a dedicated 4010-4999 port
  range that never collides with single-node projects (3010-3999) or the
  Launcher (3000-3009), per-node data files under the same project dir,
  aggregate "2/3 running" status in the UI, and full open/close/delete
  lifecycle across all nodes (open waits for full quorum health; close
  snapshot-stops every node and re-locks files at rest). The two
  previously-divergent dimension option lists are unified into one shared
  module, and `/launch` now imports the same cluster-config helpers
  instead of maintaining its own copies. Verified live end to end,
  including leader election, follower reads, and close→reopen data
  persistence. See `docs/phases/phase-6.1-project-wizard-replication.md`.
- **Shard count in the project-creation wizard (Phase S14)** — the UI's
  first surface for horizontal scaling. Creating a 3-node-cluster project
  now offers a "Shards" control (1/2/4/8); the choice is persisted in the
  project manifest and threaded to `VALORI_SHARD_COUNT` on every spawned
  node (one process per replica still — all shards on a node share its
  HTTP port and gRPC listener). Cluster projects only; standalone
  projects have no shard concept and pin to 1. Verified live end to end:
  a 3-replica/2-shard project produced six independently chain-valid
  per-node-per-shard audit logs (`valori-verify` on each). Requires
  Phase S13 (below) — shard count was not safe to expose while shards
  ≥ 1 silently discarded their audit trail. Known gap, disclosed in the
  wizard itself: Proof/Timeline pages still read shard 0's log only.
- **Shard routing completed across the entire cluster HTTP surface (Phases
  S5-S9)** — every collection-aware endpoint now routes to the shard that
  actually owns its namespace's data, closing out the routing work started
  in S3/S4:
  - **S5** — `cluster_insert_encrypted` routes by namespace;
    `DELETE /v1/crypto/shred/:key_id` fans out to every shard this node
    runs (ciphertext for one key can land on multiple shards) and
    aggregates per-shard status into `{"shredded": bool, "shards": {...}}`.
  - **S6** — linearizable reads are shard-aware:
    `ensure_read_consistency(shard_id, ...)` and
    `GET /v1/cluster/read-index?shard=N`; `cluster_memory_search` gained a
    read-index check it never had before (previously always
    eventually-consistent regardless of the requested `consistency`).
  - **S7** — core CRUD (`/v1/records`, `/v1/search`, `/v1/delete`,
    `/v1/soft-delete`, `/v1/vectors/batch-insert`) gained a `collection`
    field and shard routing, matching the standalone server's existing
    contract.
  - **S8** — graph node/edge CRUD (`/v1/graph/*`), `/v1/graphrag`, and
    namespace-scoped `/v1/community/detect` now route to their collection's
    shard.
  - **S9** — `cluster_ingest` gained automated test coverage via an
    in-process mock embed server; `cluster_tree_hybrid`'s vector-search
    section now routes to the resolved namespace's shard (previously
    resolved the namespace correctly but scanned shard 0 regardless — a bug
    flagged back in S1 and never revisited until now).

  See `docs/phases/phase-S5-crypto-shredding-cross-shard.md` through
  `docs/phases/phase-S9-ingest-coverage-tree-hybrid.md`.

- **Namespace→shard routing (Phases S3+S4)** — deterministic
  `shard_for_namespace(namespace_id, shard_count)` (`namespace_id % shard_count`,
  no placement table needed) and a multi-shard-aware `DataPlaneState`.
  `cluster_memory_upsert`, `cluster_memory_consolidate`,
  `cluster_extract_entities`, and `cluster_ingest` (writes) plus
  `cluster_list_nodes` and `cluster_memory_search` (reads) now route to the
  shard that actually owns a namespace's data, instead of always shard 0 —
  every collection-aware write handler is now shard-routed. `cluster_extract_entities`
  also had a latent id-allocation race fixed as part of making its routing
  safe (was pre-reading "next id" from the wrong shard's counter). See
  `docs/phases/phase-S3-shard-routing-infrastructure.md` and
  `docs/phases/phase-S4-remaining-write-handlers.md`.

### Fixed
- **Documents in named collections vanished after close/reopen (Phase S15)**
  — the standalone audit log recorded events without a namespace, so on
  recovery every event replayed into the default collection and the named
  collection came back empty. Data was never lost (the events were all on
  disk), just re-shelved into the wrong collection on each restart. Added
  an append-only `LogEntry::EventNs` wire variant that records the
  namespace; commit, replay, and every log reader (`valori-verify`,
  timeline, inspect, the legacy replication stream) are now
  namespace-aware. Default-collection logs stay byte-identical to before,
  and pre-S15 logs replay unchanged. Note: writes made *before* this fix
  stay in the default collection (their log entries lack the namespace);
  point-in-time `as_of` search in a non-default collection remains a known
  gap (the journal is namespace-agnostic). See
  `docs/phases/phase-S15-namespaced-event-log.md`.
- **Shards ≥ 1 silently discarded their audit trail (Phase S13)** —
  `bootstrap_cluster()` only ever gave shard 0 a real audit sink; every
  other shard got a hardcoded `NullAuditSink` that discards events without
  writing them to disk. This was an intentional S1-era decision made when
  no HTTP traffic could reach shard ≥ 1 — invalidated once S3-S9 wired real
  namespace→shard HTTP routing to every shard, but never revisited. Writes
  to a non-zero shard were still correctly Raft-committed and applied to
  that shard's `KernelState`, but had no BLAKE3 chain on disk. Every shard
  now gets its own genuine `events-shardN.log` (unchanged filename at
  `shard_count == 1`). A failure to open shard 0's audit log remains fatal
  (unchanged); a failure on shards ≥ 1 falls back to `NullAuditSink` for
  that shard only, logged loudly, rather than aborting the whole node —
  new capability this phase adds, no prior "fatal" guarantee to preserve
  there. See `docs/phases/phase-S13-per-shard-audit-sinks.md`.
- **Cluster mode's `GET /v1/graph/node/:id` and `GET /v1/graph/edges/:id`
  returned different field names than the standalone server (Phase S12)**
  — e.g. `{"id","kind","record"}` vs standalone's
  `{"kind","record_id","namespace_id"}`. Harmless for callers reading raw
  JSON, but the Python SDK's `walk()`/`expand()`/`neighbors()` read
  specific keys (`record_id`, `to_node`) and threw `KeyError` against
  cluster nodes. Predates S1-S11 entirely; found while documenting S11.
  Cluster now emits the same shape as standalone. `GET /v1/graph/subgraph`
  and `/v1/graphrag` were unaffected — they already shared one function
  between both modes.
- **Python SDK graph methods had no `collection` support (Phase S11)** —
  `create_node()`, `get_node()`, `create_edge()`, `get_edges()`,
  `subgraph()`, and `neighbors()` on both `SyncRemoteClient` and
  `AsyncRemoteClient` always targeted the default collection — the server
  side has always supported `collection` on these endpoints (and the
  cluster path routes it correctly as of S8), but the SDK never exposed
  it. All six gained a `collection: str = "default"` parameter,
  backward-compatible with every existing call site.
- **`valoricore-ffi` did not compile (Phase S10)** — `get_timeline()`'s
  exhaustive `KernelEvent` match was missing arms for
  `AutoCreateNamespace`/`DropNamespace` (added in S2). Predates the S1-S9
  sharding work — confirmed present on `main` before any of it. Fixed and
  verified with a real `maturin build --release` (the crate's actual build
  path; a bare `cargo build -p valoricore-ffi` never links successfully by
  design — PyO3's `extension-module` feature omits `libpython`).
- **Python SDK `soft_delete()` permanently deleted records instead of
  soft-deleting them (Phase S7)** — `SyncRemoteClient.soft_delete()` and
  `AsyncRemoteClient.soft_delete()` posted to `/v1/delete` (hard delete)
  instead of `/v1/soft-delete`, on both standalone and cluster targets.
  Fixed both methods to hit the correct endpoint; `crates/valori-node/README.md`'s
  API table had the same mislabeling, corrected, and `/v1/soft-delete`
  (previously undocumented) added as its own row. `delete()`/`soft_delete()`
  also gained an optional `collection` parameter on both clients (and their
  `ClusterClient`/`AsyncClusterClient` wrappers) — previously always scoped
  to the default collection regardless of where the record actually lived.
- **Collections/namespaces for graph data (nodes/edges) and vector-record
  writes were non-functional in cluster mode (Phase S3a)** —
  `ValoriStateMachine::apply()`'s generic dispatch always applied
  `AutoInsertRecord`/`AutoCreateNode`/`AutoCreateEdge` to namespace 0
  regardless of which collection a handler resolved (`cluster_memory_upsert`/
  `cluster_memory_consolidate` resolved a namespace id and then discarded
  it). Only the crypto-shredding path
  (`InsertRecordEncrypted`/`AutoInsertRecordEncrypted`) was genuinely
  namespace-scoped. Fixed by adding `namespace_id` to `ClientRequest`
  (`#[serde(default)]`, backward compatible) and threading it through
  `apply()`'s generic dispatch. Verified live: writes to two different
  collections now correctly land in their own namespaces (and, combined
  with the routing above, their own shards).
- **Cluster-mode collection creation was not Raft-replicated (Phase S2)** —
  `POST /v1/namespaces` mutated a private, per-node, in-memory registry
  directly. Two nodes could silently assign different `NamespaceId`s to the
  same collection name (or the same id to different names), and a follower
  would happily "succeed" against its own out-of-sync copy instead of
  redirecting to the leader. Now goes through Raft like every other write
  (`KernelEvent::AutoCreateNamespace`/`DropNamespace`); every node ends up
  with the identical, durable mapping, and a follower correctly
  307-redirects. See `docs/phases/phase-S2-namespace-replication.md`.
- **Snapshot `CapacityExceeded` at scale** — `encode_state` rewritten from a
  fixed `&mut [u8]` buffer to a growable `&mut Vec<u8>`. Snapshots above ~250K
  records (any dimension) previously failed with `Kernel(CapacityExceeded)`
  because the V6 schema added 10 bytes/record that the buffer-size formula did
  not account for. Verified end-to-end at 1M records (515 MB snapshot in 1.2 s).
  The encoder is now structurally incapable of this error. Stays `no_std`.
- **WAL loss on clean teardown** — added `impl Drop for Engine` and
  `impl Drop for EventCommitter` to flush the batched write buffer on scope
  exit. A clean shutdown could previously lose up to `flush_every` buffered
  events; recovery tests found 0 events after a simulated crash.

### Added
- **Multi-Raft consensus skeleton (Phase S1)** — a cluster process can now run
  multiple independent Raft groups ("shards") sharing one gRPC listener, each
  with its own persistent redb log, state machine, and leader election.
  New `VALORI_SHARD_COUNT` env var (default `1`, byte-identical to prior
  single-Raft-group behavior). Foundation for future namespace-sharded
  horizontal scaling — namespace→shard routing and HTTP-layer wiring are not
  part of this phase. See `docs/phases/phase-S1-multi-raft-skeleton.md`.
- **IVF centroid auto-scaling** (`n_list = max(16, sqrt(N))`, `n_probe = max(1, sqrt(n_list))`) — fixes a 153× QPS regression from 10K to 1M records. Centroids now scale with dataset size so average bucket size stays O(sqrt(N)) and scan cost is O(sqrt(N)) not O(N). Manual override via `VALORI_IVF_N_LIST` / `VALORI_IVF_N_PROBE` disables auto-scaling. Added `IvfIndex::needs_rebuild(count)` hook (returns true when online inserts exceed 2× the build size).
- **`encode_capacity_hint(state)`** — V6-correct pre-allocation estimate so the
  snapshot `Vec` avoids repeated reallocation on the hot path.
- **SIMD L2 distance** (`l2_sq_i32`) — NEON (aarch64) + AVX2 (x86_64) paths with
  scalar fallback; identical integer result on every path (determinism
  preserved), purely a speedup.
- **Benchmark suite** — `benchmarks/local_perf.py` (B1–B7) + `RESULTS_1M.md`,
  with a full performance section and HNSW-above-50K / small-batch warnings in
  the root `README.md`.

## [0.2.3] — 2026-06-29

### Security
- **SEC-2** `SyncRemoteClient` — bearer token was stored in `session.headers`
  (visible in `dict(session.headers)`, Python logging, and tracebacks). Ported
  the `_BearerAuth(requests.auth.AuthBase)` redaction pattern from
  `protocol.py`; token now injected per-request via `__call__`, never stored
  in the headers dict. `_BearerAuth.__repr__` returns `[REDACTED]`.
- **SEC-3** `ProtocolRemoteClient.set_metadata()` / `get_metadata()` — both
  called `session.post/get` without `auth=self._auth`, bypassing authentication
  even when an API key was configured. Fixed; all HTTP calls in
  `ProtocolRemoteClient` are now authenticated.
- **SEC-4** `set_metadata` — `metadata.decode(errors='replace')` silently
  corrupted binary metadata on round-trip (`b'\xff\xfe'` → garbage). Resolved
  by unifying the metadata type to `Dict[str, Any]` with a JSON codec; the
  corrupt decode path is gone entirely.

### Fixed
- **BUG-2** `ProtocolRemoteClient.upsert_text()` crashed with `KeyError` on
  every call — `res["proof_hash"]` hard-access on a field the server does not
  return. Changed to `res.get("proof_hash", "")`.
- **BUG-3** `test_batch_verify.py` called `exit(1)` at module scope when
  `VALORI_URL` was not set, killing the entire pytest process. Replaced with
  `pytest.skip()` inside the test function.
- **BUG-4** `record_count()` always returned 0 — `resp.json().get("record_count", 0)`
  but `/health` returns `{"records": {"live": N}}`. Fixed to
  `resp.json().get("records", {}).get("live", 0)` on both sync and async clients.
- **BUG-5** Duplicate, incompatible exception hierarchies — `protocol.py`
  defined its own `ValoricoreError`, `ValidationError`, `AuthError`,
  `ProtocolError` as separate classes from `exceptions.py`. `except
  valoricore.ValidationError` would not catch a `protocol.ValidationError`.
  Deleted the four duplicates from `protocol.py`; all now imported from
  `exceptions.py`. `ValidationError` now also inherits `ValueError`.
  `AuthError` kept as a backward-compat alias for `AuthenticationError`.
- **#3** `record_count()` — same as BUG-4 above (sync + async).
- **#4** `factory.py` — `Valoricore(remote=…, token=…)` silently dropped the
  token; `SyncRemoteClient` was constructed with no auth. Fixed by forwarding
  `token=token` in both `Valoricore` and `AsyncValoricore`.
- **#5** `ValoriClient` ABC added — shared interface for `LocalClient` and
  `SyncRemoteClient`. `LocalClient` methods widened to accept
  `collection/text/consistency/metadata_filter` kwargs (ignored with annotation)
  so factory-swapped code never raises `TypeError`.
- **#6** Metadata types unified — `insert_batch` now accepts
  `List[Optional[Dict[str, Any]]]` (SDK serialises each dict to a JSON string);
  `get_metadata`/`set_metadata` use `Dict[str, Any]` on all clients with JSON
  encode/decode. `LocalClient` stores as UTF-8 JSON bytes internally.
- **#7** `AsyncRemoteClient` timeout — constructor now accepts
  `timeout: float = 10.0` forwarded to `httpx.AsyncClient`; `AsyncValoricore`
  factory passes it through.
- **#8** BFS O(n²) — all three `walk()` implementations (`LocalClient`,
  `SyncRemoteClient`, `AsyncRemoteClient`) replaced `list.pop(0)` with
  `collections.deque` + `popleft()`.
- **#9** `EXPECTED_DIM = 384` removed from `memory.py`; dead imports cleaned
  from `protocol.py` and `async_memory.py`. `MemoryClient` already used
  `self._dim` for validation; the constant had no effect.
- **#10** Context-manager support — `SyncRemoteClient` gains `close()` /
  `__enter__` / `__exit__`; `AsyncRemoteClient` and both `ClusterClient`
  variants gain `__aenter__` / `__aexit__`.
- **#11** `__init__.py` module docstring — moved to first statement so
  `__doc__` is populated; RST grid table replaced with plain text readable in
  `help()` and `pydoc`.
- **#12** `ClusterClient.close()` — closes all N underlying `requests.Session`
  pools; adds `__enter__` / `__exit__`.
- **#13** `__version__` fallback — `except Exception` narrowed to
  `except PackageNotFoundError`; fallback changed from `"0.0.0"` to `"dev"` to
  distinguish an unregistered editable install from a real release.
- Test suite — 42 offline test failures resolved; `conftest.py` added with
  auto-skip for integration tests, env-var cleanup, and shared fixtures.
  `addopts = "-m 'not integration'"` means `pytest` on a clean checkout runs
  73 tests with 0 failures.

### Added (Phase I7 — Metadata filtering)
- **`metadata_filter` on `POST /search`** — optional JSON predicate that restricts
  results to records whose stored metadata satisfies all specified key-value conditions.
  Supports exact equality for strings/booleans/null and range operators (`gt`, `gte`,
  `lt`, `lte`, `eq`) for numeric fields. Example:
  `{"author": "Alice", "year": {"gte": 2020}}`. Both standalone and cluster paths
  are covered. When a filter is present the server over-fetches `k×10` candidates
  (capped at 5000) before post-filtering to ensure `k` results are returned.
- **Python SDK** — `SyncRemoteClient.search()` and `AsyncRemoteClient.search()` both
  accept `metadata_filter: Optional[Dict[str, Any]] = None`. `ClusterClient` and
  `AsyncClusterClient` inherit via `**kwargs`.

### Added (Phase I6 — Community layer: global sensemaking + entity extraction)
- **`POST /v1/community/detect`** — Label Propagation on the existing GraphNode
  adjacency list (pure Rust, zero LLM). Assigns every node a `community_id`,
  computes an f32 centroid vector per community (average of member FxpVectors),
  and emits a BLAKE3 receipt over the sorted `(node_id, community_id)` map —
  a tamper-evident proof of community structure at that point in time.
  Community store cached in-process; accessible by subsequent search calls.
- **`POST /v1/community/search`** — Cosine-similarity search over community
  centroids. Returns top-k communities ranked best-first with `member_count`
  and a `sample_node_ids` list. Answers "what are the themes across all
  documents?" — the global-sensemaking query that vector RAG cannot handle.
- **`POST /v1/ingest/extract-entities`** — Sends text to the configured LLM
  (reuses `VALORI_EMBED_PROVIDER` credentials — no new env vars). Parses
  `(entity, type, description)` tuples and `(source, target, description,
  strength)` relationships. Embeds entity descriptions and inserts them as
  `Concept` graph nodes with `Relation` edges — bridges a document graph into
  a true entity knowledge graph.
- All three endpoints exist in both **standalone** (`server.rs`) and **cluster**
  (`cluster_server.rs`) paths, following the mandatory dual-path rule.
- `valori-kernel`: added `incoming_edges()` on `KernelState` so Label
  Propagation can traverse both directions of the adjacency list.
- Python SDK: `community_detect()`, `community_search()`, `extract_entities()`
  on both `SyncRemoteClient` and `AsyncRemoteClient`.

### Added (Phase I5 — Tree-RAG: hierarchical retrieval with provable receipts)
- **`POST /v1/tree/build`** — parse a structured/markdown document into a
  navigable table-of-contents tree (sections, parent/child, line ranges).
  Deterministic, zero-LLM, zero-embedding. Returns `{node_count, structure_map, tree}`.
- **`POST /v1/tree/query`** — navigate the tree to the *right section* and answer
  with a breadcrumb + line-range citation and a BLAKE3-chained **retrieval receipt**.
  Distinguishes vocabulary-overlapping sections (e.g. "sick days" → *Sick Leave*,
  not *Annual Leave*) where plain vector search fails. Supports `prev_hash` to
  chain receipts.
- **`POST /v1/tree/verify`** — replay a receipt against the tree; `valid: false`
  proves the stored content was altered after retrieval (tamper detection).
- All three are stateless handlers — identical in standalone and cluster mode.
- Python SDK: `tree_build` / `tree_query` / `tree_verify` on both
  `SyncRemoteClient` and `AsyncRemoteClient`.

### Added (Phase I5 gap-fill — server-side tree cache + hybrid retrieval)
- **Server-side tree cache** — `Engine` (standalone) and `DataPlaneState` (cluster) now
  hold a `HashMap<String, TreeIndex>` keyed by `BLAKE3(text)`. `/v1/tree/build` stores the
  parsed tree and returns `cache_key` in the response. Subsequent `/v1/tree/query` and
  `/v1/tree/hybrid` calls accept `cache_key` instead of re-transmitting the full tree.
- **`POST /v1/tree/hybrid`** — single-call hybrid retrieval fusing tree-RAG section scores
  (term-frequency, normalized to [0,1]) with vector-search similarity scores (if
  `VALORI_EMBED_PROVIDER` is set). Configurable `tree_weight` (default 0.6). Returns merged,
  re-ranked hits with per-hit `source` tag (`"tree"` or `"vector"`), BLAKE3 receipt for the
  tree path, and a human-readable `reasoning` string. Available on both standalone and cluster.
- **`/v1/tree/build` and `/v1/tree/query`** are now stateful (take engine state for cache
  read/write); `/v1/tree/verify` remains stateless (no cache dependency).
- Python SDK: `tree_hybrid()` added to both `SyncRemoteClient` and `AsyncRemoteClient`.

### Added (Phase I4.1 — replicated metadata sidecar)
- **`KernelEvent::SetMeta { key, value }`** — new kernel event storing a
  replicated `meta` map on `KernelState`. Cluster ingest now writes the chunk/
  document metadata sidecar via `raft.client_write(SetMeta)` so **all** peers
  share it (previously node-local on the ingesting node only).
- **`/v1/memory/meta/set` + `/v1/memory/meta/get`** added to the cluster router,
  reading/writing through the kernel (`sm.with_state`) instead of a node-local map.

### Added (Phase I1/I2/I3 — Built-in ingest pipeline)
- **`POST /v1/ingest/document`** — server-side document chunking with five strategies:
  `auto` (sniffs text), `tree` (section headers), `conversation` (Q&A boundaries),
  `sentence` (sentence-window with ±2 context), `fixed` (overlapping windows).
  Returns `{strategy_used, chunk_count, chunks: [{index, title, text}]}`.
  Works in both standalone and cluster mode (stateless handler).
- **`POST /v1/ingest`** — full one-call pipeline: chunk + embed + insert + graph nodes +
  metadata sidecar. Requires `VALORI_EMBED_PROVIDER` (ollama / openai / custom).
  Supports `VALORI_EMBED_MODEL`, `VALORI_EMBED_URL`, `VALORI_EMBED_API_KEY`.
  Returns `{document_node_id, chunk_count, record_ids, strategy_used}`.
- **`/health`** now includes `embed_enabled: bool` and `embed_provider: string?` so
  clients can probe node capability before deciding on a pipeline.
- **`crates/valori-node/src/embedder.rs`** — HTTP embed client with Ollama fallback
  (`/api/embed` → `/api/embeddings`) and OpenAI-compatible batching.
- **Python SDK** — `SyncRemoteClient.chunk_document()`, `ingest()`,
  `AsyncRemoteClient.chunk_document()`, `ingest()`.
- **UI** — DocumentUploadTab probes node on mount; shows "Server-side pipeline active ⚡"
  banner and routes upload through `/v1/ingest` when embed is configured;
  falls back transparently to client-side pipeline otherwise.
- **Phase I4 — cluster ingest**: `POST /v1/ingest` now works in 3/5-node cluster mode.
  Vectors and graph nodes/edges go through `raft.client_write()` and are replicated to
  all peers. `DataPlaneState` gains `embed_config` and node-local `metadata` sidecar.
  `build_cluster_router` auto-reads `VALORI_EMBED_*` env vars. Cluster `/health`
  now exposes `embed_enabled` + `embed_provider`.

### Added (Phase C5 — Valori Reranker)
- **Valori Reranker** (`crates/valori-node/src/valori_reranker.rs`) — server-side hybrid
  retrieval that runs inside the node with no external dependency. Records inserted with a
  `text` field are tokenised and indexed. At search time, `query_text` triggers a two-stage
  pipeline: the kernel returns `k × POOL_FACTOR` candidates by vector similarity, the
  reranker blends vector and term-frequency scores (50 / 50), and the top-k are returned.
  Achieves **90 % accuracy** on hard lexical queries vs 60 % for LLM-based navigation, at
  **0.4 s** latency.
- `/records` and `/v1/vectors/batch_insert` accept `text` / `texts` fields for reranker
  indexing. `/search` accepts `rerank: bool` (default `true`) and `query_text: string`.
- `SyncRemoteClient` and `AsyncRemoteClient` updated: `insert(text=)`,
  `insert_batch(texts=)`, `search(rerank=True, query_text=)`, and new `health()` method.
- Cluster path: `ValoriStateMachine` stores raw texts in `text_corpus`; `cluster_server`
  builds a transient reranker per query from the corpus via `with_text_corpus()`.
- `KernelState::iter_records_in_ns(namespace_id)` — public iterator over records in a
  namespace, used by `drop_collection` to clean up the reranker on collection drop.

### Added (Phase 6 — Persistent, isolated projects in the UI)
- **Each UI project is now its own persistent, isolated workspace.** A project maps to one
  `valori-node` process with its own data dir, port, and WAL/snapshot under
  `~/.valori/projects/<name>/` (manifest at `~/.valori/ui-projects.json`, kept separate from
  the CLI wizard's `projects.json`). Home is now a project picker that lists every project
  from disk — even when all nodes are stopped — and one click resumes a session.
- **Auto-start on open / snapshot-on-close.** Opening a project boots its node and points the
  UI at it; closing writes a final snapshot, stops the node, and re-locks the files at rest.
- **Files are deletable only through the UI.** Data files carry the macOS immutable flag
  (`chflags uchg`; Linux falls back to read-only perms) while a project is at rest — Finder
  and `rm` refuse to remove them. The UI delete path clears the flag first.
- **Node graceful-shutdown snapshot.** Standalone `valori-node` now serves with a
  `SIGTERM`/`Ctrl-C` handler that writes a final snapshot to `VALORI_SNAPSHOT_PATH` before
  exiting — a durable backstop on top of the always-on WAL.
- New UI API routes `GET/POST /api/projects`, `DELETE /api/projects/[name]`, and
  `POST /api/projects/[name]/{open,close}`. The Launcher's defaults moved off `/tmp` to
  `~/.valori/cluster`.

### Changed (Python SDK — full endpoint coverage)
- **The Python SDK now wraps every product endpoint (40/40).** Newly added to `SyncRemoteClient` and `AsyncRemoteClient`:
  - **Agent-memory primitives** — `memory_upsert()` (`/v1/memory/upsert_vector`: store vector + document→chunk graph, returns `memory_id`) and `memory_search()` (`/v1/memory/search_vector`: hits carry `memory_id`, `metadata`, and decay fields). Previously only the lower-level `insert`/`search` (which return `{id, score}` with no `memory_id`/metadata) were exposed.
  - **Proof / provenance** — `event_log_proof()` (`/v1/proof/event-log`: the receipt primitive — event-log hash, state hash, committed height). Also on `ClusterClient`/`AsyncClusterClient`.
  - **Graph / introspection** — `list_nodes()` (`/graph/nodes`), `get_version()` (`/version`).
  - **Snapshot / object-store offload** — `save_snapshot()`, `restore_snapshot()`, `list_remote_snapshots()`, `upload_snapshot_to_store()`, `restore_from_store()`, `list_remote_wal()`, `archive_wal_segment()`.
- **Deprecated** `list_contradictions()` / `resolve_contradiction()` — legacy C3 methods that called the Next.js UI layer (`ui_url`), not the node, and returned whatever that layer held (historically `[]`). They now emit `DeprecationWarning` pointing to the node-native, audited `contradict()` / `consolidate()`. Scheduled for removal.

### Added (Phase C4.3 — Contradiction detection: self-maintaining memory, pillar 3)
- **`POST /v1/memory/contradict`** — given two record ids, computes cosine similarity between their Q16.16 vectors and, if it meets `threshold` (default 0.85), commits an `AutoCreateEdge(record_a → record_b, Contradicts)` to the BLAKE3 audit chain. Request `{ record_a, record_b, threshold?, collection? }`; response `{ record_a, record_b, similarity, contradicts, edge_id?, state_hash }` (`edge_id` only when `contradicts`). On both standalone and cluster data planes.
- **`EdgeKind::Contradicts = 8`** — new kernel edge kind (no_std-safe); the verdict is a first-class hashed event, not mutable metadata.
- **Python SDK** — `contradict(record_a, record_b, threshold=, collection=)` on all four clients; cluster variants route to the leader.
- **v1 boundary (documented):** "contradiction" is currently a structural proxy — cosine similarity ≥ threshold, which detects near-duplicates, *not* semantic NLI. The hashed `Contradicts` event path is signal-agnostic: a real entailment model can replace the cosine gate at the node layer with zero kernel change. See `docs/phases/phase-C4.3-contradiction.md`.

### Added (Phase C4.2 — Memory consolidation: self-maintaining memory, pillar 2)
- **`POST /v1/memory/consolidate`** — replace a memory in one auditable operation: commits `SoftDeleteRecord(old)` → `AutoInsertRecord(new)` → `AutoCreateEdge(new → old, Supersedes)` to the audit chain. Request `{ old_record_id, new_vector, collection?, metadata? }`; response `{ old_record_id, new_record_id, supersedes_edge_id, state_hash }`. On both standalone and cluster data planes.
- **`EdgeKind::Supersedes = 7`** — new kernel edge kind (no_std-safe) linking a replacement to the memory it retired, so a reader can trace why a record was soft-deleted.
- **Python SDK** — `consolidate(old_record_id, new_vector, collection=, metadata=)` on all four clients; cluster variants route to the leader.
- **Atomicity:** standalone is atomic (single engine write lock across all three events). Cluster commits the events as a sequence of Raft entries — each chain-valid and replicated, but not a single transaction; a mid-sequence leader crash can leave a partial result (follow-up: multi-event `ClientRequest`). See `docs/phases/phase-C4.2-consolidation.md`.

### Added (Phase C4.1b — Cluster decay + state-machine creation timestamps)
- **Cluster `/search` now honours `decay_half_life_secs`.** In C4.1 the cluster endpoint accepted the field but ignored it; now the consensus state machine tracks per-record creation timestamps (`StateMachineInner.created_at`, stamped at `AutoInsertRecord` apply time) and the cluster search path runs the same over-fetch → `decay::rerank` → top-k pipeline as standalone. One SDK call now behaves identically against both node types.
- **`ValoriStateMachine::record_created_at` / `with_state_and_timestamps`** — read accessors exposing creation time to the search path under one lock.
- **Determinism preserved** — `created_at` is a derived, non-hashed, non-replicated side map (same design as standalone `Engine.created_at`), so the BLAKE3 state hash is unchanged. Known boundary: a node that restarts or installs a snapshot loses timestamps and ranks pre-event records neutrally until re-stamped — durable WAL timestamps are deferred to **C4.1c**. See `docs/phases/phase-C4.1b-cluster-decay.md`.
- **Internal:** new `raft_write_data` helper returns the committed `ClientResponse` so cluster multi-step writes (consolidate/contradict) read allocated record/node/edge IDs from the apply response instead of pre-reading them — closing a TOCTOU race against concurrent writers.

### Added (Phase C4.1 — Kernel-native time decay: self-maintaining memory, pillar 1)
- **`decay_half_life_secs`** on `POST /search` and `POST /v1/memory/search_vector` — recency-aware re-ranking. When set (> 0), older records decay: a record one half-life old has its L2 distance doubled, so a fresh near-match can overtake a stale better one. Each hit gains `decay_factor` (∈ (0,1]) and `age_secs`; `score` stays the true, undecayed distance. Absent/`0` → byte-identical to the prior response.
- **`VALORI_DECAY_HALF_LIFE_SECS`** — optional server-default half-life; a per-request value wins (incl. an explicit `0` to disable).
- **Determinism preserved** — decay is a read-time re-rank: it never mutates kernel state, emits no event, and does not change the BLAKE3 state hash (regression-tested). Creation time lives in a derived, non-hashed `Engine.created_at` map stamped on live inserts only.
- **Python SDK** — `search(..., decay_half_life_secs=…)` on all four clients (`Sync`/`Async` `RemoteClient`, `ClusterClient` via `**kwargs`).
- **MCP** — `memory_recall` accepts `decay_half_life_secs` for recency-aware agent recall; the receipt still verifies over the decayed result set.
- **Supersedes the UI-only Phase C3** "self-maintaining memory," which shipped no decay and lived outside the audit chain. See `docs/phases/phase-C4.1-decay.md`.
- Known boundaries (v1): cluster decay is accepted-but-neutral (creation time isn't tracked in the consensus state machine yet — C4.1b); `created_at` is in-memory, so recovered records rank neutrally until re-stamped (durable WAL timestamps — C4.1b).

### Added (Phase 3.15 — Native GraphRAG: one-call retrieval)
- **`POST /v1/graphrag`** — retrieve the K nearest vectors **and** the connected knowledge subgraph around them in a single call, from one consistent kernel snapshot. Request `{ query_vector, k, depth, collection? }`; response `{ hits, seed_nodes, subgraph: { nodes, edges } }`. Added to both standalone and cluster data planes (cluster also honours `consistency`).
- **`memory_graph_recall` MCP tool** — GraphRAG with a receipt that binds **both** the hits and the returned subgraph (`receipt.subgraph = { node_ids, edge_ids }`, sorted). valori-mcp now exposes 7 tools.
- **Shared `graph_rag` module** (`expand_subgraph`, `resolve_seed_nodes`) — one BFS implementation reused by `/v1/graphrag`, `/graph/subgraph`, and the cluster equivalents, so the traversal stays identical across paths.
- **Python SDK** — `graphrag(query_vector, k, depth, collection, consistency)` on `SyncRemoteClient`, `AsyncRemoteClient`, `ClusterClient`, and `AsyncClusterClient` (cluster variants route to a read replica).
- Plain `memory_recall` receipts are unchanged on the wire (the new optional `subgraph` field is omitted when absent).

### Added (Phase 3.14 — MCP server: verifiable agent memory)
- **New crate `valori-mcp`** — a Model Context Protocol server (stdio, protocol `2024-11-05`) exposing a Valori node as verifiable, deterministic long-term memory for agents. New binary `valori-mcp`.
- **Six MCP tools** — `memory_write`, `memory_recall`, `memory_why`, `memory_timeline`, `memory_forget`, `memory_fork` — each a thin composition over existing node endpoints.
- **Retrieval receipts** — `memory_recall` returns a `receipt`: `receipt_digest = BLAKE3(canonical_json(body))` binding the exact result set to the committed `state_hash`, `event_log_hash`, and `committed_height` at recall time. Independently recomputable offline by any client, in any language.
- **`VALORI_URL` / `VALORI_AUTH_TOKEN`** (and `--url` / `--auth-token`) configure the node the MCP server talks to.
- **`examples/mcp_agent_memory.py`** — runnable end-to-end demo that boots a node, drives the MCP handshake, and re-derives the receipt digest in Python to prove cross-language verification. **`examples/claude_desktop_config.json`** — copy-paste client config.

### Added (Phase 3.13 — HNSW parameter exposure)
- **`VALORI_HNSW_M`** — sets max edges per node per layer; `m_max0` and `lambda` are derived automatically (`m_max0 = 2*M`, `lambda = 1/ln(M)`).
- **`VALORI_HNSW_EF_CONSTRUCTION`** — sets beam width during index build (default 100). Higher = better recall at the cost of insert throughput.
- **`VALORI_HNSW_EF_SEARCH`** — sets beam width floor during queries (default 50). Higher = better recall at the cost of query latency.
- **`GET /v1/index/config`** — new endpoint returning active index type and current HNSW parameters. Returns `{"index_type":"hnsw","hnsw":{"m":…,"m_max0":…,"ef_construction":…,"ef_search":…}}` for HNSW or `{"index_type":"brute_force","hnsw":null}` for brute-force.
- **Python SDK** — `SyncRemoteClient.get_index_config()` and `AsyncRemoteClient.get_index_config()` wrap the new endpoint.
- `HnswIndex::new_with_config(config: HnswConfig)` constructor; `HnswConfig` gains `ef_search` field.
- `Engine` stores `hnsw_config: HnswConfig` so `rebuild_index()` preserves operator-supplied parameters across crash recovery.

### Added (Phase 3.10 — Signed releases + SBOM)
- **cosign keyless signing** — every release binary and Docker image is signed
  using GitHub Actions OIDC → Sigstore transparency log. No private key to
  manage. Verify with `cosign verify-blob --certificate ... --signature ...`.
- **SPDX 2.3 SBOM** — `valori-sbom.spdx.json` generated via `cargo-sbom` on
  every release tag and attached to the GitHub Release with its own cosign
  signature.
- **Multi-platform binaries** — `linux/amd64`, `linux/arm64`, `darwin/amd64`,
  `darwin/arm64` in every GitHub Release alongside SHA-256 checksums.
- **SOC 2 evidence collection** — `scripts/soc2/collect_evidence.py` hits
  `/v1/proof/*`, `/v1/keys`, `/v1/cluster/status`, `/v1/storage/snapshots`
  and writes an evidence bundle with control-family mappings (CC6.6, CC7.2, A1.1, CC9).
- **Weekly evidence workflow** — `.github/workflows/soc2-evidence.yml` collects
  and uploads a 90-day-retained artifact bundle every Sunday at 02:00 UTC.

### Added (Phase 3.9 — Terraform modules)
- **`terraform/aws/`** — EKS cluster, VPC (3 AZs), S3 Object Lock bucket (KMS
  encrypted), IAM IRSA role for pod-level S3 access, ALB controller role,
  CloudWatch alarms for `state_hash_match` and replication lag.
- **`terraform/azure/`** — AKS cluster, Azure Blob Storage (ZRS, versioning,
  lifecycle policy), Key Vault (purge-protected, Premium SKU for Phase 5 CMK),
  Log Analytics workspace (90-day retention), Monitor alerts.
- **`docs/DEPLOY_AWS.md`** — Quick-start, variables, Helm deploy, cost estimate (~$575/mo).
- **`docs/DEPLOY_AZURE.md`** — Quick-start, SOC 2 KQL queries, CMK upgrade path, cost estimate (~$636/mo).

### Added (Phase 3.8 — Write-throughput regression gates)
- **`benchmarks/write_regression.py`** — Measures p50/p99 single-insert latency
  and batch throughput; compares against `benchmarks/baseline/write_regression_baseline.json`.
  Exit 1 if p99 grows > 15% or throughput drops > 10%.
- **`.github/workflows/write-regression.yml`** — Runs on every PR touching `crates/`.
  Builds release binary, starts node, runs benchmark, posts a warning comment on
  regression. Does not block merge (`continue-on-error: true`).
- **`benchmarks/baseline/write_regression_baseline.json`** — Seed baseline
  (p99 = 8 ms, throughput = 3 000 rps). Update via `--save-baseline` after
  deliberate perf improvements.

### Added (Phase 3.12 — Batch insert per-item idempotency)
- **Per-item `request_ids`** in `POST /v1/vectors/batch_insert` — each slot in
  the batch may carry an optional 32-hex idempotency key. A duplicate key is
  detected server-side (O(1) in-memory `FxHashMap`) and the previously assigned
  record ID is returned instead of creating a new record.
- **Mixed batches supported** — deduped and new items may be interleaved at
  arbitrary positions; the response `ids` array preserves original order.
- **Capacity guard accounts for deduped items** — a fully-deduped batch never
  trips the capacity limit.
- **Python SDK** — `insert_batch()` on all four client classes gains
  `request_ids: Optional[List[Optional[str]]] = None`.
- **4 new integration tests** in `tests/api_batch_idempotency.rs`.

### Changed (Phase 3.11 — Concurrent reads via RwLock engine)
- `SharedEngine` type changed from `Arc<Mutex<Engine>>` to `Arc<RwLock<Engine>>`;
  18+ read-only HTTP handlers now acquire a shared read lock, allowing concurrent
  search, proof, health, and timeline requests without serializing behind a global
  write lock. Write handlers (insert, delete, restore, crypto-shred, etc.) retain
  the exclusive write lock.
- `main.rs` auto-snapshot task uses `.read().await` (snapshot is read-only).
- Replication hash-checker and start-offset reads use `.read().await`.

### Added (Phase 3.6 — Crypto-shredding / GDPR erasure)
- **AES-256-GCM per-record encryption** — `POST /v1/records/encrypted` encrypts
  a binary payload before storing; the vector slot is zeroed (not searchable).
  Returns `{"id": int, "key_id": str}`. Group multiple records under one
  `key_id` to shred them atomically.
- **Cryptographic key destruction** — `DELETE /v1/crypto/shred/:key_id` destroys
  the DEK; all records encrypted under that key become permanently unrecoverable
  (GDPR Article 17 "right to erasure" via key destruction, not log truncation).
- **Key existence check** — `GET /v1/crypto/status/:key_id` returns
  `{"exists": bool}`.
- **`VALORI_SHRED_LOG_PATH`** — optional env var; shredded key_ids are appended
  to this file so they remain unrecoverable across restarts.
- **Python SDK** — `insert_encrypted()`, `shred_key()`, `shred_key_status()`
  added to both `SyncRemoteClient` and `AsyncRemoteClient`.
- **Kernel invariants** — `FLAG_ENCRYPTED` (0x02) and `FLAG_SHREDDED` (0x04)
  now fully implemented; `is_searchable()` added to `Record`; shredded records
  are excluded from search, iteration, and index rebuild.
- **Audit chain preserved** — encrypted/shredded record slots remain in the
  BLAKE3 hash chain; the flags byte proves shredding happened without exposing
  plaintext.
- **5 new integration tests** in `tests/api_crypto_shred.rs`.

### Added (Phase 3.7 — `valori import` — provable migrations)
- **`valori import qdrant`** — imports from a Qdrant collection via the scroll
  API. Detects source dimension automatically and aborts with a clear error if
  it mismatches the Valori node's `VALORI_DIM`. Cursor-based pagination;
  per-record idempotency keys ensure exactly-once delivery even on retry.
  Supports `--resume` via a `.valori-import-qdrant-<collection>.json` sidecar
  (tracks `last_offset` + import count across interruptions). Progress bar via
  `indicatif`; state hash printed on completion.
- **`valori import jsonl`** — imports from a JSONL file
  (`{"vector": [...], "metadata": "...", "tag": 0}` per line). Accepts aliases
  `embedding`/`values` for the vector field and `text`/`content`/`payload` for
  metadata. Skips malformed or wrong-dimension lines with a warning; does not
  abort the whole import.
- **Dim validation before any data write** — both subcommands call
  `GET /health` and compare the node's declared `dim` to the source before
  touching any data.
- **Auto-create target collection** — if the target collection doesn't exist,
  it is created before the first insert (idempotent; `400 Already Exists` is
  swallowed).
- **No new dependencies** — uses `ureq` + `indicatif` + `chrono` already in
  `valori-cli`'s dep tree.

### Added (Phase 3.5 — Per-tenant API Keys + RBAC)
- **`POST /v1/keys`** — create a scoped API key (`read_only`, `read_write`, or
  `admin`). Returns the plain-text token once; thereafter only the BLAKE3 hash
  is stored. Accepts optional `collection` lock and `description`.
- **`GET /v1/keys`** — list all keys (masked — `prefix` + metadata, no raw token).
  Requires `admin` scope.
- **`DELETE /v1/keys/:id`** — revoke a key. Audit-safe: key is removed from the
  store immediately; the `events.log` is not affected.
- **`VALORI_KEYS_PATH`** — new env var (JSON file); key store survives restarts
  when set. Absent = in-memory only.
- **`VALORI_AUTH_TOKEN` legacy fallback** — existing static tokens continue to
  work; the new key store is checked first, then the static token as a fallback
  (treated as admin scope).
- **`build_router_with_keys()`** / **`build_cluster_router_with_keys()`** — new
  router builders used by `main.rs`; existing `build_router()` unchanged
  (in-memory key store, no breaking change for tests).
- **Scope enforcement at middleware layer** — routes auto-classified as
  read-only, read-write, or admin by path + method without per-handler changes.
- **8 new integration tests** in `crates/valori-node/tests/api_keys.rs`.

### Added (Phase 3.3 — Cluster-aware Python SDK)
- **`ClusterClient`** — new sync multi-node client. Takes a list of node URLs;
  routes writes to the discovered leader, round-robins local reads across all
  replicas, and upgrades to linearizable reads on request. Leader is discovered
  from the first 307 redirect and cached; failover resets the cache and
  self-heals on the next call.
- **`AsyncClusterClient`** — async mirror backed by `AsyncRemoteClient`.
  `cluster_health()` fans out with `asyncio.gather`. `close()` shuts down all
  underlying httpx clients.
- **`SyncRemoteClient.insert()`** — now auto-generates a UUID4 idempotency key
  and sends it as `"request_id": [u8; 16]` in the JSON body on every call.
  The key is identical across all retry attempts, enabling server-side dedup
  when a write was applied before a connection reset. Pass `idempotency_key=`
  to supply your own token.
- **`SyncRemoteClient.delete()` / `soft_delete()`** — same idempotency key
  handling.
- **`SyncRemoteClient.leader_url()`** — expose the cached leader base URL.
- **`SyncRemoteClient.get_cluster_role()`** / **`AsyncRemoteClient.get_cluster_role()`**
  — `GET /v1/cluster/role` → `"leader"` | `"follower"`.
- **`AsyncRemoteClient.timeline()`** — replaced `aiohttp` with the existing
  `httpx.AsyncClient` (`self.client`); eliminates the mixed-client inconsistency.
- `ClusterClient` and `AsyncClusterClient` exported from `valoricore` package.

### Added (Phase 3.4 — As-of / Point-in-Time Reads)
- **`POST /search`** — new optional fields `as_of` (ISO 8601 UTC string) and
  `as_of_log_index` (u64). When either is set the server replays committed
  events up to the target, searches the resulting state, and returns
  `as_of_log_index`, `as_of_timestamp_iso`, and `as_of_state_hash` (BLAKE3
  hex) alongside the hit list. Requires `VALORI_EVENT_LOG_PATH`.
- **`GET /v1/timeline`** — upgraded from a raw string list to structured JSON
  (`TimelineResponse`). Accepts `from=<ISO8601>` and `to=<ISO8601>` query
  params for timestamp range filtering. Each entry includes `log_index`,
  `timestamp_unix`, `timestamp_iso`, `event_type`, and per-event IDs.
- **`EventJournal`** — now stamps each committed event with a wall-clock
  unix-second timestamp. New methods: `committed_with_timestamps()`,
  `find_log_index_at_or_before()`, `event_timestamp()`.
- **Python SDK** — `SyncRemoteClient.search()` and `AsyncRemoteClient.search()`
  gain `as_of` and `as_of_log_index` params. New `timeline()` method on both.
- **6 new integration tests** in `crates/valori-node/tests/api_as_of.rs`.

### Added (Phase 2.10d — Partition Harness)
- **`crates/valori-consensus/tests/partition_scenarios.rs`** — three new
  integration tests for the in-process partition harness:
  - `asymmetric_partition_lagging_node_catches_up` — one-directional link block
    (leader → follower); 2/3 quorum commits; lagging node catches up and all
    three BLAKE3 hashes converge.
  - `blake3_chain_consistent_across_partition_and_heal` — full compliance proof:
    isolated-leader's hash is frozen during a symmetric partition, and after heal
    all 3 replicas share the same BLAKE3 state hash over all 6 records.
  - `isolated_node_hash_frozen_then_converges` — confirms an isolated follower
    cannot fork the audit chain; hash is frozen during isolation and adopts the
    majority chain after heal.
- All 3 new tests pass (0.73 s); full `valori-consensus` suite clean.

### Added (C3 — Self-Maintaining Memory)
- **Global entity registry** (`ui/src/app/api/ingest/route.ts`) — before creating a
  Concept node, checks `entity:<collection>:<normalized_label>` in the metadata sidecar.
  Existing nodes are reused across documents and ingest sessions so the same real-world
  entity converges to a single graph node.
- **Content dedup** — per-chunk SHA-256 computed before embedding. Exact duplicates
  (`content:<collection>:<sha>` already registered) skip the vector insert entirely.
  `dedup_skipped` count returned in ingest response; `dedup: true` flag per chunk.
  `content_sha256` stored in sidecar for external verification.
- **Contradiction detection** — after each ingest, `detectContradictions()` runs
  async (fire-and-forget). Similarity > 0.92 with a different source document queues
  a `contradiction:<id>` entry with `status: "pending"`.
- **`GET /api/contradictions`** — lists pending/dismissed/superseded contradictions
  for a collection with chunk text preview.
- **`POST /api/contradictions`** — resolve: `dismiss` (both valid) or `supersede_b`
  (marks `record_b` sidecar as `superseded: true`).
- **Supersession filter in `/api/why`** — chunks with `metadata.superseded === true`
  are excluded from vector search results. Kernel record is immutable (audit trail
  preserved); only retrieval is suppressed.

### Added (C2 — Audited Entity Graph + Provenance Receipt)
- **`GET /graph/subgraph?root=<id>&depth=<d>`** — bounded BFS (depth capped at 4)
  returning all reachable nodes and edges. Added to both `server.rs` (standalone)
  and `cluster_server.rs` (cluster, respects readiness gate).
- **Entity extraction at ingest** (`ui/src/app/api/ingest/route.ts`) — when
  contextual enrichment is enabled, extracts up to 8 named entities per chunk via
  the configured LLM. Creates `NodeKind::Concept` nodes + `EdgeKind::Mentions`
  edges (chunk → concept), deduplicated within the ingest session via a
  `entityNodeMap`. Entity labels are stored in the metadata sidecar.
- **Provenance subgraph in receipt** (`ui/src/app/api/why/route.ts`) — after
  graph expansion, calls `/graph/subgraph?depth=1` for each top-5 chunk node and
  collects traversed nodes + edges. Entity labels fetched for Concept nodes.
- **Receipt schema** (`ui/src/lib/receipts.ts`) — `ReceiptGraphNode` and
  `ReceiptGraphEdge` interfaces added. `ServerReceiptPart` and `AnswerReceipt`
  gain `provenance_nodes` and `provenance_edges` arrays.
- **Bug fix**: `Document→Chunk` edge kind corrected from `0` (Relation) to `6`
  (ParentOf) in the ingest route.

### Added (C1 — Contextual Retrieval + Audited Enrichment)
- **Audited context sentences** — `BatchInsertRequest` now accepts
  `metadata: Option<Vec<Option<String>>>`. Per-vector UTF-8 metadata blobs are
  committed into `KernelEvent::InsertRecord.metadata` / `AutoInsertRecord.metadata`,
  included in the BLAKE3 audit chain, and replicated through Raft. The cluster ingest
  path (`cluster_server.rs`) previously always passed `metadata: None` — fixed.
- **Contextual enrichment at ingest** (`ui/src/app/api/ingest/route.ts`) — when
  enabled, generates a one-sentence LLM context per chunk before embedding and
  commits it as `{"doc","n","total","ctx"}` JSON in the audited metadata field.
  Concurrency limit: 6 parallel LLM calls via `Promise.allSettled`. Failure is
  graceful (ingest continues without enrichment, `enriched: false` in receipt).
- **Tier-2 reranker** (`ui/src/app/api/why/route.ts`) — optional cross-encoder
  reranker (Cohere or custom endpoint) applied after vector search. Failure is
  silent. `rerank_score: number | null` per chunk + `reranked: boolean` flag are
  written into the proof receipt so non-determinism is documented, not hidden.
- **Receipt schema** (`ui/src/lib/receipts.ts`) — `ReceiptChunkRef` gains
  `rerank_score: number | null` and `enriched: boolean`. Both additive, no version
  bump needed within `"1.0"`.
- **Settings → Tier-2 Reranker** (`ui/src/app/settings/page.tsx`) — Disabled /
  Cohere / Custom endpoint toggle persisted in `localStorage["valori:reranker_config"]`.
- **DocumentUploadTab** (`ui/src/components/ingestion/DocumentUploadTab.tsx`) — adds
  per-upload contextual enrichment toggle that passes LLM params to the ingest route.
- **AskTab** (`ui/src/components/collections/AskTab.tsx`) — loads reranker config
  from localStorage and passes it to `/api/why` on each question.

### Added (C0 — Eval Harness)
- **`scripts/eval/eval.py`** — Python eval harness with three subcommands: `probe`
  (health check, no embedding needed), `seed-eval` (seeds 10 records, embeds,
  searches, measures recall@k + provenance integrity; CI gate exits 1 if
  recall@1 < 0.8 or citation_existence < 1.0), `verify` (verifies
  `content_sha256` in saved receipt JSON files against a live node).
- **`scripts/eval/qa_sets/bootstrap.jsonl`** — 10 bootstrap QA entries labeled
  `[bootstrap]`. Not for external claims; replaced with real corpus when available.
- **`ui/src/lib/receipts.ts`** — receipt schema frozen at `version: "1.0"`.
  Breaking changes must bump `RECEIPT_VERSION`.
- **`docs/phases/phase-C0-cortex-plan.md`** — full converged Cortex plan (5
  contradiction cycles, 34 items, 4-point moat statement).

### Fixed (B13 — Startup Readiness Gate)
- **Partial-state-on-restart bug fixed** (`valori-node`) — cluster nodes no longer
  serve `Local`-consistency reads during the openraft log-replay catch-up window that
  follows a restart. Reads now return HTTP 503 (`Retry-After: 1`) until the node has
  replayed all entries committed before shutdown.
- **`ReadinessGate`** added to `cluster_server.rs` — atomic latch initialized from
  `startup_committed_index` (read from the redb `KEY_COMMITTED` entry before Raft
  opens). Latch opens permanently once `applied_index >= startup_committed_index`;
  fresh/in-memory nodes get `target=0` and are immediately ready.
- **Explicit snapshot cadence** (`cluster.rs`) — `SnapshotPolicy::LogsSinceLast(n)`
  now explicitly configured (default 5000, overridable via
  `VALORI_SNAPSHOT_EVERY_EVENTS`) instead of relying on openraft's implicit default,
  bounding the maximum catch-up window after restart.

### Added (B13 — env vars)
- `VALORI_SNAPSHOT_EVERY_EVENTS` — trigger a Raft snapshot every N applied entries
  (default 5000). Lower values reduce restart catch-up latency at the cost of more
  frequent snapshot I/O.
- `VALORI_RAFT_SNAPSHOT_KEEP` — log entries to retain after snapshot for followers
  that are slightly behind (default 1000).

### Added (Phase 3.2 — Rolling Upgrades)
- **`schema_version` field on `ClientRequest`** (`valori-consensus`) — the
  leader stamps `CURRENT_SCHEMA_VERSION` (currently `0`) on every proposal. Old
  nodes decode the field as `0` via `#[serde(default)]`.
- **`CURRENT_SCHEMA_VERSION: u8 = 0`** constant (`valori-consensus::types`) —
  single source of truth for the cluster wire version. Bump when a new
  `KernelEvent` variant or breaking field change requires newer followers.
- **Schema version gate in `ValoriStateMachine::apply()`** — followers reject
  entries with `schema_version > CURRENT_SCHEMA_VERSION` with `StorageError`
  (halts replication on that node; cluster continues through remaining quorum).
  State and audit log are untouched on rejection.
- **`valori cluster upgrade --url … --target-version …`** CLI command — interactive
  guided rolling upgrade: discovers topology, upgrades non-leaders first then
  leader, polls `/health` after each restart, waits for re-election before
  declaring the leader step complete.
- **`docs/COMPATIBILITY.md`** — schema version history, rolling-window rules,
  coexistence matrix, and the procedure for bumping `CURRENT_SCHEMA_VERSION`.

### Fixed (Phase 3.2)
- `corrupted_snapshot_payload_is_refused_and_state_kept` snapshot corruption
  test was flipping byte `bytes.len() / 2` which, for V6 snapshots (8318 bytes),
  lands in the namespace sentinel region not covered by `hash_state_blake3`.
  Fixed to corrupt `bytes.last_mut()` (last byte of the `state_hash` tail),
  which always triggers the hash mismatch check regardless of format version.

---

## [0.2.1] — 2026-06-19

### Added
- **Multi-tenant collections** — up to 1 024 named namespaces per node.
  `POST /v1/namespaces`, `GET /v1/namespaces`, `DELETE /v1/namespaces/:name`.
  All data endpoints accept an optional `"collection"` field. Records are
  isolated at the kernel level via intrusive per-namespace linked lists enforced
  at three independent points (event-commit, WAL replay, `build_index`).
- **`AutoCreateNode` / `AutoCreateEdge` kernel events** — graph mutations with
  IDs assigned at apply time for deterministic cluster-mode graph operations.
- **Persistent Raft state machine** — when `VALORI_RAFT_LOG_PATH` is set, the
  state machine shares the redb file and persists `last_applied`, membership,
  and the latest snapshot, preventing duplicate audit-log writes on restart.
- **Replay suppression** — `replay_until` suppresses already-written audit
  entries when openraft replays committed log entries after a restart.
- **`GET /v1/cluster/role`** — current node's Raft role for load-balancer routing.
- **`state_hash_match` Prometheus gauge** — cluster-wide hash-convergence metric.
- **Snapshot V6 format** — per-record `namespace_id` + linked-list pointers,
  2 × 1 024 × 4 = 8 KB namespace heads arrays, and a backward-compatible NSRG
  section (namespace registry as JSON, detected by `"NSRG"` magic tag).
- **Python SDK collection API** — `create_collection`, `list_collections`,
  `drop_collection` on both `SyncRemoteClient` and `AsyncRemoteClient`;
  `collection` parameter on all data methods; `consistency` parameter on search.
- **Threat model** (`docs/THREAT_MODEL.md`).
- **Capacity planning** (`docs/CAPACITY.md`).
- **DR & multi-region runbook** (`docs/DR.md`).
- **Multi-arch hash benchmark** (`benchmarks/multi_arch_hash.py`).
- **Q16.16 precision benchmark** (`benchmarks/q16_precision.py`).
- **Helm snapshot CronJob** (`deploy/helm/valori/templates/snapshot-cronjob.yaml`).
- **CI test-count workflow** (`.github/workflows/test-count.yml`).

### Fixed
- `LeaderClient::get_proof()` wire-format mismatch — server returns
  `{"final_state_hash":"<hex>"}` but client expected `[u8; 32]`. Added
  `LeaderProof { final_state_hash: String }` and updated hex comparison in replication.
- Snapshot buffer too small for V6 in `format.rs` and `snapshot_roundtrip.rs`
  (4 KB → 16 KB).
- `spawn_state_hash_watcher` held `Arc<Database>` indefinitely, blocking redb
  file re-open on restart. Now returns `JoinHandle`, stored in `ClusterHandle`,
  aborted and awaited before shutdown.
- arXiv paper title corrected from *"Deterministic Memory: A Substrate for
  Verifiable AI Agents"* to *"Valori: A Deterministic Memory Substrate for
  AI Systems"* in README and BibTeX.
- Hardcoded test count badge (271) replaced with CI-driven workflow badge.
- Python SDK version badge corrected from v0.1.11 to v0.2.1.
- Apply-vs-audit ordering invariant now explicitly documented with crash-window
  analysis in `valori-consensus/README.md`.
- Comparison table "No" cells now cite competitor documentation.

### `valori_raft_state_hash_match` Prometheus gauge — a background task on
  each cluster node periodically calls `/v1/proof/state` on every peer and
  publishes `1` when all reachable nodes agree on the BLAKE3 state hash, `0`
  when any peer diverges. Mismatches are also logged at `ERROR` level and
  counted by `valori_raft_divergence_detections_total`. Configurable via
  `VALORI_STATE_HASH_CHECK_SECS` (default 30 s; `0` disables).
- **`GET /v1/cluster/role`** endpoint — returns `{"role":"leader"|"follower",
  "node_id":N,"current_leader":N}` on any node. Designed for load-balancer
  health-check routing: steer writes at the pod that answers `"leader"` to
  avoid 307 redirect round trips on every write.
- **Proptest event-sequence fuzz** (`crates/valori-consensus/tests/proptest_event_fuzz.rs`)
  — 32 randomly generated insert/soft-delete/delete sequences applied through
  a 3-node in-process cluster, asserting all nodes converge to the same BLAKE3
  state hash after each sequence. Shrinks failing cases automatically.
- **Helm chart** (`deploy/helm/valori/`) — production StatefulSet with
  PersistentVolumeClaims for `events.log` and `raft.redb`, headless service
  for stable pod DNS, client service, and configurable liveness/readiness
  probes pointing at `/v1/cluster/health` and `/health`. Topology spread
  anti-affinity keeps pods on separate availability zones by default.

- **Automatic `events.log` rotation** on both write paths — the standalone
  `EventCommitter` and the cluster `EventLogAuditSink` seal the live segment to
  `events.log.NNNNNN` once it passes `VALORI_EVENT_LOG_ROTATION_BYTES` (default
  256 MiB; `0` disables), opening a fresh segment that splices from the sealed
  one's chain head.
- **Multi-segment recovery** — recovery now discovers and replays every local
  segment (sealed archives + live file) in sequence order and verifies each
  splice point.

- **Linearizable reads via the read-index protocol** (now the default read
  consistency). The leader serves through openraft's `ensure_linearizable()`;
  a follower fetches the leader's read index from the new
  `GET /v1/cluster/read-index` endpoint, then waits for its own apply to catch
  up before scanning local state. Clients can opt into a faster,
  eventually-consistent read with `consistency: "local"` (Python SDK:
  `search(..., consistency="local")`).

### Fixed
- Rotated logs previously recovered **only the live segment**, silently dropping
  all pre-rotation history; recovery is now multi-segment and lossless.
- Archive segments are named by monotonic segment sequence instead of a
  wall-clock timestamp, so two rotations within the same second no longer
  collide and clobber an earlier archive.

## [0.2.0] — 2026-06-13

The multi-node release. Valori graduates from a single standalone node to a
Raft-replicated cluster with verifiable, crash-symmetric state on every replica.

### Added
- **Raft consensus layer** (`valori-consensus`) over openraft 0.9: replicated
  log store (in-memory + persistent `redb`), `KernelState` state machine with
  the audit-log write at apply time, and a tonic/gRPC peer transport.
- **Cluster mode** for `valori-node`: boot-time dispatch on
  `VALORI_CLUSTER_MEMBERS`, leader-redirect (`307 + Location`) for writes,
  local reads on any replica, and a `/v1/cluster/*` management plane
  (status, health, add-node, remove-node).
- **Mutual TLS** on the Raft channel (`VALORI_TLS_*`), enforced at the
  handshake against a shared cluster CA.
- **Persistent Raft log** via embedded `redb` (`VALORI_RAFT_LOG_PATH`) — the
  log and vote survive process restarts.
- **Raft metrics** exported on `/metrics` (term, leader, log/apply lag,
  snapshot/purge indexes).
- **State-machine ID allocation** (`KernelEvent::AutoInsertRecord`): record IDs
  are assigned deterministically at apply time, removing the per-node insert
  mutex and retry loop.
- **Cluster data-plane endpoints**: `/v1/delete`, `/v1/soft-delete`,
  `/v1/vectors/batch_insert`, `/v1/proof/state`.
- **Interactive setup wizard** (`valori setup`): pick architecture and node
  count, start an in-process cluster, and drive inserts/search/membership from
  a live menu. Projects persist to `~/.valori/projects.json`.
- **`valori cluster` CLI**: operate a running cluster (status, health,
  add-node, remove-node) against any node's HTTP API.
- **Docker deployment**: distroless multi-stage `Dockerfile` with a built-in
  `--health-check` TCP probe, and a 3-node `docker-compose.yml`.
- **Partition harness**: in-memory switchable-transport test suite covering
  leader isolation, re-election, partition heal/convergence, and the
  minority-cannot-commit invariant.

### Changed
- Cluster search now uses the kernel's maintained index via `search_l2`
  instead of an ad-hoc record-pool scan.
- Workspace versioning unified at `0.2.0` via `[workspace.package]`; all crates
  inherit version, edition, and license.

### Fixed
- `Dockerfile` now copies all workspace member manifests so workspace
  resolution succeeds; healthcheck no longer references a non-existent flag.

### Repository
- Removed scratch and stale top-level files; relocated manual/e2e/benchmark
  scripts under `scripts/`.
- Tightened `.gitignore` for runtime database directories and caches.

[Unreleased]: https://github.com/valori-db/valori-kernel/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/valori-db/valori-kernel/releases/tag/v0.2.0
