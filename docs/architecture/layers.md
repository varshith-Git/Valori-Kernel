# Valori — Crate Layer Architecture

This document is the authoritative reference for where features belong. When
adding a new capability, find its layer here first. If it spans layers, put the
primitive in the lower crate and the orchestration in the higher one.

---

## Dependency graph

```
valori-core
    └── valori-kernel  (no_std, the deterministic core)
            └── valori-wire  (shared serialization types + event-log wire format)
                    └── valori-storage  (WAL + event log + object store — bytes on disk)
                            └── valori-state  (recovery orchestration — state lifecycle)
                                    ├── valori-consensus  (Raft state machine, wraps kernel)
                                    └── valori-node  (HTTP server, cluster orchestration)
                                            ├── valori-ffi  (PyO3 embedded SDK)
                                            └── valori-verify  (standalone audit binary)
```

**Rule**: dependency arrows point downward only. No crate may import from a
crate above it in this graph. Adding an upward import is an architecture
violation — split the shared concept into a lower crate instead.

---

## Layer ownership

### `valori-core` — type foundation
**Owns**: shared IDs (`RecordId`, `NodeId`, `NamespaceId`), shared error types,
cross-crate traits.  
**Does not own**: any I/O, any business logic.  
**Constraint**: `no_std` + minimal deps (`serde`, `thiserror`, `getrandom` behind `std`).

---

### `valori-kernel` — deterministic vector store
**Owns**: `KernelState`, `KernelEvent`, `apply_event_ns`, `hash_state_blake3`,
snapshot encode/decode, fixed-point arithmetic (`FxpScalar`/`FxpVector`),
HNSW/BQ/IVF index structures, BLAKE3 audit helpers.  
**Does not own**: any file I/O, any network I/O, any thread spawning.  
**Constraint**: `no_std` (compiles for `wasm32-unknown-unknown`). Every new dep
must use `default-features = false`. Verify with:
```
cargo build -p valori-kernel --target wasm32-unknown-unknown
```

---

### `valori-wire` — serialization types + event-log wire format
**Owns**: `KernelEvent` serde structs, V2/V3/V4 event-log encode/decode,
`chain_advance`, `parse_header`, `decode_entry`, `encode_entry`,
`MAX_ENTRIES_PER_SEGMENT`, `MAX_ENTRY_DECODE_BYTES`.  
**Does not own**: file handles, recovery logic, state machines.  
**Note**: V4 includes a per-entry CRC. Any byte corruption in an entry is
caught as `Failure::Decode` before the BLAKE3 chain check fires.

---

### `valori-storage` — bytes on disk
**Owns**:
- WAL: `WalWriter`, `WalReader`, `LegacyWalCommand` (v1 backward-compat)
- Event log: `EventLogWriter`, `recover_from_event_log`, `read_all_segments`,
  `EventJournal`, `EventCommitter`
- Object store: `ObjectStoreBackend` (S3/file snapshot offload + WAL archival)
- `compute_event_log_hash` (file-level BLAKE3, used by `/v1/proof/event-log`)

**Does not own**: which files to load, in what order, on startup. That is
recovery orchestration and belongs in `valori-state`.  
**Does not own**: verification of the chain entry-by-entry. That is
`valori-verify`.

**Key distinction**:
- `compute_event_log_hash` = BLAKE3 of raw file bytes (quick integrity check)
- `valori_verify::verify_log_file` = full entry-by-entry chain verification (audit path)

---

### `valori-state` — recovery orchestration
**Owns**: `BootstrapMode`, `recover_from_events` (the single entry point the
node calls on startup).  
**Internal helpers** (`pub(crate)`, not exported): `has_wal`, `has_event_log`,
`load_snapshot`, `validate_snapshot`, `replay_wal`.

**Does not own**: raw byte I/O (that is `valori-storage`).  
**Does not own**: HTTP, Raft, or anything network-facing.

**Recovery priority order** (enforced in `bootstrap.rs`):
1. Event log — canonical truth; replay from scratch
2. Snapshot — fast-path cache; loaded only when event log is absent/empty
3. WAL — legacy fallback; replayed on top of an existing state
4. Fresh start — no durable state found

---

