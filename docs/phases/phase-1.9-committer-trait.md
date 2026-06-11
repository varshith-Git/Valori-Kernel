# Phase 1.9 — `Committer` Trait Seam

**Status:** planned  
**Roadmap:** [MULTINODE_ROADMAP.md](../MULTINODE_ROADMAP.md) § 1.9  
**Why now:** Phase 2 plugs `valori-consensus` into the write path. Without a
trait seam the Raft adapter would have to fork `Engine` or reach inside
`EventCommitter`'s private fields. The seam must exist before Phase 2 starts,
and it must carry zero behavior change in standalone mode — `StandaloneCommitter`
is a verbatim wrap of the existing `EventCommitter` path.

---

## Goal

1. **Extract a `Committer` trait** that represents the *only* way to mutate
   `KernelState` through the engine. `Engine` owns a `Box<dyn Committer>`.
2. **`StandaloneCommitter`** wraps the existing `EventCommitter` shadow-exec →
   fsync → apply path verbatim. Zero behavior change. Zero new code path.
3. **Move capacity checks into shadow execution** so they run against the kernel
   state *before* any I/O, making them deterministic and replication-ready.
   Today they run in the HTTP handler against `Engine.max_records` — they must
   move to `Committer::apply`, which sees the state the Raft leader will apply.
4. **`RaftCommitter` stub** in `valori-consensus` that satisfies `Committer`
   but panics on call — a compile-time proof that the interface is correct
   before the Raft implementation exists.

---

## Problem: Current Write Path Is Not Seam-Able

### Today

```
HTTP handler
  → Engine::insert_record_from_f32()
      ├─ capacity check (against Engine.max_records) ← PROBLEM
      ├─ if event_committer.is_some():
      │     EventCommitter::commit_event()   ← standalone path
      │       shadow → fsync → live_apply
      └─ else:
            WalWriter::append_command()      ← legacy path
            KernelState::apply()
```

The capacity check lives *outside* `EventCommitter`. When Phase 2 replicates
a write through Raft, the leader's capacity check fires on the leader's local
`Engine.max_records` — but the follower applies the same event without any
capacity check, because `apply_event` (the Raft state-machine adapter) bypasses
the HTTP handler entirely. A follower could silently diverge if its pool is
full when the leader's was not.

### Target

```
HTTP handler
  → Engine::insert_record_from_f32()    ← NO capacity check here
      → self.committer.commit(event)
            ↓
     ┌──────────────────────────────────────────────────────────────────┐
     │  Committer::commit(event) → Result<CommitReceipt, CommitError>  │
     │                                                                  │
     │  1. shadow_apply(event)           ← capacity check happens here │
     │       if CapacityExceeded → Err(CommitError::Capacity)          │
     │       if other error → Err(CommitError::Apply)                  │
     │  2. persist(event)                ← fsync (standalone) or       │
     │                                      Raft propose (cluster)     │
     │  3. live_apply(event)             ← identical in both modes     │
     └──────────────────────────────────────────────────────────────────┘
```

The capacity check is now *inside shadow execution*, which runs against the
same `KernelState` that will receive the live apply. A follower that replays
the committed event through `KernelState::apply_event` also respects capacity
because the kernel enforces it natively — the check in shadow catches it before
any I/O happens.

---

## D1 — `Committer` Trait

