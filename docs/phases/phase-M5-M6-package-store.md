# Phase M5–M6 — Package Store + Integrity Manager

## Goal

Complete the `valori-models` package manager with an atomic on-disk package store (M5), an integrity verification layer (M6), a garbage collector with reference counting (M6.1–M6.2), and a health-report API endpoint (M6.3).

## Delivered

### New files

| File | Phase | What it provides |
|---|---|---|
| `crates/valori-models/src/package_store.rs` | M5 | `PackageStore` — directory-layout manager for installed packages; `PackageManifest` (M5.3 versioning); `InstallLock` (M5.2 exclusive file lock) |
| `crates/valori-models/src/integrity.rs` | M6 | `IntegrityManager` — `verify(id)`, `verify_all()`; `repair()` free function; `IntegrityReport`, `IntegrityStatus`, `RepairResult`, `RepairAction` |
| `crates/valori-models/src/gc.rs` | M6.1–M6.2 | `RefCounter` — in-memory reference tracking (model → project set); `GarbageCollector` — `scan()`, `clean()`, `safe_delete()`; `GcReport`, `UnreferencedPackage` |
| `crates/valori-models/src/health.rs` | M6.3 | `SystemHealth`, `PackageHealth`, `PackageHealthStatus`; `system_health()` free function |

### Updated files

| File | Change |
|---|---|
| `crates/valori-models/src/error.rs` | Added `ModelError::InstallConflict` |
| `crates/valori-models/src/lib.rs` | Exposed all new modules; re-exported `PackageStore`, `PackageManifest`, `InstallLock`, `IntegrityManager`, `IntegrityReport`, `IntegrityStatus`, `repair_package`, `RefCounter`, `GarbageCollector`, `GcReport`, `UnreferencedPackage`, `system_health`, `SystemHealth`, `PackageHealth`, `PackageHealthStatus` |
| `crates/valori-node/src/server.rs` | Added `GET /v1/models/health` route + `models_health()` handler |
| `crates/valori-node/src/cluster_server.rs` | Added `GET /v1/models/health` route + `cluster_models_health()` handler |
| `crates/valori-node/Cargo.toml` | Added `dirs = "5"` for home directory resolution |

## Findings

### On-disk layout

```
<VALORI_MODELS_DIR>/           # default: ~/.valori/models
  .locks/                      # RAII advisory locks (create-new + drop-remove)
  .tmp/                        # cleaned on PackageStore::new(); staging for atomic installs
  embedding/
    bge-small-en-v1/
      manifest.json            # PackageManifest { schema_version=1, model: ModelManifest, ... }
  reranker/
    bge-reranker-base/
      manifest.json
```

### Atomic install (M5.1)

Download → `.tmp/<unix-timestamp>/model.bin` → SHA-256 verify → `fs::rename` to final location → write `manifest.json`. If the process crashes between download and rename, `.tmp/` is cleaned on the next `PackageStore::new()`.

### File locking (M5.2)

Uses `OpenOptions::create_new(true)` rather than `fs2`/advisory locking. If the lock file exists (another process is installing), `InstallLock::acquire` returns `ModelError::InstallConflict`. The lock file is removed on `Drop`.

### RefCounter gaps (M6.2)

`RefCounter` is in-memory only. The node does not yet populate it from collection metadata on startup. Until that wiring lands, `ref_count` is always 0 for all models and `reclaimable_bytes` in the health report represents all installed packages.

### `list()` scans disk

`PackageStore::list()` walks the task subdirectories on every call rather than maintaining an in-memory index. This is intentional — correct for small stores (< 1000 entries) and avoids stale-cache issues.

## Validation

```
cargo test -p valori-models
```

**75 tests: 75 passed, 0 failed**

New tests added:
- `package_store::tests` — 11 tests (register, duplicate, list, get, remove, find_by_task, disk_usage, commit_staged happy/sad path, lock, repair, stale tmp cleanup)
- `integrity::tests` — 8 tests (remote/all/not-found/valid/corrupted verify; repair happy/not-found paths)
- `gc::tests` — 9 tests (ref counter: basic/unknown/all-ids/projects; GC: scan/empty/clean/safe-delete fail/ok)
- `health::tests` — 4 tests (empty/remote/reclaimable/json-serialize)

`cargo build -p valori-node` — clean (warnings only from pre-existing code, zero new errors).

`GET /v1/models/health` added to both `server.rs` and `cluster_server.rs` — route parity maintained.

## Follow-ups

| Phase | What | Priority |
|---|---|---|
| M6.2 wiring | Populate `RefCounter` from collection metadata on node startup | ⭐⭐⭐ |
| M7 | Update check: compare installed `sha256` vs. registry entry | ⭐⭐⭐ |
| M8 | True byte-range resumption in `DownloadJob::run()` | ⭐⭐ |
| M9 | `import_local_model()` — verify → create manifest → register | ⭐⭐⭐ |
| M10 | `LocalProvider` — ONNX/GGUF runtime (not blocked by M5–M6) | future |
| Python SDK | `health_models()` method on `SyncRemoteClient`/`AsyncRemoteClient` | ⭐⭐ |