### `valori-consensus` — Raft state machine
**Owns**: `ValoriStateMachine` (wraps `KernelState` as an openraft state
machine), `LogStoreRedb`, gRPC peer transport, `ClientRequest`/`ClientResponse`.  
**Write path**: `client_write(KernelEvent)` → Raft log → `apply()` on all
nodes → `KernelState` mutated identically on every peer.  
**Constraint**: one `ValoriStateMachine` per `ShardId`; namespace→shard routing
is `ns_id % shard_count`.

---

### `valori-node` — HTTP server + cluster orchestration
**Owns**: axum routes, `Engine` (standalone), `DataPlaneState` (cluster),
community layer, tree-RAG, decay re-rank, Valori Reranker, GraphRAG traversal,
object-store endpoints, WAL writer (standalone path).

**Two execution paths — both must be maintained**:

| Path | Router | State access | Write mechanism |
|---|---|---|---|
| Standalone | `server.rs` | `SharedEngine` | `engine.write().await` |
| Cluster | `cluster_server.rs` | `DataPlaneState` | `raft.client_write(KernelEvent)` |

Shared handler bodies live in `crates/valori-node/src/routes/` via the `*Ops`
trait pattern. `tests/route_parity.rs` enforces that every `/v1` route exists
in both routers (or is listed in `STANDALONE_ONLY` / `CLUSTER_ONLY` with a
documented reason).

---

### `valori-verify` — standalone audit binary
**Owns**: `verify_log_file` (entry-by-entry BLAKE3 chain replay, returns JSON
verdict), `valori-verify` binary.  
**Verdicts**: `verified`, `tampered_chain`, `tampered_structural`,
`tampered_semantic`, `tampered_content`.  
**Constraint**: std-only, never import into `valori-kernel`.

---

### `valori-ffi` — PyO3 embedded SDK
**Owns**: `ValoricoreEngine` (wraps `Engine` behind `Arc<Mutex<>>`), all
`#[pyfunction]` / `#[pyclass]` bindings.  
**Constraint**: std-only. Lock engine with `lock_engine!` macro; never bypass
the lock. Use `save_snapshot()` (flushes WAL pending writes), never the deleted
`save()`.

---

## Ownership summary table

| Concern | Crate |
|---|---|
| Fixed-point vector arithmetic | `valori-kernel` |
| Snapshot encode / decode | `valori-kernel` |
| `hash_state_blake3` | `valori-kernel` |
| Event wire format (encode/decode/CRC) | `valori-wire` |
| WAL write / read | `valori-storage` |
| Event log write / read | `valori-storage` |
| Multi-segment replay (`read_all_segments`) | `valori-storage` |
| File-level log hash | `valori-storage` |
| Recovery orchestration (which files, what order) | `valori-state` |
| Entry-by-entry chain verification | `valori-verify` |
| Raft consensus | `valori-consensus` |
| HTTP endpoints | `valori-node` |
| Python FFI | `valori-ffi` |

---

## Decision rules for new features

**New kernel mutation** (e.g. new event type):  
→ Add variant to `KernelEvent` in `valori-kernel/src/event.rs`, handle in
`KernelState::apply_event_ns`. Add to wire format in `valori-wire` if it needs
to cross a process boundary.

**New persistence primitive** (new file format, new WAL variant):  
→ `valori-storage`. If it involves orchestrating multiple primitives on startup,
the orchestration belongs in `valori-state::bootstrap`.

**New HTTP endpoint**:  
→ Both `server.rs` (standalone) and `cluster_server.rs` (cluster). Use the
shared `routes/` handler pattern. Run `cargo test -p valori-node --test
route_parity` to verify parity. See CLAUDE.md checklist.

**New verification / audit capability**:  
→ `valori-verify` (binary + library). Never add file I/O to `valori-kernel`.

**New Python SDK method**:  
→ `crates/valori-ffi/src/lib.rs` (embedded) and
`python/valoricore/remote.py` (remote — both `SyncRemoteClient` and
`AsyncRemoteClient`).

**Anything that requires `std`**:  
→ Cannot go in `valori-kernel`. Gate behind `#[cfg(feature = "std")]` or put
it in a higher crate.
