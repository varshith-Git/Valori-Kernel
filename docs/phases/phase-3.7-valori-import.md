# Phase 3.7 — `valori import` — Provable Migrations

## Goal

Give users a first-class migration path into Valori from existing vector stores
(Qdrant) and from any tool that can emit JSONL. Every imported record is a
normal `KernelEvent`, making provenance auditable from migration day zero via
the BLAKE3 audit chain.

## Delivered

### `crates/valori-cli/src/commands/import.rs` (new, ~380 lines)

Two public entry points, both zero-new-dependency:

| Entry point | Source | Notes |
|---|---|---|
| `run_qdrant(QdrantImportArgs)` | Qdrant scroll API | Cursor-based, resumable, dim-validated |
| `run_jsonl(JsonlImportArgs)` | JSONL file | Streaming, alias-aware fields, skip-bad-lines |

**Shared helpers:**
- `random_key() -> [u8; 16]` — reads `/dev/urandom` for per-record idempotency keys; counter-based fallback on non-Unix.
- `ValoriClient` — thin `ureq` wrapper with `get_dim()`, `ensure_collection()`, `insert_one()`, `batch_insert()`.
- `ImportState` / sidecar JSON — tracks `last_offset`, `imported` count, source metadata; written after each page; deleted on clean completion.
- `make_progress()` — `indicatif` progress bar with spinner, elapsed time, and records/sec.

### `crates/valori-cli/src/commands/mod.rs`
- Added `pub mod import;`

### `crates/valori-cli/src/main.rs`
- Added `ImportSource` enum (`Qdrant`, `Jsonl`) with full `clap` argument specs.
- Added `Commands::Import { source }` variant.
- Added dispatch arms in `main()`.

## Findings

### Dim mismatch is the most common migration failure
Validating dim before the first insert (rather than failing mid-stream) saves
users from partial imports that require cleanup. The error message includes the
exact `VALORI_DIM=N` env var to set before retrying.

### Qdrant named vectors
Qdrant supports both a single unnamed vector (`"vector": [...]`) and named
vectors (`"vector": {"my-vec": [...]}`). The importer handles both: for named
vectors it picks the first named entry. A future `--vector-name` flag would let
users select a specific named vector explicitly (deferred).

### Per-record vs. batch idempotency
The Valori `/records` endpoint accepts `request_id` per-record for Raft dedup.
The batch endpoint (`/v1/vectors/batch_insert`) does not currently thread
`request_id` through per item, so the importer uses single inserts with
idempotency keys. This is slower (N round-trips vs. 1 per batch) but safe.
A follow-up to add per-item `request_id` in the batch endpoint would 10-100×
the import throughput for large datasets.

### JSONL progress bar uses byte position
Record count is unknown before parsing the file. The progress bar uses the file
size as the total and advances by bytes read, so the % complete is approximate
but useful.

## Validation

```
cargo build -p valori-cli    # clean, warnings pre-existing only
cargo test -p valori-kernel -p valori-node
# 215 passed, 0 failed, 1 ignored
```

Manual smoke test (requires a running Valori node):
```bash
# JSONL roundtrip
echo '{"vector":[0.1,0.2,0.3,0.4],"metadata":"hello"}' > /tmp/test.jsonl
cargo run -p valori-cli --bin valori -- import jsonl /tmp/test.jsonl \
  --target-url http://localhost:3000

# Help text renders
cargo run -p valori-cli --bin valori -- import --help
cargo run -p valori-cli --bin valori -- import qdrant --help
cargo run -p valori-cli --bin valori -- import jsonl --help
```

## Follow-ups

| Item | Suggested phase |
|---|---|
| `--vector-name` flag for Qdrant named-vector collections | 3.7b |
| Parquet import (`arrow` + `parquet` crates) | 3.7b |
| Per-item `request_id` in `/v1/vectors/batch_insert` for 10–100× throughput | 3.7b or kernel |
| `GenesisImport` kernel event for on-chain provenance of the import job itself | future |
| `valori import valori` (cross-node snapshot migration) | future |