```rust
// crates/valori-node/src/commit/mod.rs  [NEW — Phase 1.9]

use valori_kernel::event::KernelEvent;

/// Result of a successful commit.  Opaque to callers — used for metrics and
/// dedup tracking; the specific fields depend on the committer implementation.
pub struct CommitReceipt {
    /// Monotonically increasing log index of the committed event.
    /// In standalone mode this is the EventJournal committed height.
    /// In cluster mode this is the Raft commit index.
    pub log_index: u64,
}

/// The one way to mutate KernelState through the Engine.
///
/// # Invariants
///
/// * **Atomicity:** Either the event is committed (persisted + applied to live
///   state) or it is not; no partial states are visible.
/// * **Order:** Commits are strictly ordered by the returned `log_index`.
/// * **Determinism:** The same sequence of events produces the same
///   KernelState regardless of which implementation is behind the trait.
/// * **Capacity:** If shadow application returns `KernelError::CapacityExceeded`,
///   `commit` MUST return `Err(CommitError::Capacity)` without any I/O.
pub trait Committer: Send + Sync {
    /// Attempt to commit a single event.
    fn commit(&mut self, event: KernelEvent) -> Result<CommitReceipt, CommitError>;

    /// Attempt to commit a batch of events atomically.
    /// Default implementation commits events one at a time; override for
    /// group-commit optimisation.
    fn commit_batch(&mut self, events: Vec<KernelEvent>) -> Result<CommitReceipt, CommitError> {
        let mut last = None;
        for event in events {
            last = Some(self.commit(event)?);
        }
        last.ok_or(CommitError::EmptyBatch)
    }

    /// Current committed log height.  Used by health reporting and metrics.
    fn log_height(&self) -> u64;

    /// Flush any in-memory write buffer to durable storage.
    /// No-op in implementations that fsync on every commit.
    fn flush(&mut self) -> Result<(), CommitError>;
}

#[derive(Debug, thiserror::Error)]
pub enum CommitError {
    #[error("capacity exceeded: {pool} pool is full ({used}/{cap})")]
    Capacity { pool: &'static str, used: usize, cap: usize },

    #[error("shadow application rejected event: {0:?}")]
    Apply(valori_kernel::error::KernelError),

    #[error("persistence layer error: {0}")]
    Io(String),

    #[error("batch was empty — nothing to commit")]
    EmptyBatch,
}
```

All existing `CommitError` variants from `event_commit.rs` map to this set.
`StandaloneCommitter` translates them in its `From` impls.

---

## D2 — `StandaloneCommitter`

```rust
// crates/valori-node/src/commit/standalone.rs  [NEW — Phase 1.9]

use super::{Committer, CommitError, CommitReceipt};
use crate::events::event_commit::EventCommitter;
use valori_kernel::event::KernelEvent;
use valori_kernel::error::KernelError;

/// Wraps EventCommitter to implement the Committer trait.
/// Standalone mode: shadow → fsync → live_apply, exactly as today.
/// Zero behavior change — this is a delegation wrapper, not a reimplementation.
pub struct StandaloneCommitter {
    inner: EventCommitter,
    /// Capacity limits — moved here so shadow_apply can check them.
    max_records: usize,
    max_nodes:   usize,
    max_edges:   usize,
}

impl StandaloneCommitter {
    pub fn new(inner: EventCommitter, max_records: usize, max_nodes: usize, max_edges: usize) -> Self {
        Self { inner, max_records, max_nodes, max_edges }
    }
}

impl Committer for StandaloneCommitter {
    fn commit(&mut self, event: KernelEvent) -> Result<CommitReceipt, CommitError> {
        // ── Capacity check in shadow ──────────────────────────────────────────
        // These checks mirror what the kernel's apply_event already does for
        // most cases, but we surface a richer CommitError::Capacity so the
        // HTTP layer can return HTTP 507 with an informative body.
        let state = self.inner.live_state();
        match &event {
            KernelEvent::InsertRecord { .. } if state.record_count() >= self.max_records =>
                return Err(CommitError::Capacity {
                    pool: "records", used: state.record_count(), cap: self.max_records,
                }),
            KernelEvent::CreateNode { .. } if state.node_count() >= self.max_nodes =>
                return Err(CommitError::Capacity {
                    pool: "nodes", used: state.node_count(), cap: self.max_nodes,
                }),
            KernelEvent::CreateEdge { .. } if state.edge_count() >= self.max_edges =>
                return Err(CommitError::Capacity {
                    pool: "edges", used: state.edge_count(), cap: self.max_edges,
                }),
            _ => {}
        }

        // ── Delegate to EventCommitter (shadow → fsync → live_apply) ──────────
        self.inner.commit_event(event)
            .map(|_| CommitReceipt { log_index: self.inner.journal().committed_height() })
            .map_err(|e| match e {
                crate::events::event_commit::CommitError::LiveApply(ke) => CommitError::Apply(ke),
                crate::events::event_commit::CommitError::EventLog(io) =>
                    CommitError::Io(io.to_string()),
                _ => CommitError::Io(e.to_string()),
            })
    }

    fn log_height(&self) -> u64 {
        self.inner.journal().committed_height()
    }

    fn flush(&mut self) -> Result<(), CommitError> {
        self.inner.flush_log().map_err(|e| CommitError::Io(e.to_string()))
    }
}
```

