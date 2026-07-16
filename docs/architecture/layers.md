# Valori — Normative Architecture Document

This is the normative architecture document for the Valori codebase. It defines
ownership, invariants, and allowed dependency directions. When adding a new
capability, resolve its layer here first. If something spans layers, put the
primitive in the lower crate and the orchestration in the higher one.

Changes to this document require explicit architectural reasoning. It is not a
description of what exists — it is a contract for what is permitted.

This page covers compile-time crate dependencies. For the runtime control
plane — which process owns project lifecycle vs. data operations vs. the
frontend (daemon / node / `ui`) — see
[`control-plane.md`](control-plane.md).

Referenced by: `CONTRIBUTING.md`, `CLAUDE.md`.

---

## Dependency graph

Compile-time dependencies and runtime composition are separated because they
differ in several meaningful places (noted below).

---

### Compile-time crate dependencies

These are the `[dependencies]` edges from each `Cargo.toml`. Dev-dependencies
are excluded — they do not affect what ships.

```
valori-core  ──────────────────────────────────────────────── (no valori deps)
  │
  ├─▶ valori-kernel ──────────────────────────────────────── (no_std)
  │       │
  │       ├─▶ valori-wire ─────────────────────────────────── (V2/V3/V4 wire format)
  │       │       │
  │       │       ├─▶ valori-storage ─────────────────────── (WAL + event log + object store)
  │       │       │       └─▶ valori-state ────────────────── (recovery orchestration)
  │       │       │
  │       │       └─▶ valori-metadata ──────────────────────  (redb control-plane store)
  │       │               └─▶ valori-planner ──────────────── (operation→task DAG)
  │       │                       └─▶ valori-effect ─────────  (EffectBus + 7 capability traits)
  │       │
  │       ├─▶ valori-verify ──────────────────────────────── (chain replay lib + binaries)
  │       │
  │       └─▶ valori-consensus ───────────────────────────── (openraft — one Raft per ShardId)
  │
  └─▶ (transitive via above crates)
```

`valori-node` is the convergence point. Its direct `[dependencies]`:

```
valori-node
  depends on: valori-core, valori-kernel, valori-wire,
              valori-storage, valori-state, valori-consensus,
              valori-metadata, valori-planner, valori-effect
```

Crates that depend on `valori-node`:

```
valori-ffi   depends on: valori-node, valori-kernel, valori-verify
valori-cli   depends on: valori-node, valori-kernel, valori-wire, valori-consensus
valori-mcp   depends on: (no valori compile-time deps — see runtime note below)
```

**Notable divergences from the conceptual layering**:

- `valori-mcp` has **no compile-time valori dependency**. Its `Cargo.toml`
  `[dependencies]` contains only `reqwest`, `serde_json`, `blake3`, etc.
  `valori-node` appears only in `[dev-dependencies]` for integration tests.
  At compile time, `valori-mcp` is an independent HTTP client crate.

- `valori-state` has **no compile-time dependency on `valori-verify`**.
  `valori-verify` appears only in `valori-state`'s `[dev-dependencies]`
  (used in the event-log end-to-end tests). At compile time `valori-state`
  knows nothing about the verifier.

- `valori-verify` depends only on `valori-kernel` and `valori-wire` — not
  on `valori-node` or `valori-storage`. It reads raw bytes and does
  entry-by-entry BLAKE3 replay entirely through the wire-level codec.

**Layering rule**: arrows point downward only. No crate may introduce an
upward `[dependencies]` edge. Adding one is an architecture violation —
move the shared concept into a lower crate instead.

---

### Runtime composition

How the system is actually wired when binaries run. This differs from the
compile-time graph in three ways: mode selection, process boundaries, and
dynamic capability injection.

