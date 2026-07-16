# RFC-0005: Crate Boundaries

**Status:** Draft  
**Owner:** Core team (architecture)  
**Stability:** Beta — boundary decisions are live and enforced by `tests/architecture.rs`; new crate extractions require an RFC update  
**Last reviewed:** 2026-07-08  
**Depends on:** [`rfcs/0000-glossary.md`](0000-glossary.md) through [`rfcs/0004-capability-model.md`](0004-capability-model.md)  
**Implements:** overall architecture; guides Phases A3–A8

---

## 1. Motivation

The current workspace has grown organically. `valori-node` contains types that
belong in shared crates, `valori-storage` owns a `recovery.rs` that belongs in
`valori-state`, and there is no crate that owns the control-plane types
(Project, Collection, Shard, ExecutionHistory, PlannerCache).

This RFC freezes the crate boundary decisions derived from the four RFC discussions.
No new crate is created without a corresponding entry in this document.

---

## 2. Target crate graph

```
                     valori-core        (no_std, zero deps)
                          │
                    valori-kernel       (no_std, core only)
                          │
              ┌───────────┼────────────┐
              │           │            │
       valori-wire   valori-storage  valori-state
       (serde only)  (WAL, events,   (bootstrap,
                      obj store)      lifecycle,
                                      manifest)
              │           │            │
              └───────────┴──────┬─────┘
                                 │
                        valori-metadata      (control plane)
                                 │
                        valori-planner       (Operation, ExecutionGraph)
                                 │
                          valori-node        (HTTP + runtime)
                         /         \
               valori-consensus   valori-ffi
               (Raft, gRPC)       (PyO3)
                                   │
                             valori-mcp    valori-verify    valori-cli
                             (MCP stdio)  (standalone)     (CLI binary)
```

Dependency rule: **a crate may only depend on crates below it in the graph**.
`valori-kernel` never depends on `valori-storage`. `valori-storage` never depends
on `valori-node`. Violations are enforced by `cargo deny` rules.

---

## 3. Crate inventory

### valori-core

**Status:** Complete (Phase A1)  
**`no_std`:** Yes  
**Owns:** `RecordId`, `NodeId`, `EdgeId`, `NamespaceId`, `CollectionId`, `ExecutionId`, `ShardId`, `ClusterEpoch`, `NodeKind`, `EdgeKind`, `Version`, `CoreError`  
**Does not own:** Anything that touches disk, network, or the kernel's `apply` logic.

---

### valori-kernel

**Status:** Existing (pre-redesign)  
**`no_std`:** Yes — enforced by WASM build in CI  
**Owns:** `KernelState`, `KernelEvent`, `KernelCommand`, `RecordPool`, `GraphNode`, `GraphEdge`, `FxpScalar`, snapshot encode/decode (V5/V6), BLAKE3 chaining helpers, index structures (BruteForce, HNSW, IVF, BQ)  
**Does not own:** File I/O, network, tokio, threads, `HashMap` (uses `no_std` btree maps)

---

### valori-storage

**Status:** Complete (Phase A2)  
**`no_std`:** No (uses tokio, opendal, blake3)  
**Owns:** `WalWriter`, `WalReader`, `EventLogWriter`, `EventJournal`, `EventCommitter`, event replay, event proof, `ObjectStoreBackend`, `StorageError`  
**Does NOT own:** `recovery.rs` — currently here but moves to `valori-state` in Phase A3  
**Note:** `recovery.rs` placement in `valori-storage` is a known A2 debt item.

---

### valori-state *(new — Phase A3)*

**`no_std`:** No  
**Owns:** State lifecycle (`Recovering`, `Ready`, `Snapshotting`), bootstrap (snapshot → WAL replay → `KernelState`), manifest (current snapshot path, segment list), graceful shutdown (snapshot-on-close), `StateError`  
**Receives from A2:** `recovery.rs` (moved here and rewritten as `bootstrap.rs`)  
**Depends on:** `valori-kernel`, `valori-storage`  
**Does not own:** The HTTP server, the Raft state machine, or any control-plane types.

---

### valori-wire

**Status:** Existing  
**`no_std`:** No (serde + serde_json)  
**Owns:** Shared serialization types used by `valori-node` ↔ Python SDK ↔ CLI: HTTP request/response structs, cluster wire types, snapshot metadata  
**Semver policy:** Strict — see `COMPATIBILITY.md`.

---

### valori-metadata *(new — Phase A4)*

**`no_std`:** No  
**Owns:**
- `Project` (name, dir, port, dim, index, created_at, last_opened_at)
- `Collection` (name → `NamespaceId` mapping, per-project)
- `Shard` topology (shard_id → Raft member list)
- `ExecutionHistory` (completed `ExecutionGraph` logical records + `Receipt` references)
- `ExecutionAnalytics` (optional time-series; resource usage per `OperationKind`)
- `PlannerCache` (key: `OperationHash + PlannerFingerprint.hash + PlanningContextHash` → `ExecutionGraph`)
- `SnapshotCatalog` (path, size, produced_at, format version, per-shard)
- `CompatibilityMatrix` (runtime check against `COMPATIBILITY.md`)

**Depends on:** `valori-core`, `valori-wire`  
**Storage backend:** `redb` (embedded, same as Raft log)  
**Does not own:** `KernelState`, network server, Raft.

---

### valori-planner *(new — Phase A5)*