### Engine changes

`Engine` field `event_committer: Option<EventCommitter>` is replaced by
`committer: Box<dyn Committer>` in Phase 1.9. The legacy WAL path becomes a
second implementation (`WalCommitter`) for backward compatibility during the
transition, also wrapping the existing WAL writer verbatim.

All `Engine` methods that currently contain:
```rust
if let Some(ref mut committer) = self.event_committer {
    committer.commit_event(event.clone())…
} else {
    // WAL path
}
```
become a single call:
```rust
self.committer.commit(event)?;
```

The capacity guard blocks scattered across `insert_record_from_f32`,
`insert_batch`, `create_node_for_record`, and `create_edge` are **removed from
those methods** and live exclusively in `StandaloneCommitter::commit`.

---

## D3 — Capacity Checks Move Into Shadow Execution

### Why this matters for Raft

In Phase 2, the `RaftCommitter` path is:

```
HTTP leader → RaftCommitter::commit(event)
  → Raft propose(event)          ← network round trip
  → On quorum: apply to state machine
     → KernelState::apply_event(event)
```

The Raft state machine adapter (`valori-consensus`) calls
`KernelState::apply_event` directly for committed entries. The leader's HTTP
handler must not allow an event to be proposed if it would be rejected by the
kernel on any replica — because a rejected apply on a follower is a divergence
(the follower cannot be in a different state from the leader after the same
committed log entries).

With capacity checks *inside shadow execution against KernelState*, the leader
pre-validates against the same logic the kernel uses on apply. If the check
passes shadow, it will pass on every replica (capacity is replicated state).
If it fails shadow, the request is rejected before any I/O, so no log entry is
ever proposed.

### Capacity as replicated state

`KernelState` tracks the count of live records, nodes, and edges. These counts
are part of the replicated state machine. They are included in the BLAKE3 state
hash. A capacity check against `KernelState` is therefore a check against
*identical* data on every replica — it is deterministic and replication-safe.

`Engine.max_records / max_nodes / max_edges` are configuration values (same on
every node by operator guarantee). The combination of replicated counts +
shared config makes capacity checks a distributed invariant.

---

## D4 — `RaftCommitter` Stub

```rust
// crates/valori-consensus/src/raft_committer.rs  [NEW — Phase 1.9 stub]

use valori_node::commit::{Committer, CommitError, CommitReceipt};
use valori_kernel::event::KernelEvent;

/// Compile-time proof that valori-consensus can satisfy the Committer trait.
/// All methods panic — this is a type-system stub, not an implementation.
/// Phase 2 replaces the panic bodies with Raft propose/wait logic.
pub struct RaftCommitter;

impl Committer for RaftCommitter {
    fn commit(&mut self, _event: KernelEvent) -> Result<CommitReceipt, CommitError> {
        panic!("RaftCommitter not yet implemented — Phase 2")
    }
    fn log_height(&self) -> u64 {
        panic!("RaftCommitter not yet implemented — Phase 2")
    }
    fn flush(&mut self) -> Result<(), CommitError> {
        panic!("RaftCommitter not yet implemented — Phase 2")
    }
}
```

This file must compile cleanly as part of the Phase 1.9 acceptance gate. It
proves the trait is object-safe and the import paths are correct.

---

## D5 — HTTP 507 Body Improvement

Today `EngineError::Kernel(KernelError::CapacityExceeded)` produces:
```json
{"error": "CapacityExceeded"}
```

With `CommitError::Capacity { pool, used, cap }` the body becomes:
```json
{
  "error": "capacity_exceeded",
  "pool": "records",
  "used": 1024,
  "cap": 1024,
  "hint": "Increase VALORI_MAX_RECORDS or delete unused records"
}
```

No change to the HTTP status code (still 507). The richer body is purely
additive — existing clients that check the status code are unaffected.