```
┌─────────────────────────────────────────────────────┐
│  valori-node process                                 │
│                                                      │
│  ┌──────────── standalone mode ──────────────────┐  │
│  │  Engine (KernelState + WAL + EventLogWriter)  │  │
│  │  Planner → EffectBus → KernelCapability impl  │  │
│  └───────────────────────────────────────────────┘  │
│                        OR                            │
│  ┌──────────── cluster mode ─────────────────────┐  │
│  │  DataPlaneState                               │  │
│  │  raft.client_write() → ValoriStateMachine     │  │
│  │    → KernelState (applied on every peer)      │  │
│  │  Planner → EffectBus → KernelCapability impl  │  │
│  └───────────────────────────────────────────────┘  │
│                                                      │
│  Both modes expose the same HTTP surface (axum)      │
└──────────┬──────────────────────────────────────────┘
           │ HTTP/JSON
     ┌─────┴────────────────────┐
     │                          │
     ▼                          ▼
valori-mcp process         valori-cli process
(stdio MCP server;         (short-lived; spawns
 calls /v1/memory/*        node in-process OR
 over HTTP/reqwest;        calls remote HTTP;
 no shared memory)         no shared memory)


valori-ffi (.so)                  valori-verify (binary)
  Loaded into Python process.       Standalone; reads
  Wraps Engine directly             event log files
  (in-process, shared memory        directly from disk.
  via Arc<Mutex<Engine>>).          No live node needed.
  Also exposes verify_log_file      Callable as a library
  as a pyfunction (calls            from valori-ffi.
  valori-verify lib directly).
```

**Key runtime distinctions**:

| Crate | Compile-time coupling | Runtime coupling |
|---|---|---|
| `valori-mcp` | None to valori (HTTP client only) | HTTP to a running `valori-node` |
| `valori-cli` | Depends on `valori-node` | May spawn node in-process OR call remote HTTP |
| `valori-ffi` | Directly links `valori-node` + `valori-verify` | In-process; Python holds `Arc<Mutex<Engine>>` |
| `valori-verify` | Only `valori-kernel` + `valori-wire` | File I/O only; never connects to a live node |
| `valori-consensus` | Linked into `valori-node` | Only active in cluster mode; idle in standalone |
| `EffectBus` (valori-effect) | Defined in `valori-effect` | Capability impls registered at node startup by `valori-node`; runtime trait-object dispatch |

**Standalone vs cluster is a runtime decision, not a compile-time branch.**
Both paths are compiled into the same `valori-node` binary. The binary selects
the path at startup based on `VALORI_NODE_ID` / `VALORI_CLUSTER_MEMBERS`.
This is why every HTTP endpoint must be implemented in both `server.rs` and
`cluster_server.rs`.

---

## Global invariants

These are architecture-level contracts. A PR that breaks any of them is wrong
regardless of whether tests pass.

### Determinism

Given identical inputs — `KernelEvent` stream, snapshot bytes, fixed-point
format — every node must produce identical outputs: `KernelState`,
`hash_state_blake3`, and snapshot bytes.

No wall-clock time, OS RNG, thread scheduling, filesystem ordering, or
floating-point arithmetic may influence deterministic state.

Corollary: `valori-kernel` is `no_std`. If you need std, the code belongs in a
higher crate.

### Replay

`KernelState` is mutated through exactly one path:

```
KernelEvent
    ↓
KernelState::apply_event_ns(event, namespace_id)
```

No crate may mutate `KernelState` fields directly. No crate may call
`apply_event` without going through `apply_event_ns` (the namespace-aware
entry point).

This invariant is what makes the audit chain meaningful: every state change is
a `KernelEvent` that was applied at a specific namespace, in a specific order,
with a specific BLAKE3 chain entry.

### Recovery has exactly one public entry point

```
valori_state::bootstrap::recover_from_events()
```

No other crate decides which persistence layer is authoritative (event log,
snapshot, WAL, or fresh start). That decision belongs to `valori-state::bootstrap`
and nowhere else.

This invariant was violated when `valori-storage::recovery` existed as a
duplicate of `valori-state::bootstrap`. It must not recur.

### `valori-kernel` is `no_std`

`crates/valori-kernel/src/lib.rs` carries `#![cfg_attr(not(feature = "std"), no_std)]`.
This must never be removed. Every new dependency in `valori-kernel/Cargo.toml`
must use `default-features = false`.

Verify after any change to the kernel:
```
cargo build -p valori-kernel --target wasm32-unknown-unknown
```

### Every ID type is defined once

`RecordId`, `NodeId`, `NamespaceId`, `EdgeId`, `ShardId` are defined in
`valori-core` or `valori-kernel`. No other crate may define a structurally
identical local duplicate. If a crate needs the type, it imports it.

---

## Never do this

These are the specific mistakes the codebase has already paid to remove.
Finding one in a PR is a revert.

