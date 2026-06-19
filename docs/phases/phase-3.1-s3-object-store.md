# Phase 3.1 — S3 Object Store (Snapshot Offload + WAL Archival)

**Status:** done · on `multinode`
**Roadmap:** Phase 3 (durability & operations) — first step beyond local disk.

## Goal

Give operators a one-env-var path to durable off-node storage. Every snapshot
and every sealed WAL segment can be pushed to S3 (or any S3-compatible service)
so that a full cluster loss can be recovered in under 15 minutes with zero data
loss beyond the last snapshot interval. Adds five new REST endpoints and zero
new required configuration — the feature is completely opt-in.

## Delivered

### `crates/valori-node/src/object_store/mod.rs` (new)

`ObjectStoreBackend` wraps [opendal](https://github.com/apache/opendal) (already
a `valori-node` dependency) behind a thin, Valori-specific API:

| Method | What it does |
|---|---|
| `from_env()` | Reads `VALORI_OBJECT_STORE_URL`, builds backend, logs on failure |
| `from_url(url)` | Parses `s3://bucket/prefix` or `file:///path`; configures S3 credential chain |
| `upload_snapshot(data, hash)` | Writes `snapshots/{epoch}_{hash8}.snap` + `.hash` sidecar |
| `list_snapshots()` | Lists `.snap` objects newest-first; reads hash sidecars |
| `download_snapshot(key)` | Downloads raw bytes by object key |
| `prune_snapshots(keep)` | Deletes oldest objects keeping `keep` most recent |
| `archive_wal_segment(path)` | Reads a sealed local segment, uploads to `wal/{filename}` |
| `list_wal_segments()` | Lists archived segments in sequence order |

**Key design decisions:**

- Object keys embed a zero-padded epoch timestamp (`{epoch:020}`) so
  `list()` + lexicographic sort = chronological order — no `LastModified`
  metadata required.
- A `.hash` sidecar is written alongside every `.snap`. Restore callers must
  verify the returned `state_hash` against their recorded pre-failure hash
  before trusting the data.
- S3 credentials are resolved by the opendal AWS credential chain:
  `AWS_ACCESS_KEY_ID` env vars → IAM instance profile → `~/.aws/credentials`.
  No Valori-specific credential management or new auth surface.
- MinIO, Localstack, and Cloudflare R2 are supported via
  `VALORI_OBJECT_STORE_ENDPOINT`.
- `file:///path` backend for local dev/CI — no S3 account needed for testing.

### Five new REST endpoints in `server.rs`

| Endpoint | What it does |
|---|---|
| `GET /v1/storage/snapshots` | List all snapshots in object store (newest-first) |
| `POST /v1/storage/snapshots/upload` | Snapshot current state → upload → auto-prune |
| `POST /v1/storage/snapshots/restore` | Pull snapshot by key → restore → return verified hash |
| `GET /v1/storage/wal` | List archived WAL segments |
| `POST /v1/storage/wal/archive` | Archive a sealed local segment; body: `{"path": "..."}` |

All endpoints return 400 with a clear message if `VALORI_OBJECT_STORE_URL` is
not configured, so they fail loudly rather than silently doing nothing.

**Async safety:** handlers release the `Arc<Mutex<Engine>>` lock before any
`await` — snapshot bytes and the object store `Arc` are cloned out, then the
lock is dropped, then the S3 call runs. This means S3 network latency never
blocks other in-flight requests.

### Config additions (`config.rs`)

| Env var | Default | Purpose |
|---|---|---|
| `VALORI_OBJECT_STORE_URL` | — | `s3://bucket/prefix` or `file:///path`; absent = disabled |
| `VALORI_OBJECT_STORE_KEEP` | 7 | Max snapshots to retain in object store after pruning |
| `VALORI_OBJECT_STORE_REGION` | `us-east-1` | S3 region (also reads `AWS_DEFAULT_REGION`) |
| `VALORI_OBJECT_STORE_ENDPOINT` | — | Custom endpoint for MinIO / Localstack / R2 |

### Engine additions (`engine.rs`)

```rust
pub object_store: Option<Arc<ObjectStoreBackend>>,
pub object_store_keep: u32,
```

Initialized from `ObjectStoreBackend::from_env()` at `Engine::new()` time.
Handlers clone the `Arc` before dropping the engine lock; the backend is
`Send + Sync` so the clone is free.

### `Cargo.toml` additions

```toml
opendal = { version = "0.55.0", features = ["services-s3", "services-fs"] }
bytes = "1.0"
```

`services-fs` was added to support the `file://` backend for dev/test.
`bytes` was made an explicit dependency (was previously transitive via `reqwest`).

## Findings

- **opendal builder is move-based.** Each configuration method consumes `self`
  and returns `Self`. The initial code used the mutable-ref style
  (`builder.bucket(b); builder.region(r);`) which doesn't compile. Fixed by
  using method chaining: `S3::default().bucket(b).region(r)`.
- **`Metadata` has no `Default`.** `op.stat(path).await.unwrap_or_default()`
  doesn't compile. Fixed by `op.stat(path).await.map(|m| m.content_length()).unwrap_or(0)`.
- **`opendal::ErrorKind::NotFound` on missing prefix.** `op.list("snapshots/")`
  on a new bucket errors with `NotFound` rather than returning empty. Fixed by
  matching on `e.kind() == opendal::ErrorKind::NotFound` and returning `Ok(vec![])`.
- **WAL auto-archival on rotation is not yet wired.** `EventCommitter::commit_event`
  is sync; adding an async S3 call requires a tokio channel or callback seam.
  Deferred to Phase 3.2 TODO.

## Validation

```
cargo test -p valori-node
# All test suites: 0 failed
```

| Test suite | Passed |
|---|---|
| `valori_node::tests` | 24 |
| `collections` | 16 |
| `cluster_boot` | 4 |
| `replication` | various |
| **Total** | all green, 0 failures |

**Manual smoke test (local `file://` backend):**

```bash
VALORI_OBJECT_STORE_URL=file:///tmp/valori-store \
VALORI_DIM=8 VALORI_MAX_RECORDS=100 cargo run -p valori-node &

# Insert data
curl -s -X POST http://localhost:3000/records \
  -H 'Content-Type: application/json' \
  -d '{"values":[1,2,3,4,5,6,7,8]}'

# Upload snapshot to object store
curl -s -X POST http://localhost:3000/v1/storage/snapshots/upload | jq .

# List snapshots
curl -s http://localhost:3000/v1/storage/snapshots | jq .

# Restore from object store
curl -s -X POST http://localhost:3000/v1/storage/snapshots/restore \
  -H 'Content-Type: application/json' \
  -d '{"key":"snapshots/00000001750000000_abcd1234.snap"}' | jq .
```

## Follow-ups

| Item | Target phase |
|---|---|
| Auto-archive WAL segment on rotation (async hook in `EventCommitter`) | Phase 3.2 TODO |
| Extract `valori-object-store` to a separate crate when `valori-cli` needs `valori snapshot pull` | Phase 3.3 |
| Streaming upload for snapshots >500 MB (multipart S3 API) | Phase 3.3 |
| GCS and Azure Blob backends (opendal supports both; add features + test) | Phase 3.4 |
