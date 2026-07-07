# Phase A9 — Node Cleanup + RaftKernelCapability

## Goal

Replace the `NoRaftKernelCapability` placeholder stub with a real `RaftKernelCapability`
that submits mutations through `raft.client_write()`, achieving parity between the
standalone `EngineKernelCapability` and the cluster execution path.

## Delivered

### `crates/valori-node/src/capabilities.rs`

| Symbol | Description |
|---|---|
| `RaftKernelCapability` | Implements `KernelCapability` against the live Raft cluster. `apply_command()` deserializes `event_json → KernelEvent`, wraps it in a `ClientRequest { schema_version: CURRENT_SCHEMA_VERSION, namespace_id, event, request_id }`, calls `raft.client_write()`, and returns the BLAKE3 state hash from `sm.state_hash().await`. |
| `NoRaftKernelCapability` | Renamed from the A7 placeholder to a test-only stub (`is_available() = false`, always returns `CapabilityUnavailable`). |

`RaftKernelCapability::new` accepts:
- `Arc<valori_consensus::types::Raft>` — the shard's Raft handle
- `ValoriStateMachine` — for reading the post-apply state hash
- `shard_count: u8`

The `state_hash()` sync fn returns 64 zeros (best-effort; callers that need the
live hash use `apply_command` instead, which returns the authoritative post-write hash).

## Findings

- `ClientRequest.request_id` is `Option<[u8; 16]>` (raw bytes), not a `Uuid` — the
  initial draft used `uuid::Uuid::parse_str()`. Fixed to a raw byte copy of the first
  16 bytes of the hex request_id string.
- `CURRENT_SCHEMA_VERSION` (not `SCHEMA_VERSION`) is the exported constant name in
  `valori_consensus::types`.
- `ValoriStateMachine::state_hash()` returns `[u8; 32]`, not `String` — needed an
  explicit hex conversion.

## Validation

```
cargo build -p valori-node
```

Clean build, 3 pre-existing warnings (unrelated to A9).

```
cargo test -p valori-kernel -p valori-node -p valori-effect
```

All suites pass; 0 failures. Total: ~245 tests across all binaries.

## Follow-ups

- Wire `RaftKernelCapability` into `cluster_server.rs`'s request handlers — currently
  the cluster path's HTTP handlers write directly to Raft without going through the
  capability/task system. Phase A10 should introduce a shared handler layer that routes
  through the `TaskRunner` on both paths.
- The `state_hash()` sync method on cluster always returns zeros — a future phase
  should cache the last-known hash from the state machine in an `AtomicPtr` or similar.
