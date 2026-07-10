# RFC-0002: Kernel Contract

**Status:** Draft  
**Owner:** valori-kernel / valori-consensus  
**Stability:** Beta — kernel event schema is versioned; state machine contract is live but may gain fields  
**Last reviewed:** 2026-07-08  
**Depends on:** [`rfcs/0000-glossary.md`](0000-glossary.md)  
**Implements:** `valori-kernel` API surface, `valori-consensus` state machine contract, Phase A3 (`valori-state`)

---

## 1. Motivation

The Kernel is the deterministic core of Valori. Its contract defines:

1. What inputs it accepts (`KernelCommand` / `KernelEvent`).
2. What it produces (mutations to `KernelState`, audit log entries).
3. What guarantees it provides (determinism, exactly-once, no external I/O).
4. Where the determinism boundary lies (inside the kernel's `apply` call).

This RFC makes these guarantees explicit so that any crate that wraps the kernel
(consensus, standalone engine, verifier) can implement the same contract independently.

---

## 2. The determinism boundary

The Kernel's `apply` function is a pure state transition:

```
KernelState × KernelEvent → KernelState'
```

Every call with the same starting state and the same event produces the same ending state.
No system calls, no file I/O, no random numbers, no timestamps.

The boundary is enforced by the `no_std` invariant: `valori-kernel` compiles for
`wasm32-unknown-unknown`. Any code that would require `std` cannot live in the kernel.

---

## 3. KernelCommand

A `KernelCommand` is a typed, higher-level instruction from the runtime to the kernel.
It wraps one or more `KernelEvent`s that will be applied atomically.

```rust
pub struct KernelCommand {
    pub id: CommandId,           // UUID; used for exactly-once dedup
    pub namespace_id: NamespaceId,
    pub shard_id: ShardId,
    pub payload: KernelCommandPayload,
}

pub struct CommandId(uuid::Uuid);

pub enum KernelCommandPayload {
    InsertRecord { vector: Vec<FxpScalar>, metadata: Option<Vec<u8>> },
    DeleteRecord { record_id: RecordId },
    InsertNode { kind: NodeKind, label: String },
    InsertEdge { from: NodeId, to: NodeId, kind: EdgeKind, weight: Option<FxpScalar> },
    DeleteNode { node_id: NodeId },
    DeleteEdge { edge_id: EdgeId },
    CreateNamespace { name: String },
    DropNamespace,
    // ... extensible
}
```

### Command→Event mapping

The standalone engine and the Raft state machine both translate `KernelCommand`
to `KernelEvent`(s) before calling `KernelState::apply_event_ns()`. This translation
is the only place where a `KernelEvent` may be constructed outside the kernel.

### Exactly-once guarantee

Every `KernelCommand` carries a `CommandId`. The state machine:

1. Checks whether `CommandId` is in the seen-set.
2. If yes: drops the command, returns success (idempotent).
3. If no: applies the command, records `CommandId` in the seen-set.

The seen-set is part of `KernelState` and is included in the state hash.

---

## 4. KernelEvent

`KernelEvent` is the atomic mutation record. It is the kernel's output format —
the source of truth for replay and audit.

```rust
pub enum KernelEvent {
    RecordInserted { record_id: RecordId, namespace_id: NamespaceId, vector_len: u16 },
    RecordDeleted { record_id: RecordId, namespace_id: NamespaceId },
    NodeInserted { node_id: NodeId, kind: NodeKind, namespace_id: NamespaceId },
    EdgeInserted { edge_id: EdgeId, from: NodeId, to: NodeId, kind: EdgeKind },
    NodeDeleted { node_id: NodeId },
    EdgeDeleted { edge_id: EdgeId },
    NamespaceCreated { namespace_id: NamespaceId, name_hash: [u8; 32] },
    NamespaceDropped { namespace_id: NamespaceId },
    // ... one variant per atomic kernel mutation
}
```

`KernelEvent` is defined in `valori-kernel/src/event.rs` and is the only type
exported from the kernel that may appear in the audit log.

---

## 5. KernelABI

The `KernelABI` is the versioned interface token embedded in every `Receipt`.

```rust
pub struct KernelABI {
    pub semantic_version: semver::Version,
    pub event_schema_hash: [u8; 32],   // BLAKE3(canonical serialization of KernelEvent enum)
    pub state_schema_hash: [u8; 32],   // BLAKE3(canonical serialization of KernelState fields)
}
```

`KERNEL_ABI_VERSION` is a static constant in `valori-kernel/src/lib.rs`.
It is the value embedded in every `Receipt` produced while that kernel version is running.

**Change policy:** see [`COMPATIBILITY.md`](../COMPATIBILITY.md) — KernelABI.

---

## 6. Apply protocol

The apply protocol is identical whether running standalone or in a Raft cluster.

```
1. DEDUP CHECK
   - Look up CommandId in the seen-set.
   - If found: return Ok(()) without applying.

2. KERNEL APPLY
   - Translate KernelCommand → KernelEvent(s).
   - Call KernelState::apply_event_ns(namespace_id, event) for each event.
   - If any apply returns Err: rollback (discard all events from this command), return Err.

3. AUDIT WRITE
   - Append each KernelEvent to the BLAKE3-chained audit log.
   - Update the running state hash.
   - Record CommandId in the seen-set.
```

The shadow-first commit barrier in `EventCommitter` (Phase A2) implements steps 2 and 3:
shadow-apply to a clone of `KernelState`, then live-apply, then persist. A failed
shadow-apply prevents any log write (invariant I-08).

---

## 7. Namespace isolation

Every `apply_event_ns()` call takes an explicit `namespace_id`. Namespace isolation
is enforced at three independent points:

| Point | Location |
|---|---|
| Event apply | `KernelState::apply_event_ns()` in `state/kernel.rs` |
| WAL replay | `recovery::replay_wal()` in `valori-storage/src/recovery.rs` |
| Index rebuild | `KernelState::build_index()` after snapshot restore |

If a fourth apply path is added, it must include a namespace guard. This is invariant I-04
applied to the namespace dimension.

---

## 8. The no_std boundary

`valori-kernel` is `no_std` (invariant I-15 from `INVARIANTS.md`). This is structural,
not optional:

- `use std::` is prohibited in any file under `crates/valori-kernel/src/`.
- Use `core::` or `alloc::` instead.
- Anything requiring std (file I/O, threads, `HashMap`) must be gated behind `#[cfg(feature = "std")]`.
- New dependencies must use `default-features = false`.
- Verified by: `cargo build -p valori-kernel --target wasm32-unknown-unknown` (run in CI).

`valori-consensus`, `valori-node`, and `valori-cli` opt into std via `features = ["std"]`.

---

## 9. One Task = One Transaction (invariant I-07 + I-08)

From the kernel's perspective, all `KernelCommand`s from one Task are applied to the
same shard in the same logical transaction. The `EventCommitter` ensures atomicity:
either all events from the Task land in the audit log, or none do.

Implications for Task authors:

- A Task that needs to write to two shards is a design error — it must be split into
  two Tasks in the `ExecutionGraph`, one per shard.
- A Task must not hold references to `KernelState` across an await point.
- A Task must not issue `KernelCommand`s to different `ShardId`s.

---

## 10. Verifier contract

The standalone verifier (`valori-verify`) replays an `events.log` and checks the
BLAKE3 chain. It does not execute Tasks or route Effects — it only:

1. Reads `KernelEvent`s from the log in order.
2. Calls `KernelState::apply_event_ns()` for each event.
3. Verifies the running BLAKE3 hash matches the chain embedded in the log.

The verifier depends only on `valori-kernel` and `valori-storage`. It does not depend on
`valori-node`, `valori-consensus`, or `valori-planner`. This separation is intentional —
the verifier must be independently auditable.

---

## 11. valori-state scope (Phase A3)

`valori-state` owns the lifecycle around `KernelState` — not `KernelState` itself.
Specifically:

| Responsibility | Module |
|---|---|
| Bootstrap (snapshot → WAL replay → running state) | `valori-state::bootstrap` |
| Manifest (which snapshot is current, segment list) | `valori-state::manifest` |
| State lifecycle (ready, recovering, snapshotting) | `valori-state::lifecycle` |
| Graceful shutdown (snapshot-on-close) | `valori-state::shutdown` |

`recovery.rs` currently in `valori-storage` is the wrong home — it orchestrates a state
lifecycle, not raw byte movement. It will move to `valori-state` in Phase A3.

---

## 12. Files to create/modify

| File | Change |
|---|---|
| `crates/valori-kernel/src/event.rs` | Authoritative `KernelEvent` enum (already exists; freeze the ABI) |
| `crates/valori-kernel/src/command.rs` | `KernelCommand`, `CommandId`, `KernelCommandPayload` |
| `crates/valori-kernel/src/lib.rs` | Export `KERNEL_ABI_VERSION: KernelABI` as a static |
| `crates/valori-state/src/bootstrap.rs` | `bootstrap(snapshot_path, event_log_path) → KernelState` |
| `crates/valori-state/src/manifest.rs` | Snapshot manifest: current snapshot path, segment list |
| `crates/valori-state/src/lifecycle.rs` | `StateLifecycle` enum: Recovering, Ready, Snapshotting |
| `crates/valori-state/src/shutdown.rs` | `shutdown_snapshot(engine, path)` — graceful snapshot-on-close |