**`no_std`:** No  
**Owns:** `Operation`, `OperationId`, `OperationHash`, `OperationKind`, `OperationInputs`, `ExecutionPolicy`, `ResourceBudget`, `PlanningContext`, `PlanningContextHash`, `PlannerFingerprint`, `ExecutionGraph`, `TaskSpec`, `TaskEdge`, `GraphHash`, `ExecutionRetentionPolicy`, `ExecutionRegistry`, `ExecutionCache`, `ExecutionHandle`, `ExecutionStatus`, `ExecutionContext`, `TaskState`, planner trait  
**Planner cache:** reads `ExecutionHistory` from `valori-metadata`  
**Depends on:** `valori-core`, `valori-metadata`  
**Does not own:** Task implementations, `EffectBus`, `KernelState`.

---

### valori-node

**Status:** Existing  
**`no_std`:** No  
**Owns:** HTTP server (axum), runtime task dispatch, `EffectBus`, `Effect` enum, `EffectId`, `EffectDurability`, capability traits and registry, task implementations, `Receipt`, `ReceiptAssembler`, `ReceiptFragment`, cluster server, cluster API, decay/reranker, community detection, Tree-RAG, WAL writer integration, ingest pipeline  
**After A6:** gains `effect.rs`, `effect_bus.rs`, `capability.rs`, `task.rs`, `tasks/` directory  
**After A8:** gains `receipt.rs`, `receipt_assembler.rs`; replaces `EventProof` + ad-hoc receipts  
**Does not own:** Kernel internals, Raft log, snapshot encoding.

---

### valori-consensus

**Status:** Existing  
**`no_std`:** No  
**Owns:** Raft state machine (`ValoriStateMachine`), openraft integration, Raft log (`redb`), gRPC peer transport  
**Depends on:** `valori-kernel`, `valori-storage` (for `StorageError`), `valori-wire`

---

### valori-ffi

**Status:** Existing  
**`no_std`:** No (PyO3)  
**Owns:** PyO3 bindings for in-process (embedded) Python SDK  
**Depends on:** `valori-kernel`, `valori-node`

---

### valori-mcp

**Status:** Existing  
**`no_std`:** No  
**Owns:** MCP stdio server; `memory_recall` returns a `Receipt` (post-A8: unified `Receipt`)  
**Depends on:** `valori-node` (or subset via library crate)

---

### valori-verify

**Status:** Existing  
**`no_std`:** No  
**Owns:** Standalone verifier binary — replays `events.log`, checks BLAKE3 chain  
**After A8:** implements `verify_receipt` against unified `Receipt`  
**Depends on:** `valori-kernel`, `valori-storage` only (intentional — independently auditable)

---

### valori-cli

**Status:** Existing  
**`no_std`:** No  
**Owns:** `valori` binary — `setup` wizard, `cluster` subcommand, `timeline` subcommand  
**Depends on:** `valori-wire`, `valori-node` (via HTTP client), `valori-metadata` (project manifest)

---

## 4. What lives where — quick reference

| Type | Crate |
|---|---|
| `RecordId`, `NodeId`, `NamespaceId`, `ShardId` | `valori-core` |
| `KernelState`, `KernelEvent`, `FxpScalar`, snapshot | `valori-kernel` |
| `WalWriter`, `EventLogWriter`, `EventCommitter`, `ObjectStoreBackend` | `valori-storage` |
| Bootstrap, manifest, lifecycle, `StateError` | `valori-state` |
| HTTP wire types, `ClientRequest` | `valori-wire` |
| `Project`, `Collection`, `ExecutionHistory`, `PlannerCache` | `valori-metadata` |
| `Operation`, `ExecutionGraph`, `PlannerFingerprint`, `ExecutionRegistry` | `valori-planner` |
| `Effect`, `EffectBus`, `CapabilityRegistry`, task impls, `Receipt`, `ReceiptAssembler` | `valori-node` |
| Raft, gRPC peer transport | `valori-consensus` |

---

## 5. The no_std line

```
no_std side                     std side
─────────────────────────────   ─────────────────────────────
valori-core                     valori-storage
valori-kernel                   valori-state
                                valori-wire
                                valori-metadata
                                valori-planner
                                valori-node
                                valori-consensus
                                valori-ffi
                                valori-mcp
                                valori-verify
                                valori-cli
```

The line is enforced by:
1. `valori-kernel/src/lib.rs`: `#![cfg_attr(not(feature = "std"), no_std)]`
2. CI: `cargo build -p valori-kernel --target wasm32-unknown-unknown`

No crate on the `no_std` side may take a dependency that requires std without
gating it behind `#[cfg(feature = "std")]`.

---

## 6. Dependency enforcement

`deny.toml` (cargo-deny) enforces:

```toml
[[bans.deny]]
name = "tokio"
wrappers = ["valori-storage", "valori-state", "valori-node", "valori-consensus", "valori-ffi", "valori-mcp", "valori-verify", "valori-cli"]
# tokio must not appear as a dependency of valori-core or valori-kernel
```

Additionally, a `cargo tree -p valori-kernel --edges no-dev` check in CI verifies
that `std`, `tokio`, and `std::io` do not appear in the kernel's transitive closure.

---

## 7. Phase sequencing constraints

| Phase | Crate | Prerequisite |
|---|---|---|
| A3 | `valori-state` | RFCs 0000–0002 frozen |
| A4 | `valori-metadata` | A3 complete (needs `StateError`) |
| A5 | `valori-planner` | A4 complete (needs `ExecutionHistory`) |
| A6 | Effect system in `valori-node` | A5 complete (needs `ExecutionGraph`, `TaskSpec`) |
| A7 | `ExecutionRegistry` + runtime | A6 complete (needs `EffectBus`) |
| A8 | `Receipt` + `ReceiptAssembler` | A7 complete (needs `EffectBus`, `ExecutionGraph`) |
| A9 | `valori-node` cleanup | A8 complete (remove redundant source files from A2) |

---

*Update this document when adding any new crate or moving a type between crates.
A PR that introduces a new `[package]` in the workspace without a corresponding
entry in this RFC will be rejected.*
