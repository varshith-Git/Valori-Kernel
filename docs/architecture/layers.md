# Valori — Normative Architecture Document

This is the normative architecture document for the Valori codebase. It defines
ownership, invariants, and allowed dependency directions. When adding a new
capability, resolve its layer here first. If something spans layers, put the
primitive in the lower crate and the orchestration in the higher one.

Changes to this document require explicit architectural reasoning. It is not a
description of what exists — it is a contract for what is permitted.

Referenced by: `CONTRIBUTING.md`, `CLAUDE.md`.

---

## Dependency graph

```
valori-core
    └── valori-kernel  (no_std — the deterministic core)
            └── valori-wire  (shared serde types + event-log wire format)
                    └── valori-storage  (WAL + event log + object store — bytes on disk)
                            └── valori-state  (recovery orchestration — bootstrap only)
                                    ├── valori-consensus  (Raft state machine, wraps kernel)
                                    └── valori-node  (HTTP server + cluster orchestration)
                                            ├── valori-ffi  (PyO3 embedded SDK)
                                            └── valori-verify  (standalone audit binary + library)
```

**Layering rule**: arrows point downward only. No crate may import from a crate
above it. Adding an upward import is an architecture violation — move the shared
concept into a lower crate instead.

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

---

### `valori-kernel` — deterministic vector store

**Owns**: `KernelState`, `KernelEvent`, `apply_event_ns`, `hash_state_blake3`,
snapshot encode/decode (V7 current), fixed-point arithmetic (`FxpScalar` / `FxpVector`),
HNSW/BQ/IVF index structures, BLAKE3 audit helpers.  
**Does not own**: file I/O, network I/O, thread spawning, wall-clock time.  
**Constraint**: `no_std`. See invariant above.

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

---

### `valori-verify` — standalone audit binary + library

**Owns**: `verify_log_file` (entry-by-entry BLAKE3 chain replay, JSON report),
the `valori-verify` binary.  
**Verdicts**: `verified`, `tampered_chain`, `tampered_structural`,
`tampered_semantic`, `tampered_content`.  
**Constraint**: std-only. Never import into `valori-kernel`.

---

### `valori-ffi` — PyO3 embedded SDK

**Owns**: `ValoricoreEngine` (wraps `Engine` behind `Arc<Mutex<>>`), all
`#[pyfunction]` / `#[pyclass]` bindings.  
**Constraint**: std-only. Lock engine with `lock_engine!` macro; never bypass
the lock. Use `save_snapshot()` (flushes WAL pending writes) — `save()` was
deleted because it skipped the flush.

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