- **Add `std`-only deps to `valori-kernel`** without gating behind `#[cfg(feature = "std")]`.
- **Serialize `KernelState` directly** from any crate other than `valori-kernel::snapshot`.
- **Bypass `KernelEvent`** to mutate kernel state. No direct field writes.
- **Read event logs from `valori-node`** using `read_event_log()` or equivalent. Use `read_all_segments()` — it preserves namespace and handles multi-segment rotation.
- **Import `valori-node` into any lower crate**. It is a leaf.
- **Define a duplicate recovery path**. `valori-state::bootstrap` is the one and only orchestrator.
- **Define a duplicate ID type**. If two crates need the same ID, it belongs in `valori-core`.
- **Regenerate compatibility fixtures** to fix a failing test. A failing fixture test means a format regression. Fix the regression; do not regenerate the fixtures.
- **Add speculative public API**. Every `pub fn` is a compatibility contract. Use `pub(crate)` until the API has an external caller.

---

## Layer ownership

### `valori-core` — type foundation

**Owns**: shared IDs (`RecordId`, `NodeId`, `NamespaceId`, `EdgeId`, `ShardId`),
shared error types, cross-crate traits.  
**Does not own**: any I/O, any business logic.  
**Constraint**: `no_std` + minimal deps (`serde`, `thiserror`, `getrandom` behind `std`).

| Item | Visibility | Notes |
|---|---|---|
| `RecordId`, `NodeId`, `EdgeId`, `NamespaceId`, `CollectionId`, `ShardId`, `ClusterEpoch`, `ExecutionId` | Public — used externally | Every crate that addresses entities imports from here |
| `NodeKind`, `EdgeKind` | Public — used externally | Graph node/edge type enums |
| `CoreError`, `Result` | Public — used externally | Base error type |
| `Version` | Public — used externally | Semantic version helper |

---

### `valori-kernel` — deterministic vector store

**Owns**: `KernelState`, `KernelEvent`, `apply_event_ns`, `hash_state_blake3`,
snapshot encode/decode (V7 current), fixed-point arithmetic (`FxpScalar` / `FxpVector`),
HNSW/BQ/IVF index structures, BLAKE3 audit helpers.  
**Does not own**: file I/O, network I/O, thread spawning, wall-clock time.  
**Constraint**: `no_std`. See invariant above.

| Item | Visibility | Notes |
|---|---|---|
| `KernelState` (`state::kernel`) | Public — used externally | Central mutable state; apply events through `apply_event_ns` only |
| `KernelEvent` (`event`) | Public — used externally | Every mutation variant; stable contract |
| `KernelConfig` (`config`) | Public — used externally | Dimension, capacity, index kind |
| `FxpScalar`, `FxpVector` (`fxp`) | Public — used externally | Q16.16 fixed-point arithmetic |
| `encode_snapshot`, `decode_snapshot` (`snapshot`) | Public — used externally | V7 snapshot format; format version is a stable contract |
| `hash_state_blake3` (`crypto`) | Public — used externally | Merkle state hash; domain is a stable contract |
| `HnswIndex`, `BruteForceIndex`, `IvfIndex`, `BqIndex` (`index`) | Public — used externally | Index impls; swappable via `KernelConfig` |
| `RecordPool`, `Record` (`storage`) | Public — internal only | Slab allocator; not part of the external contract |
| `GraphNode`, adjacency helpers (`graph`) | Public — internal only | Knowledge-graph node store |
| `dot`, `l2_sq` (`math`) | Public — internal only | SIMD-dispatched inner product and L2; used by index impls |

---

### `valori-wire` — serialization types + event-log wire format

**Owns**: `KernelEvent` serde structs, V2/V3/V4 event-log encode/decode,
`chain_advance`, `parse_header`, `decode_entry`, `encode_entry`,
`MAX_ENTRIES_PER_SEGMENT`, `MAX_ENTRY_DECODE_BYTES`.  
**Does not own**: file handles, recovery logic, state machines.  

Note: V4 format includes a per-entry CRC. Any byte corruption in an entry body
is caught as `Failure::Decode` before the BLAKE3 chain check fires. This means
`valori-verify` may return `tampered_structural` rather than `tampered_chain`
for arbitrary byte flips — both are valid detections.

