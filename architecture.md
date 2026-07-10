# Valori — Architecture Guide

This document explains **why** Valori is structured the way it is. For the complete API surface, env vars, and SDK reference, see [CLAUDE.md](CLAUDE.md). For per-feature history, see [docs/phases/README.md](docs/phases/README.md).

---

## Three Bets

Every design decision in this codebase reduces to three bets:

**1. Determinism is more valuable than float convenience.**
All vector arithmetic uses Q16.16 fixed-point integers. Two machines applying identical event sequences always produce the same BLAKE3 state root — regardless of CPU, SIMD flags, or OS floating-point rounding. This is not a performance choice; it is a correctness guarantee. When an AI agent stores a memory and later asks to verify it, "the state hash matches" must mean something provable.

**2. Disk before memory.**
No mutation reaches live `KernelState` before it is fsynced to the append-only event log. Kill the process at any point and the system recovers to a consistent state. There is no "dirty in-memory state that gets flushed later."

**3. The proof must be offline-computable.**
The BLAKE3 Merkle root over all kernel state can be reproduced by any machine that has the raw event log — no server required. This is what makes Valori useful for audit trails and compliance use cases, not just fast vector search.

---

## Layer Model

```
┌─────────────────────────────────────────────────────────────────┐
│  Python SDK  /  HTTP clients  /  MCP agents                     │  Clients
├─────────────────────────────────────────────────────────────────┤
│  valori-node  (axum HTTP, standalone + cluster dual-path)       │  Transport
│  valori-consensus  (openraft, multi-shard Raft log)             │
├─────────────────────────────────────────────────────────────────┤
│  valori-effect  │  valori-planner  │  valori-metadata           │  Orchestration
├─────────────────────────────────────────────────────────────────┤
│  valori-state  │  valori-storage  │  valori-wire                │  Persistence
├─────────────────────────────────────────────────────────────────┤
│  valori-kernel  (no_std — deterministic state machine)          │  Core
│  valori-core   (no_std — shared types, IDs, errors)             │
└─────────────────────────────────────────────────────────────────┘
```

Each layer may only import from layers below it. `valori-kernel` and `valori-core` are `no_std` — they compile for embedded and WASM targets with zero OS dependencies.

---

## The Core: `valori-kernel`

The kernel is a pure state machine. Its entire API is:

```
apply(KernelEvent) → Result<(), KernelError>
search_l2_ns(query, namespace, k) → Vec<Hit>
snapshot() / restore(snapshot)
```

The kernel has no I/O, no threads, no allocator policy. It is a function from `(State, Event) → State'`.

### Q16.16 Fixed-Point

Vectors are stored as `i32` with 16 integer bits and 16 fractional bits (`FxpScalar`). This gives ~4 decimal digits of precision — enough for any embedding model in use today — while guaranteeing that `a + b` on an ARM machine produces exactly the same bits as on an x86 machine.

The tradeoff: inputs from Python/HTTP arrive as `f32` and are converted once at the boundary. Output scores are converted back to `f32` for the response. The hot path (insert, search) never sees a float.

### Record Storage: Slab + Intrusive Lists

`RecordPool` is a flat array (slab allocator) of `Record` structs. Each record stores its vector, a tag, a namespace ID, and two intrusive linked-list pointers (`next_in_ns`, `prev_in_ns`). This means iterating all records in a namespace is a linked-list walk — O(namespace size), not O(total records).

Graph nodes and edges use the same slab pattern with `first_out_edge`/`first_in_edge` heads and `next_out`/`next_in` chain pointers. Deleting a node cascades to all incident edges in O(degree) — no full-table scan.

### BLAKE3 Proofs

Every `apply()` call extends the BLAKE3 Merkle chain. The running `state_hash` is the hash of all applied events in order. After a snapshot restore, the hash is recomputed from scratch from the deserialized state — there is no "trust the saved hash" shortcut.

The `events.log` file is a BLAKE3-chained audit log: each entry includes `prev_hash`, so any modification (insert, delete, reorder) breaks the chain. `valori-verify` checks this chain offline.

---

## Persistence Layer

### Event Committer (standalone path)

Mutations follow a three-phase protocol:

```
1. Shadow execute   → validate the event against current KernelState
2. Fsync to log     → write + flush to events.log (V4 format, per-entry CRC32)
3. Live apply       → apply the event to KernelState
```

A crash between steps 2 and 3 is safe: recovery replays from the log. A crash between 1 and 2 loses the mutation (the client times out and retries with the same `request_id`, which deduplicates).

### Deduplication

Every `ClientRequest` carries a `request_id` (UUID). The kernel's dedup table (65 536 entries, LRU-evicted) prevents double-apply on retry. Dedup state travels in snapshots so every replica makes the same decision after a leader failover.

### V4 Wire Format

`events.log` uses the V4 segment format: each entry has a 4-byte CRC32 header, the serialized event payload, and the `prev_hash` for BLAKE3 chaining. V2/V3 segments decode without modification.

---

## Dual-Path Architecture

Every HTTP endpoint must work in two modes:

| Mode | Entry | State access | Write path |
|---|---|---|---|
| **Standalone** | `server.rs` | `engine.write()` | `commit_and_apply_ns` → WAL/EventLog |
| **Cluster** | `cluster_server.rs` | read local state | `raft.client_write(KernelEvent)` → Raft log → all nodes |

**Why not just one path?** Standalone mode has sub-millisecond write latency. Cluster mode has 5–50 ms per consensus round. Both are useful. The cluster path is always available; standalone is the default for development and single-machine deployments.

