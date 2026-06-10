# Valori Node — Deployment Guide

**Applies to:** `valori-node` v0.1.0 / `valori-kernel` v0.1.11+  
**Last updated:** 2026-06-09

---

## Table of Contents

1. [Quick start](#1-quick-start)
2. [How the node starts](#2-how-the-node-starts)
3. [Configuration reference](#3-configuration-reference)
   - [Sizing](#31-sizing)
   - [Persistence](#32-persistence)
   - [Index and quantization](#33-index-and-quantization)
   - [Networking and security](#34-networking-and-security)
   - [Replication](#35-replication)
   - [Observability](#36-observability)
4. [Persistence modes — choosing the right one](#4-persistence-modes--choosing-the-right-one)
5. [Index types — choosing the right one](#5-index-types--choosing-the-right-one)
6. [Replication setup](#6-replication-setup)
7. [Upgrade paths](#7-upgrade-paths)
   - [WAL → Event Log (v0.0.x → v0.1.x)](#71-wal--event-log-v00x--v01x)
   - [Generic Engine → heap Engine (library users)](#72-generic-engine--heap-engine-library-users)
   - [Python SDK endpoint fixes (v0.1.9 → v0.1.10+)](#73-python-sdk-endpoint-fixes-v019--v0110)
8. [Production checklist](#8-production-checklist)
9. [Docker reference](#9-docker-reference)

---

## 1. Quick start

```bash
# Minimal — ephemeral (no persistence)
VALORI_DIM=384 cargo run -p valori-node --release

# Recommended production minimum
VALORI_DIM=384 \
VALORI_MAX_RECORDS=100000 \
VALORI_EVENT_LOG_PATH=/data/events.log \
VALORI_SNAPSHOT_PATH=/data/snapshot.bin \
VALORI_SNAPSHOT_INTERVAL=300 \
VALORI_AUTH_TOKEN=$(openssl rand -hex 32) \
VALORI_BIND=0.0.0.0:3000 \
cargo run -p valori-node --release
```

The server listens on `VALORI_BIND`, serves HTTP/1.1, and is ready once you see:

```
INFO valori_node: Listening on 0.0.0.0:3000
```

---

## 2. How the node starts

On every startup `main()` runs the following sequence:

```
1. Read config from environment variables
2. Engine::new(&cfg)       — allocate in-memory state
3. engine.try_recover()    — restore durable state (never panics)
   ├─ Priority 1: Event log  (replay all events from events.log)
   ├─ Priority 2: Snapshot   (load snapshot.bin if event log absent/empty)
   └─ Priority 3: Fresh start (no prior state found — empty store)
4. Spawn auto-snapshot task (if VALORI_SNAPSHOT_INTERVAL is set)
5. Spawn follower loop     (if VALORI_FOLLOWER_OF is set)
6. axum::serve             — accept HTTP requests
```

`try_recover()` is crash-safe: a truncated event log recovers all fully-written
events and discards the partial tail; a corrupt snapshot falls through to a
fresh start, logging an error but never killing the process.

---

## 3. Configuration reference

All configuration is read from **environment variables** at startup. There is no
config file; use a `.env` file or your container's env section.

### 3.1 Sizing

| Variable | Type | Default | Description |
|---|---|---|---|
| `VALORI_DIM` | `usize` | `16` | **Vector dimension.** Every record in the store must have exactly this many components. Set this to match your embedding model (e.g. `384` for `all-MiniLM-L6-v2`, `1536` for `text-embedding-ada-002`, `3072` for `text-embedding-3-large`). Changing this after data has been written requires a full data migration — the event log header encodes the dimension and will reject mismatched events. |
| `VALORI_MAX_RECORDS` | `usize` | `1024` | **Hard record limit.** Once the live record count reaches this value, any insert (`POST /records`, `POST /v1/memory/upsert_vector`, `POST /v1/memory/insert_batch`) is rejected with **HTTP 507 Insufficient Storage**. This is not a pre-allocation — memory is allocated lazily — but the count is enforced strictly at write time. Soft-deleted records still occupy a slot; reuse of deleted slots is not yet implemented. Set with 10–20 % headroom above your expected peak. |
| `VALORI_MAX_NODES` | `usize` | `1024` | **Hard graph-node limit.** Graph node creation (`POST /graph/node`) returns HTTP 507 when this limit is reached. Set to `0` if you do not use the graph API; this prevents all node creation (any attempt returns 507 immediately). |
| `VALORI_MAX_EDGES` | `usize` | `2048` | **Hard graph-edge limit.** Graph edge creation (`POST /graph/edge`) returns HTTP 507 when this limit is reached. Rule of thumb: `MAX_EDGES` ≈ `MAX_NODES × 4` for lightly connected graphs; higher for dense knowledge graphs. |

**Capacity planning example** for 100 k vectors at 384-dim:

```
VALORI_DIM=384
VALORI_MAX_RECORDS=110000   # 10 % headroom
VALORI_MAX_NODES=0          # graph unused
VALORI_MAX_EDGES=0
```

Memory footprint (approximate):
- Record pool: `MAX_RECORDS × (DIM × 4 + 16)` bytes
  → 100 k × (384 × 4 + 16) ≈ **153 MB**
- Graph pools: negligible when `MAX_NODES=0`
- Index: depends on type (see §5)

### 3.2 Persistence

| Variable | Type | Default | Description |
|---|---|---|---|
| `VALORI_EVENT_LOG_PATH` | `path` | _(unset)_ | **Recommended persistence path.** Path to the binary event log file (e.g. `/data/events.log`). When set, every mutation is appended here as an immutable, sequenced entry. This is the canonical source of truth. On startup the node replays this file to reconstruct state exactly. A companion sidecar `events.metadata.json` is written alongside it to persist `set_metadata` calls. If both `VALORI_EVENT_LOG_PATH` and `VALORI_WAL_PATH` are set, the WAL is silently ignored — the event log supersedes it entirely. |
| `VALORI_SNAPSHOT_PATH` | `path` | _(unset)_ | Path where snapshots are written and read from. Used as a fast-path recovery cache (loaded if the event log is absent or empty) and by the `POST /v1/snapshot/save` endpoint. The snapshot format is `VAL1` (see `docs/SNAPSHOT_FORMAT.md`). Safe to delete — the event log is always the canonical state. |
| `VALORI_SNAPSHOT_INTERVAL` | `u64` | _(unset)_ | Auto-snapshot interval in **seconds**. Requires `VALORI_SNAPSHOT_PATH`. A background task wakes at this cadence and writes a fresh snapshot. Useful for bounding recovery time: a snapshot at interval T means the worst-case replay on the next boot covers at most T seconds of events. Set to `300` (5 min) for most deployments. |
| `VALORI_WAL_PATH` | `path` | _(unset)_ | **Legacy persistence path.** Write-ahead log used before the event log was introduced. Still works for backward compatibility but offers fewer guarantees than the event log (no journal, no replay metadata). Do not set alongside `VALORI_EVENT_LOG_PATH`. Prefer the event log for all new deployments. See [§7.1](#71-wal--event-log-v00x--v01x) for migration. |

**Persistence decision tree:**

```
New deployment?
  └─ Yes → Set VALORI_EVENT_LOG_PATH + VALORI_SNAPSHOT_PATH
               └─ Also set VALORI_SNAPSHOT_INTERVAL=300

Existing deployment using WAL?
  └─ See §7.1 (upgrade path)

Need durable writes at all?
  └─ No (dev / ephemeral) → leave all three unset
```

### 3.3 Index and quantization

| Variable | Accepted values | Default | Description |
|---|---|---|---|
| `VALORI_INDEX` | `brute`, `hnsw`, `ivf` | `brute` | Vector search index type. `brute` is exact nearest-neighbour with O(n) scan — correct but slow above ~50 k vectors. `hnsw` is approximate nearest-neighbour with sub-linear query time, good for interactive workloads. `ivf` clusters vectors into k-means partitions; queries probe a subset of partitions for sub-linear recall. See [§5](#5-index-types--choosing-the-right-one) for trade-offs. |
| `VALORI_QUANT` | `none`, `scalar`, `product` | `none` | Vector quantization applied before indexing. `none` stores full Q16.16 fixed-point vectors (4 bytes / dimension). `scalar` reduces to 1 byte / dimension (~4× compression, small accuracy loss). `product` applies product quantization for higher compression; requires a training pass similar to IVF. Not yet exposed via the HTTP API — only applicable when using the Rust API directly. |

#### `/health` response shape

`GET /health` returns JSON and is **always unauthenticated** — no bearer token required, even when `VALORI_AUTH_TOKEN` is configured.

HTTP status codes follow capacity:

| Status | `status` field | Meaning |
|---|---|---|
| **200** | `"ok"` | All pools below 90 % — route freely |
| **200** | `"degraded"` | At least one pool ≥ 90 % — still operational, plan capacity increase |
| **503** | `"full"` | At least one pool at 100 % — inserts return HTTP 507 |

Example response:

```json
{
  "status": "ok",
  "version": "0.1.0",
  "dim": 384,
  "index": "BruteForce",
  "persistence": "event_log",
  "records": { "live": 5234, "slots_used": 5240, "capacity": 100000, "fill_pct": 5.2 },
  "nodes":   { "live": 1200, "slots_used": 1200, "capacity": 10000,  "fill_pct": 12.0 },
  "edges":   { "live": 3600, "slots_used": 3600, "capacity": 20000,  "fill_pct": 18.0 },
  "event_log_height": 5234
}
```

`persistence` is one of `"event_log"`, `"wal"`, `"snapshot"`, or `"none"`.
`event_log_height` is omitted when the event log is not configured.

### 3.4 Networking and security

| Variable | Type | Default | Description |
|---|---|---|---|
| `VALORI_BIND` | `host:port` | `127.0.0.1:3000` | TCP address and port the HTTP server listens on. Use `0.0.0.0:3000` to accept connections from all interfaces (required in containers). The node speaks plain HTTP/1.1; TLS termination should be handled by a reverse proxy (nginx, Caddy, cloud load balancer). |
| `VALORI_AUTH_TOKEN` | `string` | _(unset)_ | Bearer token required on every request. When unset the server logs `Auth Disabled` and accepts all requests — suitable only for local development. In production always set this. Generate with `openssl rand -hex 32`. Rotate by restarting with a new token. Clients must send `Authorization: Bearer <token>`. |

### 3.5 Replication

| Variable | Type | Default | Description |
|---|---|---|---|
| `VALORI_FOLLOWER_OF` | `URL` | _(unset)_ | When set, the node starts in **follower mode** and treats the given URL as the leader. On boot the follower calls `GET /v1/replication/state` to check the leader, bootstraps from `GET /v1/snapshot/download` if its own journal is empty, then streams `GET /v1/replication/events` (SSE) to apply events in real time. The leader URL must include scheme and port (e.g. `http://leader:3000`). If unset, the node starts as leader. |

See [§6](#6-replication-setup) for the full leader / follower setup.

### 3.6 Observability

| Variable | Type | Default | Description |
|---|---|---|---|
| `RUST_LOG` | log filter | `valori_node=debug,tower_http=debug` | Controls log verbosity. Follows the `tracing-subscriber` filter syntax. Set to `valori_node=info` in production to reduce noise. Use `valori_node=trace` when debugging event replay or replication. |

**Prometheus metrics** are available at `GET /metrics` (no auth required, even when `VALORI_AUTH_TOKEN` is set). Gauges are refreshed from live `KernelState` on every `/health` and `/metrics` scrape.

**KernelState gauges** (always current):

| Metric | Description |
|---|---|
| `valori_records_live` | Live (non-deleted) record count |
| `valori_records_capacity` | `VALORI_MAX_RECORDS` |
| `valori_record_fill_ratio` | `records_live / records_capacity` — alert when > 0.9 |
| `valori_nodes_live` | Live graph node count |
| `valori_nodes_capacity` | `VALORI_MAX_NODES` |
| `valori_node_fill_ratio` | `nodes_live / nodes_capacity` |
| `valori_edges_live` | Live graph edge count |
| `valori_edges_capacity` | `VALORI_MAX_EDGES` |
| `valori_edge_fill_ratio` | `edges_live / edges_capacity` |
| `valori_dim` | Configured vector dimension |
| `valori_event_log_height` | Committed event count (only when event log is enabled) |
| `valori_node_up` | Always `1.0` while the process is running |

**Event / WAL metrics** (updated per operation):

| Metric | Description |
|---|---|
| `valori_events_committed_total` | Monotonically increasing count of committed events |
| `valori_event_commit_duration_seconds` | Histogram of per-event commit latency |
| `valori_snapshot_size_bytes` | Size of the last written snapshot in bytes |
| `valori_proofs_generated_total` | Count of `GET /v1/proof/state` calls |
| `valori_replay_duration_seconds` | Time spent on event-log or WAL replay at startup |

**Recommended Prometheus alert:**
```yaml
- alert: ValoriRecordPoolNearFull
  expr: valori_record_fill_ratio > 0.9
  for: 5m
  labels:
    severity: warning
  annotations:
    summary: "Valori record pool above 90 %"
    description: "Increase VALORI_MAX_RECORDS or the node will reject inserts when full (503)."
```

---

## 4. Persistence modes — choosing the right one

### Mode A: No persistence (ephemeral)

Leave `VALORI_EVENT_LOG_PATH`, `VALORI_WAL_PATH`, and `VALORI_SNAPSHOT_PATH` all
unset.  State lives entirely in memory.  A restart means a fresh start.

Use when: local development, CI test fixtures, read-only replicas that
re-bootstrap on restart.

### Mode B: Snapshot only

Set only `VALORI_SNAPSHOT_PATH` (and optionally `VALORI_SNAPSHOT_INTERVAL`).

Writes are buffered in memory; the snapshot is written periodically or
on-demand via `POST /v1/snapshot/save`.  Recovery loads the last snapshot.
**Any writes between the last snapshot and the crash are lost.**

Use when: you can tolerate some data loss, you need the simplest possible setup,
or you are running a follower that re-bootstraps from the leader anyway.

### Mode C: Event log (recommended)

Set `VALORI_EVENT_LOG_PATH`.  Optionally also set `VALORI_SNAPSHOT_PATH` and
`VALORI_SNAPSHOT_INTERVAL` to bound recovery time.

Every mutation is appended to the event log synchronously before the HTTP
response is returned.  Recovery replays the full log or, if a snapshot is
available, loads the snapshot and replays only events since the snapshot was
written.  **Zero data loss** for any completed write.

The event log is append-only and never rewritten.  It can grow without bound —
trim it by saving a snapshot, then truncating or deleting the old log file
before the next restart (the node will recover from the snapshot).

### Mode D: Legacy WAL

Set only `VALORI_WAL_PATH`.  This mode is preserved for backward compatibility
with pre-v0.1 deployments.  It offers weaker guarantees than the event log
(the WAL is replayed from a base snapshot, not from the beginning of history).
Migrate to Mode C when convenient; see [§7.1](#71-wal--event-log-v00x--v01x).

### Recovery priority

When multiple persistence files exist on disk, `try_recover()` applies this
priority order regardless of which env vars are set:

```
1. Event log (if file exists and has ≥ 1 event)
2. Snapshot  (if file exists and event log is absent/empty)
3. Fresh start
```

---

## 5. Index types — choosing the right one

### BruteForce (default)

Exact L2 nearest-neighbour.  On every query it scans all live records and
returns the true k nearest.

- **Recall:** 100 % (exact)
- **Query time:** O(n × dim)
- **Build time:** O(1) — inserts are immediate
- **Memory:** Just the record vectors (no extra structure)
- **When to use:** Up to ~50 k vectors, or whenever exact results are required

### HNSW

Hierarchical Navigable Small World graph.  Approximate nearest-neighbour with
sub-linear average query time.

- **Recall:** ~95–99 % at typical settings
- **Query time:** O(log n) average
- **Build time:** O(n log n); each insert builds graph connections
- **Memory:** Record vectors + adjacency lists (~2–4× the raw vector data)
- **Config:** `ef_construction=100`, `M=16`, `M_MAX=32` (constants in source;
  not yet env-configurable)
- **When to use:** Interactive search above ~50 k vectors

### IVF

Inverted File index.  Clusters vectors into k-means partitions at build time;
queries probe a subset of partitions.

- **Recall:** 50–95 % depending on `n_probe`/`n_list` ratio
- **Query time:** O(n_probe × cluster_size × dim)
- **Build time:** Requires an explicit `build_index()` call after bulk load.
  Incremental inserts post-build go into the closest existing centroid — no
  retraining
- **Memory:** Record vectors + centroid table (~negligible)
- **Config:** `n_list=100` (clusters), `n_probe=5` (probed at query time)
  — not yet env-configurable; edit `IvfConfig::default()` in source
- **Sizing rule:** `n_list ≈ sqrt(N)` for N total vectors
- **When to use:** Very large datasets (≥ 500 k vectors) where HNSW memory is
  prohibitive, or batch/offline workloads

**Note:** IVF must be explicitly built before searches work.  The HTTP API
currently always uses whatever index type is configured at startup.  If you
switch from BruteForce to IVF in a running deployment, call the
`POST /v1/snapshot/restore` or restart to trigger `rebuild_index()`, which
runs the IVF batch build automatically.

---

## 6. Replication setup

Valori uses a single-leader, multi-follower replication model.  The leader
owns all writes.  Followers are read-only replicas that can serve search and
proof queries.

### Leader

Run normally (no `VALORI_FOLLOWER_OF`).  The leader must have:

- `VALORI_EVENT_LOG_PATH` set — followers stream from this file
- `VALORI_SNAPSHOT_PATH` set — followers bootstrap from the snapshot endpoint

```bash
# Leader
VALORI_DIM=384
VALORI_EVENT_LOG_PATH=/data/events.log
VALORI_SNAPSHOT_PATH=/data/snapshot.bin
VALORI_SNAPSHOT_INTERVAL=60
VALORI_AUTH_TOKEN=<shared-secret>
VALORI_BIND=0.0.0.0:3000
```

### Follower

```bash
# Follower
VALORI_DIM=384                              # must match leader
VALORI_EVENT_LOG_PATH=/data/follower-events.log
VALORI_FOLLOWER_OF=http://leader-host:3000
VALORI_AUTH_TOKEN=<same-shared-secret>
VALORI_BIND=0.0.0.0:3001
```

The follower startup sequence:

1. Calls `GET /v1/replication/state` on the leader to confirm reachability.
2. If its own journal is empty, calls `GET /v1/snapshot/download` and restores.
3. Opens `GET /v1/replication/events` (SSE stream) and replays each event into
   its own engine, advancing `committed_height`.
4. A background task polls `GET /v1/proof/state` every 5 s and logs `Synced`
   or `Diverged` accordingly.  `GET /v1/replication/state` reflects this status.

**Follower divergence** is detected automatically.  If the follower's
`final_state_hash` differs from the leader's, the replication status becomes
`Diverged`.  This is logged and visible at `GET /v1/replication/state`.
Recovery: stop the follower, delete its event log, restart — it will
re-bootstrap from the leader snapshot.

**Network failures** are handled by the outer `run_follower_loop`: the SSE
connection is re-established after any error.  `get_proof` and
`download_snapshot` retry with exponential backoff (0 ms, 500 ms, 1 s, 2 s,
capped at 8 s) before returning an error.

---

## 7. Upgrade paths

### 7.1 WAL → Event Log (v0.0.x → v0.1.x)

Before v0.1, persistence used a Write-Ahead Log (`VALORI_WAL_PATH`).  The
event log is a strict superset: richer format, journal-backed, sidecar metadata
persistence, and first-class replication support.

**Migration steps (zero-downtime):**

1. While the old node is still running, call `POST /v1/snapshot/save` to
   create (or refresh) a snapshot at `VALORI_SNAPSHOT_PATH`.
2. Stop the node.
3. Update the environment:
   - Remove `VALORI_WAL_PATH`
   - Add `VALORI_EVENT_LOG_PATH=/data/events.log`
   - Keep `VALORI_SNAPSHOT_PATH` unchanged
4. Start the new node.  `try_recover()` will find no event log, load the
   snapshot (Priority 2), and begin writing to the event log from that point on.

The old WAL file can be deleted after you confirm the new node is healthy
(check `GET /v1/proof/state` before and after migration — the hash must match).

**Rollback:** stop the new node, restore `VALORI_WAL_PATH`, remove
`VALORI_EVENT_LOG_PATH`, restart with the old binary.  The snapshot is
compatible with both old and new nodes.

### 7.2 Generic Engine → heap Engine (library users)

`valori-kernel` ≤ v0.1.10 exposed a generic `Engine<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>`
struct.  v0.1.11+ removes all generics; the engine is heap-allocated and
sized at runtime from `NodeConfig`.

**Before:**
```rust
let engine = Engine::<1024, 384, 1024, 2048>::new(&config);
```

**After:**
```rust
// All capacity comes from NodeConfig fields
let mut config = NodeConfig::default();
config.max_records = 1024;
config.dim = 384;
config.max_nodes = 1024;
config.max_edges = 2048;
let engine = Engine::new(&config);
```

**Recovery API change:**

The old `engine.restore_with_wal_replay(snap_bytes, wal_path)` is removed.
Use the unified `engine.try_recover()` instead — it handles event log, snapshot,
and fresh-start in one call and never panics:

```rust
// Before
let n = engine.restore_with_wal_replay(&snap_bytes, &wal_path).unwrap();

// After
let mode = engine.try_recover();
match mode {
    RecoveryMode::EventLog(n) => println!("Replayed {} events", n),
    RecoveryMode::Snapshot    => println!("Loaded from snapshot"),
    RecoveryMode::Fresh       => println!("Started fresh"),
}
```

**`EventLogWriter` signature change:**

```rust
// Before (one-arg)
let writer = EventLogWriter::<16>::open(&path).unwrap();

// After (two-arg, non-generic)
let writer = EventLogWriter::open(&path, Some(dim as u32)).unwrap();
```

**`ValoriKernel` deprecation:**

The root-crate `ValoriKernel` struct (the original HNSW prototype) is
`#[deprecated(since = "0.3.0")]`.  It continues to compile but will be removed
in a future release.  The production path is `valori_node::engine::Engine`.

### 7.3 Python SDK endpoint fixes (v0.1.9 → v0.1.10+)

The `SyncRemoteClient` and `AsyncRemoteClient` in `python/valoricore/remote.py`
had incorrect endpoint URLs and HTTP methods:

| Operation | Old (broken) | New (correct) |
|---|---|---|
| Download snapshot | `POST /snapshot` | `GET /v1/snapshot/download` |
| Upload/restore snapshot | `POST /restore` | `POST /v1/snapshot/upload` |

If you pinned to an older release and are calling snapshot endpoints directly,
update your URLs.  No data migration is required — the server endpoints have
not changed, only the client URLs.

---

## 8. Production checklist

Before going live, verify each item:

- [ ] **`VALORI_DIM`** matches your embedding model output exactly
- [ ] **`VALORI_MAX_RECORDS`** ≥ expected peak record count
- [ ] **`VALORI_EVENT_LOG_PATH`** set to a durable, backed-up volume
- [ ] **`VALORI_SNAPSHOT_PATH`** set; `VALORI_SNAPSHOT_INTERVAL=300` (or lower)
- [ ] **`VALORI_AUTH_TOKEN`** set to a 32-byte random hex string
- [ ] **`VALORI_BIND=0.0.0.0:3000`** (not `127.0.0.1`) inside containers
- [ ] **TLS** terminated by a reverse proxy; node itself speaks plain HTTP
- [ ] **`RUST_LOG=valori_node=info`** to reduce log volume in production
- [ ] **Liveness probe:** `GET /health` returns `200` with `"status": "ok"`
- [ ] **Readiness:** confirm `GET /v1/proof/state` returns a valid hash after startup
- [ ] **Capacity alert:** Prometheus alert on `valori_record_fill_ratio > 0.9`
- [ ] **Metrics scrape:** `GET /metrics` reachable from your Prometheus instance (no auth required)
- [ ] **Backup:** `VALORI_SNAPSHOT_PATH` on a volume that is snapshotted or replicated
- [ ] **Event log rotation plan:** decide maximum event log size and when you
  will trim it (save snapshot → delete old log → restart)

---

## 9. Docker reference

Minimal `Dockerfile`:

```dockerfile
FROM rust:1.77 AS builder
WORKDIR /app
COPY . .
RUN cargo build -p valori-node --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/valori-node /usr/local/bin/valori-node
RUN mkdir -p /data
VOLUME ["/data"]
EXPOSE 3000
CMD ["valori-node"]
```

`docker-compose.yml` for a leader + one follower:

```yaml
version: "3.9"
services:
  leader:
    build: .
    environment:
      VALORI_DIM: "384"
      VALORI_MAX_RECORDS: "100000"
      VALORI_EVENT_LOG_PATH: /data/events.log
      VALORI_SNAPSHOT_PATH: /data/snapshot.bin
      VALORI_SNAPSHOT_INTERVAL: "300"
      VALORI_AUTH_TOKEN: "changeme"
      VALORI_BIND: "0.0.0.0:3000"
      RUST_LOG: "valori_node=info"
    volumes:
      - leader-data:/data
    ports:
      - "3000:3000"
    healthcheck:
      test: ["CMD", "curl", "-sf", "http://localhost:3000/health"]
      interval: 10s
      timeout: 5s
      retries: 3

  follower:
    build: .
    environment:
      VALORI_DIM: "384"
      VALORI_MAX_RECORDS: "100000"
      VALORI_EVENT_LOG_PATH: /data/events.log
      VALORI_FOLLOWER_OF: "http://leader:3000"
      VALORI_AUTH_TOKEN: "changeme"
      VALORI_BIND: "0.0.0.0:3000"
      RUST_LOG: "valori_node=info"
    volumes:
      - follower-data:/data
    depends_on:
      leader:
        condition: service_healthy

volumes:
  leader-data:
  follower-data:
```

Follower convergence can be verified at any time:

```bash
# Leader hash
curl -H "Authorization: Bearer changeme" http://localhost:3000/v1/proof/state

# Follower hash (should match within seconds)
curl -H "Authorization: Bearer changeme" http://localhost:3001/v1/proof/state
```

---

## Related documents

| Document | Contents |
|---|---|
| [`docs/SNAPSHOT_FORMAT.md`](SNAPSHOT_FORMAT.md) | VAL1 binary snapshot wire format |
| [`docs/crash-recovery-proof.md`](crash-recovery-proof.md) | Production crash recovery proof (2026-01-12) |
| [`docs/wal-replay-guarantees.md`](wal-replay-guarantees.md) | Formal durability guarantees |
| [`docs/verifiable-replication.md`](verifiable-replication.md) | Proof system and divergence detection |
| [`docs/authentication.md`](authentication.md) | Auth token setup and rotation |
| [`docs/api-reference.md`](api-reference.md) | Full HTTP API reference |