| Item | Visibility | Notes |
|---|---|---|
| `LogEntry`, `AdminEvent` | Public — used externally | Union of event variants written to the log |
| `EntryV2`, `EntryV3`, `EntryV4` (`EntryV4 = EntryV3`) | Public — used externally | Wire structs; V4 is a stable format contract |
| `DecodedEntry`, `SegmentHeader` | Public — used externally | Decode output types |
| `parse_header`, `encode_header_v3`, `encode_header_v4`, `encode_header_v2` | Public — used externally | Header encode/decode |
| `decode_entry`, `encode_entry` | Public — used externally | Entry-level codec; used by storage and verify |
| `chain_advance_v2`, `chain_advance_v3`, `chain_advance` | Public — used externally | BLAKE3 chain helpers; used by storage and verify |
| `hex`, `format_utc` | Public — used externally | Formatting utilities |
| `WireError` | Public — used externally | Error type |

---

### `valori-storage` — bytes on disk

**Owns**:
- WAL: `WalWriter`, `WalReader`, `LegacyWalCommand` (v1 backward-compat)
- Event log: `EventLogWriter`, `recover_from_event_log`, `read_all_segments`,
  `EventJournal`, `EventCommitter`
- Object store: `ObjectStoreBackend` (S3/file snapshot offload + WAL archival)
- `compute_event_log_hash` (file-level BLAKE3, used by `/v1/proof/event-log`)

**Does not own**: which files to load, in what order, on startup — that is
`valori-state`.  
**Does not own**: entry-by-entry chain verification — that is `valori-verify`.

Key distinction — two different BLAKE3 operations, two different purposes:
- `compute_event_log_hash` = BLAKE3 of raw file bytes (quick integrity, HTTP layer)
- `valori_verify::verify_log_file` = entry-by-entry chain replay + BLAKE3 (full audit)

| Item | Visibility | Notes |
|---|---|---|
| `WalWriter`, `WalReader`, `WalHeader`, `WalEntryIterator` | Public — used externally | WAL write/read primitives; called by `valori-state` |
| `EventLogWriter` | Public — used externally | Appends V4 entries; stable contract |
| `recover_from_event_log` | Public — used externally | Replays a log file into `KernelState`; called by `valori-state::bootstrap` |
| `read_all_segments` | Public — used externally | Multi-segment log reader returning `(namespace_id, KernelEvent)` tuples |
| `EventJournal`, `EventCommitter`, `CommitResult` | Public — used externally | Write path helpers used by `valori-node` |
| `compute_event_log_hash` | Public — used externally | File-level BLAKE3; used by `/v1/proof/event-log` endpoint |
| `ObjectStoreBackend` | Public — used externally | S3/file snapshot offload + WAL archival |
| `StorageError` | Public — used externally | Error type |
| `LegacyWalCommand` | Public — internal only | V1 backward-compat deserializer; not a forward contract |

---

### `valori-state` — recovery orchestration

**Owns**: `BootstrapMode`, `recover_from_events` (the single public entry point).  
**Internal helpers** (`pub(crate)`, not public API): `has_wal`, `has_event_log`,
`load_snapshot`, `validate_snapshot`, `replay_wal`.

**Does not own**: raw byte I/O (that is `valori-storage`).  
**Does not own**: HTTP, Raft, or anything network-facing.

**Recovery priority order** (enforced in `bootstrap.rs`):
1. Event log — canonical truth; replay from scratch
2. Snapshot — fast-path cache; loaded only when event log is absent/empty
3. WAL — legacy fallback; replayed on top of existing state
4. Fresh start — no durable state found

| Item | Visibility | Notes |
|---|---|---|
| `recover_from_events` | Public — used externally | The one and only recovery entry point; called by `valori-node` startup |
| `BootstrapMode` | Public — used externally | Enum describing which path was taken (EventLog / Snapshot / Wal / Fresh) |
| `StateError`, `StateResult` | Public — used externally | Error type |
| `has_wal`, `has_event_log`, `load_snapshot`, `validate_snapshot`, `replay_wal` | Internal (`pub(crate)`) | Implementation details of `recover_from_events`; not stable API |

---

### `valori-metadata` — control-plane persistence

**Owns**: `MetadataDb` (redb-backed store), `Project`, `Collection`,
`CollectionRegistry`, `PlannerCacheEntry`/`PlannerCacheKey`.  
**Does not own**: kernel state, snapshot bytes, Raft log — those are in their
respective crates.