---

## Migration Path (zero behavior change)

| Step | Action | Observable change |
|---|---|---|
| 1 | Create `commit/mod.rs` with trait + error types | None — unused |
| 2 | Create `commit/standalone.rs` wrapping `EventCommitter` | None — unused |
| 3 | Create `commit/wal.rs` wrapping `WalWriter` (legacy path) | None — unused |
| 4 | Replace `Engine.event_committer` and `Engine.wal_writer` with `committer: Box<dyn Committer>` | Behavior identical; HTTP 507 body gains pool/used/cap fields |
| 5 | Remove capacity checks from HTTP handlers / `Engine` methods | Behavior identical — checks now in `StandaloneCommitter::commit` |
| 6 | Add `RaftCommitter` stub to `valori-consensus` | Compile-time proof only |

Steps 1–3 can be done in parallel. Steps 4–5 must happen together in one commit
(search for the `if let Some(ref mut committer)` pattern — 7 call sites in
`engine.rs`).

---

## Files Changed

| File | Action | Notes |
|---|---|---|
| `crates/valori-node/src/commit/mod.rs` | NEW | `Committer` trait, `CommitError`, `CommitReceipt` |
| `crates/valori-node/src/commit/standalone.rs` | NEW | `StandaloneCommitter` |
| `crates/valori-node/src/commit/wal.rs` | NEW | `WalCommitter` (legacy) |
| `crates/valori-node/src/engine.rs` | MODIFY | Replace `event_committer`/`wal_writer` with `committer`; remove 7 capacity-guard call sites |
| `crates/valori-node/src/lib.rs` | MODIFY | `pub mod commit;` |
| `crates/valori-consensus/src/raft_committer.rs` | NEW | Stub |
| `crates/valori-consensus/src/lib.rs` | MODIFY | `pub mod raft_committer;` |

---

## Tests

| Test | Location | What it proves |
|---|---|---|
| `standalone_commit_single` | `tests/committer.rs` | `StandaloneCommitter::commit` round-trips |
| `standalone_capacity_records` | `tests/committer.rs` | HTTP 507 with `pool="records"` |
| `standalone_capacity_nodes` | `tests/committer.rs` | HTTP 507 with `pool="nodes"` |
| `standalone_capacity_edges` | `tests/committer.rs` | HTTP 507 with `pool="edges"` |
| `wal_committer_smoke` | `tests/committer.rs` | Legacy WAL path still works |
| `raft_committer_compiles` | `valori-consensus/tests/stub.rs` | `RaftCommitter: Committer` compiles |
| `capacity_check_in_shadow_not_handler` | `tests/committer.rs` | Capacity error returns before any disk write |

---

## Acceptance Criteria

- `cargo check --workspace` (excluding `embedded`) clean.
- All existing tests pass without modification — no behavior change in standalone mode.
- HTTP 507 body gains `pool`, `used`, `cap` fields; status code unchanged.
- `RaftCommitter` in `valori-consensus` compiles and satisfies `Committer`.
- Capacity checks removed from all `Engine::*` methods; present only in `StandaloneCommitter::commit`.

---

## Findings

Design-only phase — no runtime findings. One forward concern:

**`Engine.state` is still public.** After the seam, `Engine::insert_record_from_f32`
and friends become thin forwarding shims. But `Engine.state: KernelState` is
`pub` — external code (server.rs, FFI) can read it for search/get operations
without going through `Committer`. That is correct and intentional: reads are
not mutations. However, the seam must not leak a mutable reference to `state`
to callers — all mutable access must go through `Committer::commit`. Phase 2
audit item: grep for `engine.state.apply` outside of committer code.

## Follow-ups

- Phase 2: Replace `RaftCommitter` panic bodies with `openraft::RaftClient::propose`.
- Phase 2: `commit_batch` override in `RaftCommitter` for group-commit (one quorum fsync per batch).
- Phase 3: `Committer::commit_encrypted` (crypto-shredding path, Phase 1.5) follows the same signature.
- Phase 3: Rate-limit `CommitError::Capacity` metrics to avoid cardinality explosion in Prometheus.