**How shared handlers work:** `crates/valori-node/src/routes/` holds handler bodies written once. A `*Ops` trait carries the state-touching primitives (standalone impl = engine locks; cluster impl = raft_write). The shared generic function owns validation and response shaping. `tests/route_parity.rs` enforces that both routers expose identical `/v1` routes — adding an endpoint to only one router fails the test.

---

## Multi-Shard Raft (cluster mode)

When `VALORI_SHARD_COUNT > 1`, the process runs that many independent Raft groups. Each shard owns a slice of the namespace ID space: `shard_for_namespace(ns_id) = ns_id % shard_count`. Every node runs every shard (symmetric placement). There is one shared gRPC listener.

Each shard has its own BLAKE3-chained audit log (`events-shard{N}.log`). `/v1/proof/event-log` and `/v1/timeline` currently read shard 0's log only — multi-shard proof endpoints are a known gap.

---

## RAG and Agent Memory Layer

These subsystems live in `valori-node` (std-only, never in kernel):

- **Ingest pipeline** (`ingest.rs`): chunk → embed (Ollama/OpenAI/custom) → insert. Async path (`POST /v1/ingest?async=true`) spawns a tokio task and returns a `job_id`.
- **GraphRAG** (`graph_rag.rs`): K nearest vector seeds + BFS subgraph expansion. Used by `/v1/graphrag` and `/graph/subgraph`.
- **Tree-RAG** (`tree_rag.rs`): Markdown → tree index, ToC navigation, BLAKE3 retrieval receipts.
- **Community detection** (`community.rs`): Label propagation on the graph, cosine-ranked community centroids.
- **Valori Reranker** (`valori_reranker.rs`): Hybrid vector + term-frequency re-ranking at query time.
- **Agent memory** (`server.rs`): `memory_upsert` / `memory_search` / `consolidate` / `contradict` — self-maintaining memory primitives that write edges to the BLAKE3 chain.

**Design rule:** application features (RAG, reranker, memory primitives) must never be added to `valori-kernel`. The kernel is a portability moat. If a new feature requires `std`, it belongs in `valori-node`.

---

## Effect System and Planner (advanced path)

For the `POST /v1/records` standalone path, mutations flow through:

```
Operation (user intent) → Planner → DAG of TaskSpecs → EffectBus → KernelState
```

`valori-planner` turns an `Operation` into a DAG of `TaskSpec`s using a two-layer cache (in-process + `MetadataDb`). `valori-effect` routes kernel writes, receipt fragments, audit entries, and metrics from task execution to their destinations. This is the foundation for provable, cacheable, replayable computation — not yet used on all endpoints.

---

## Crate Stability

| Crate | Stability | Notes |
|---|---|---|
| `valori-core` | Stable | Public API frozen; breaking changes require RFC |
| `valori-kernel` | Stable | Event schema and snapshot format are versioned |
| `valori-wire` | Stable | V4 format; older segments decode without modification |
| `valori-storage` | Stable | WAL format tied to wire version |
| `valori-state` | Stable | |
| `valori-metadata` | Beta | redb schema subject to migration |
| `valori-node` | Beta | HTTP surface stable; internal Engine API evolving |
| `valori-consensus` | Beta | openraft 0.9; log format may change on major version |
| `valori-effect` | Alpha | Effect types stabilizing |
| `valori-planner` | Alpha | Planner cache format not yet versioned |
| `valori-ffi` | Beta | Python ABI tied to pyo3 version |
| `valori-mcp` | Beta | |
| `valori-cli` | Beta | |
| `valori-verify` | Stable | |

---

## Invariants

These must never be broken. Each has a test that enforces it:

1. **Apply before audit** — `DEDUP CHECK → KERNEL APPLY → AUDIT WRITE`. Never write an audit entry for a rejected or duplicate event.
2. **Namespace isolation at three points** — event-commit path, WAL replay, and `build_index()` after restore. A fourth path needs a guard.
3. **Q16.16 only in vector ops** — `insert_record` and `search` paths must never receive raw `f32` through the kernel.
4. **`valori-kernel` stays `no_std`** — verified by `cargo build -p valori-kernel --target wasm32-unknown-unknown`.
5. **Watcher tasks aborted before redb re-open** — `ClusterHandle` holds the watcher `JoinHandle`; abort and await it before any shutdown or test restart.
6. **Request-ID dedup on every cluster command** — `ClientRequest` carries `request_id: Uuid`; state machine checks it before applying.
7. **Route parity** — `tests/route_parity.rs` fails if standalone and cluster routers diverge on `/v1` routes.
8. **No cross-crate source duplication** — `tests/architecture.rs` fails if a `.rs` file exists in both `valori-node/src` and any extracted crate.

---

## Further Reading

- [PERFORMANCE.md](PERFORMANCE.md) — index types, memory model, capacity planning
- [SECURITY.md](SECURITY.md) — crypto primitives, threat model, vulnerability reporting
- [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) — detailed in-scope / out-of-scope threat analysis
- [docs/CAPACITY.md](docs/CAPACITY.md) — vectors/GB by dimension, WAL growth, S3 cost
- [docs/DR.md](docs/DR.md) — snapshot backup and cross-region recovery runbook
- [docs/CLUSTER.md](docs/CLUSTER.md) — cluster operations and setup wizard
- [docs/phases/README.md](docs/phases/README.md) — full phase history
- [rfcs/](rfcs/) — design RFCs for planner, effect system, and crate boundaries
- [INVARIANTS.md](INVARIANTS.md) — machine-checkable invariant list