| Item | Visibility | Notes |
|---|---|---|
| `MetadataDb` | Public — used externally | Opens/creates `~/.valori/metadata.redb`; the single control-plane store |
| `Project`, `ProjectMode`, `IndexKind`, `ClusterNodeConfig` | Public — used externally | Project config types persisted by the UI / CLI |
| `Collection`, `CollectionRegistry`, `MAX_COLLECTIONS` | Public — used externally | Collection↔NamespaceId mapping; registry is rebuilt at startup |
| `PlannerCacheEntry`, `PlannerCacheKey` | Public — used externally | Planner graph cache persisted across restarts |
| `MetadataError`, `MetadataResult` | Public — used externally | Error type |

---

### `valori-planner` — operation planning

**Owns**: `Planner` trait, `IngestPlanner`, `NoOpPlanner`, `plan_with_cache`,
`ExecutionGraph`, `TaskSpec`, `ExecutionRegistry`, `ExecutionCache`.  
**Does not own**: HTTP routing, kernel mutations — it only produces a `TaskSpec`
DAG; execution happens in `valori-node`.

| Item | Visibility | Notes |
|---|---|---|
| `Operation`, `OperationId`, `OperationHash`, `OperationKind`, `OperationInputs` | Public — used externally | Stable input contract; hash identifies operation uniquely |
| `ExecutionPolicy`, `ConsistencyLevel`, `ResourceBudget` | Public — used externally | Per-operation execution constraints |
| `PlanningContext`, `CapabilitySet`, `PlannerFingerprint`, `PlanningContextHash` | Public — used externally | Context fed into planner to produce deterministic DAGs |
| `ExecutionGraph`, `GraphHash`, `TaskId`, `TaskSpec`, `TaskKind`, `TaskEdge` | Public — used externally | Output of planning; consumed by `valori-effect` and `valori-node` |
| `ExecutionRetentionPolicy` | Public — used externally | Retention config carried on `ExecutionGraph` |
| `Planner`, `IngestPlanner`, `NoOpPlanner`, `plan_with_cache` | Public — used externally | Planning entry points |
| `ExecutionRegistry`, `ExecutionCache`, `ExecutionContext`, `ExecutionHandle`, `ExecutionStatus`, `TaskState`, `CacheKey` | Public — used externally | Runtime execution tracking |
| `PlannerError`, `PlannerResult` | Public — used externally | Error type |

---

### `valori-effect` — effect system

**Owns**: `EffectBus`, the seven capability traits, `Receipt`, `ReceiptAssembler`,
`ReceiptEnvelope`, `ReceiptStore`, `verify_receipt`.  
**Does not own**: HTTP routing, kernel — effects are routed through the bus to
capability implementations registered in `valori-node`.

| Item | Visibility | Notes |
|---|---|---|
| `EffectBus` | Public — used externally | Routes kernel writes, receipts, audit entries, and metrics from task execution |
| `KernelCapability`, `EmbedCapability`, `LlmCapability`, `StorageCapability`, `HttpCapability`, `ProofCapability`, `SchedulerCapability` | Public — used externally | The seven capability traits; implementations live in `valori-node` |
| `CapabilityRegistry`, `NoOpKernelCapability` | Public — used externally | Registry for wiring capabilities; NoOp for tests |
| `Effect`, `EffectId`, `EffectDurability`, `EffectPayload`, `KernelCommand`, `ReceiptFragment` | Public — used externally | Core effect types |
| `Receipt`, `ReceiptAssembler`, `ReceiptEnvelope`, `ReceiptHash`, `ReceiptStore`, `StateHash`, `verify_receipt` | Public — used externally | BLAKE3 receipt chain; returned to callers as proof of execution |
| `Task`, `NoOpTask`, `TaskContext`, `TaskOutput` | Public — used externally | Task trait + context passed during execution |
| `EffectError`, `EffectResult` | Public — used externally | Error type |

---

### `valori-consensus` — Raft state machine

**Owns**: `ValoriStateMachine` (wraps `KernelState` as an openraft state
machine), `LogStoreRedb`, gRPC peer transport, `ClientRequest`/`ClientResponse`.  
**Write path**: `client_write(KernelEvent)` → Raft log → `apply()` on all
nodes → `KernelState` mutated identically on every peer.  

Partitioning: one `ValoriStateMachine` per `ShardId`. Today's routing is
`namespace_id % shard_count`. Future routing strategies must remain
deterministic — consensus owns partitioning, and any change to the routing
function is a breaking change to the audit chain.

| Item | Visibility | Notes |
|---|---|---|
| `ValoriStateMachine` | Public — used externally | openraft state machine; one per `ShardId` |
| `ClientRequest`, `ClientResponse` | Public — used externally | Stable cluster write contract |
| `ValoriNode` | Public — used externally | openraft `Node` impl (gRPC address) |
| `ValoriLogStore` (trait), `RedbLogStore` | Public — used externally | Log store trait + redb-backed impl |
| `RaftNetworkFactory`, `RaftNetwork`, `RaftNetworkConnection` | Public — used externally | gRPC peer transport |
| `AuditSink`, `MemoryAuditSink`, `NullAuditSink` | Public — used externally | Pluggable audit-entry sink; production uses `EventLogWriter` |
| `ShardId`, `NodeId`, `LogId`, `Vote`, `Entry`, `Raft` | Public — used externally | openraft type aliases; `ShardId` re-exported from `valori-core` |
| `partition_harness` | Public — internal only | Test-only chaos harness for replication divergence tests |

---

### `valori-node` — HTTP server + cluster orchestration

**Owns**: axum routes, `Engine` (standalone), `DataPlaneState` (cluster),
community layer, tree-RAG, decay re-rank, Valori Reranker, GraphRAG traversal,
object-store endpoints, WAL writer (standalone path).

**Two execution paths — both must be maintained for every endpoint**:

| Path | Router | State access | Write mechanism |
|---|---|---|---|
| Standalone | `server.rs` | `SharedEngine` | `engine.write().await` |
| Cluster | `cluster_server.rs` | `DataPlaneState` | `raft.client_write(KernelEvent)` |

Shared handler bodies live in `crates/valori-node/src/routes/` via the `*Ops`
trait pattern. `tests/route_parity.rs` mechanically enforces that every `/v1`
route exists in both routers (or is listed in `STANDALONE_ONLY` / `CLUSTER_ONLY`
with a documented reason).

| Item | Visibility | Notes |
|---|---|---|
| `Engine`, `SharedEngine` | Public — used externally | Standalone engine wrapper; used by `valori-ffi`, `valori-mcp`, `valori-cli` |
| `NodeConfig` | Public — used externally | All `VALORI_*` env-var config; used by callers that start a node programmatically |
| `build_router` (standalone), `build_cluster_router` | Public — used externally | axum router constructors; used in integration tests |
| `ClusterHandle` | Public — used externally | Cluster startup handle; holds shard handles + watcher tasks |
| `events::event_replay::read_all_segments` | Public — used externally | Re-exported from `valori-storage`; used by cluster integration tests |
| `events::event_proof::compute_event_log_hash` | Public — used externally | File-level BLAKE3; `/v1/proof/event-log` endpoint |
| `decay` (`rerank`, `decay_factor`) | Public — internal only | Read-time decay re-rank; pure, never mutates state |
| `community` (`CommunityStore`, `label_propagation`, etc.) | Public — internal only | Phase I6 community layer; std-only |
| `tree_rag` (`TreeIndex`, `tree_verify`, etc.) | Public — internal only | Phase I5 Tree-RAG; std-only |
| `valori_reranker` (`ValoriReranker`) | Public — internal only | Phase C5 hybrid vector+term reranker; std-only |
| `graph_rag` (`expand_subgraph`, `resolve_seed_nodes`) | Public — internal only | GraphRAG traversal; shared by standalone and cluster routers |

---

### `valori-verify` — standalone audit binary + library

**Owns**: `verify_log_file` (entry-by-entry BLAKE3 chain replay, JSON report),
the `valori-verify` binary.  
**Verdicts**: `verified`, `tampered_chain`, `tampered_structural`,
`tampered_semantic`, `tampered_content`.  
**Constraint**: std-only. Never import into `valori-kernel`.

| Item | Visibility | Notes |
|---|---|---|
| `verify_log_file(path) -> Value` | Public — used externally | Full audit: replays every entry, checks CRC + BLAKE3 chain; returns JSON report with `verdict` and `replay.chain_head` |
| `valori_wire` (re-exported) | Public — used externally | Re-exports `valori-wire` for callers that need wire types alongside verification |
| `valori-anchor` binary | Binary | Alias for the `valori-verify` binary under an alternate name |

---

### `valori-ffi` — PyO3 embedded SDK

**Owns**: `ValoricoreEngine` (wraps `Engine` behind `Arc<Mutex<>>`), all
`#[pyfunction]` / `#[pyclass]` bindings.  
**Constraint**: std-only. Lock engine with `lock_engine!` macro; never bypass
the lock. Use `save_snapshot()` (flushes WAL pending writes) — `save()` was
deleted because it skipped the flush.

| Item | Visibility | Notes |
|---|---|---|
| `ValoricoreEngine` (`#[pyclass]`) | Public — Python API | Wraps `Engine` for in-process Python; exposes insert, search, graph, snapshot methods |
| All `#[pyfunction]` bindings | Public — Python API | The embedded SDK surface: `insert_record`, `search_records`, `create_namespace`, etc. |
| `lock_engine!` macro | Internal | Safety macro; ensures every method holds the mutex for its full duration |

---

### `valori-mcp` — Model Context Protocol server

**Owns**: MCP stdio server, `memory_recall` tool (returns BLAKE3 receipt),
`McpServer`, `tool_definitions`.  
**Constraint**: std-only. Depends on `valori-node` for all state access.

| Item | Visibility | Notes |
|---|---|---|
| `McpServer<C>` | Public — binary entrypoint | Generic over `NodeClient`; serves MCP protocol over stdio |
| `tool_definitions()` | Public — used externally | Returns the MCP tool manifest (tool names + JSON schemas) |
| `backend` module | Public — internal only | `NodeClient` trait + HTTP client impl for talking to a running `valori-node` |
| `protocol`, `stdio`, `receipt` modules | Public — internal only | MCP wire protocol + receipt serialization |

---

### `valori-cli` — CLI binary

**Owns**: `valori` binary — `setup` wizard, `cluster` subcommand, `timeline`
subcommand, `verify`, `diff`, `inspect`, `import`, `replay-query`.  
**Constraint**: std-only. Talks to a running `valori-node` over HTTP or starts
one in-process.

| Item | Visibility | Notes |
|---|---|---|
| `commands::wizard` | Internal | Interactive `setup` wizard for new projects |
| `commands::cluster` | Internal | Cluster management subcommands (add-node, remove-node, status) |
| `commands::timeline` | Internal | Replays the audit log and renders a timeline view |
| `commands::verify` | Internal | Calls `valori-verify` on a local log file |
| `commands::diff` | Internal | Diffs two snapshot or log files |
| `commands::inspect` | Internal | Inspects snapshot or log file contents |
| `commands::import` | Internal | Bulk-imports records from CSV/JSON |
| `commands::replay_query` | Internal | Replays a query against a historical snapshot |
| `engine` module | Internal | In-process engine startup helper used by CLI subcommands |

---

## Compatibility ownership

Binary compatibility is owned by the crate that defines the format. Format
migrations belong in the owning crate. Adding a new format version in the wrong
crate is an architecture violation.

| Format | Owner | Current version | Compatibility fixtures |
|---|---|---|---|
| Snapshot | `valori-kernel` | V7 | `crates/valori-kernel/tests/fixtures/` |
| Event-log wire | `valori-wire` | V4 | `crates/valori-storage/tests/fixtures/` (segment) |
| WAL | `valori-storage` | V2 | `crates/valori-storage/tests/fixtures/` |
| Event-log end-to-end | `valori-state` | — | `crates/valori-state/tests/fixtures/` |
| Verify JSON report | `valori-verify` | schema_version 1 | — |

---

## Stable public contracts

These are the APIs that external consumers (Python SDK, audit tools, cluster
peers) depend on. Changing them is a breaking change and requires a format
version bump and a new compatibility fixture.

- `KernelEvent` variants and their fields
- Snapshot binary format (magic `VALK`, schema version 7)
- Event-log wire format (V4 with per-entry CRC + BLAKE3 chain)
- WAL format (V2 — `KernelEvent + namespace_id` bincode pairs)
- `valori_verify::verify_log_file` JSON report schema (schema_version 1)
- `hash_state_blake3` domain (the Merkle tree structure over all events)

Everything else — internal struct layouts, `pub(crate)` helpers, handler
implementations — is an implementation detail that can be refactored freely.

---

## Compatibility fixtures

Fixtures are committed binary corpora that lock format contracts at a specific
commit. They are the only reliable way to detect accidental serialization drift,
because roundtrip tests (`encode → decode → equal`) evolve with the code and
cannot detect it.

| Corpus | Location | What it pins |
|---|---|---|
| Snapshot V7 | `crates/valori-kernel/tests/fixtures/` | encoder output + `hash_state_blake3` |
| WAL V2 | `crates/valori-storage/tests/fixtures/` | `WalWriter` output + replay hash |
| Event-log end-to-end | `crates/valori-state/tests/fixtures/` | `EventLogWriter` + `recover_from_event_log` + chain_head + verify verdict |

**Never regenerate these fixtures to fix a failing test.** A failing fixture
test is a format regression. Find the commit that changed the output, revert
it, and fix the underlying issue. Regenerate only when intentionally bumping a
format version, and commit the old fixtures alongside the new ones under a
versioned name.

---

## Ownership summary table

| Concern | Crate |
|---|---|
| Fixed-point vector arithmetic | `valori-kernel` |
| Snapshot encode / decode | `valori-kernel` |
| `hash_state_blake3` | `valori-kernel` |
| Event wire format (encode/decode/CRC/chain) | `valori-wire` |
| WAL write / read | `valori-storage` |
| Event log write / read | `valori-storage` |
| Multi-segment replay (`read_all_segments`) | `valori-storage` |
| File-level log hash | `valori-storage` |
| Recovery orchestration (which files, what order) | `valori-state` |
| Entry-by-entry chain verification | `valori-verify` |
| Raft consensus + partitioning | `valori-consensus` |
| HTTP endpoints (both paths) | `valori-node` |
| Python FFI | `valori-ffi` |

---

## Decision rules for new features

**New kernel mutation** (new event type):  
→ Add variant to `KernelEvent` in `valori-kernel/src/event.rs`, handle in
`KernelState::apply_event_ns`. Add to wire format in `valori-wire` if it needs
to cross a process boundary.

**New persistence primitive** (new file format, new WAL variant):  
→ `valori-storage`. If it involves deciding which primitives to load on startup,
the decision belongs in `valori-state::bootstrap`.

**New HTTP endpoint**:  
→ Both `server.rs` (standalone) and `cluster_server.rs` (cluster). Use the
`routes/` shared-handler pattern. Run `cargo test -p valori-node --test
route_parity` to verify parity. See CLAUDE.md dual-path checklist.

**New verification / audit capability**:  
→ `valori-verify` (binary + library). Never add file I/O to `valori-kernel`.

**New Python SDK method**:  
→ `crates/valori-ffi/src/lib.rs` (embedded) and
`python/valoricore/remote.py` (remote — both `SyncRemoteClient` and
`AsyncRemoteClient`).

**Anything that requires `std`**:  
→ Cannot go in `valori-kernel`. Gate behind `#[cfg(feature = "std")]` or place
it in a higher crate.

**Two crates need the same concept**:  
→ Move it downward. Do not import upward or duplicate it.

---

## PR checklist

Before opening a pull request, verify all of these:

- [ ] Layer ownership respected — new code lives in the right crate
- [ ] No upward dependency introduced — `cargo build --workspace` is clean
- [ ] Public API justified — every new `pub fn` has an external caller today, not a hypothetical one
- [ ] `no_std` kernel preserved — `cargo build -p valori-kernel --target wasm32-unknown-unknown` passes
- [ ] WASM build passes (same check, surfaced explicitly)
- [ ] Route parity passes — `cargo test -p valori-node --test route_parity`
- [ ] Compatibility fixtures intact — no fixture test failures; if format changed intentionally, new fixtures committed alongside old ones under a versioned name
- [ ] `CLAUDE.md` dual-path checklist completed for any new HTTP endpoint
- [ ] Changes follow `docs/architecture/layers.md`

---

## Design philosophy

1. **One abstraction per crate.** If a crate is doing two things, it should be two crates, or one thing should move.
2. **Primitives go down, orchestration goes up.** When a concept is needed in multiple layers, define the primitive at the lowest layer that can hold it without an upward import.
3. **Deterministic over flexible.** When a choice exists between a deterministic primitive and a flexible one, prefer determinism. It is what makes the audit chain meaningful.
4. **Prefer removing abstractions over adding new ones.** The refactors that produced this document removed duplicate recovery paths, duplicate ID types, and speculative public APIs. The direction is convergence, not proliferation.
5. **Every public API is a compatibility contract.** Use `pub(crate)` until the API has a real external caller. A `pub fn` with no callers is future maintenance debt, not future flexibility.
