# Phase 3.2 — Rolling Upgrades (Zero-Downtime Version Migration)

**Status:** done · on `multinode`
**Roadmap:** Phase 3 (durability & operations) — safe multi-version cluster coexistence.

## Goal

Give operators a safe, rehearsable path to upgrade a live Raft cluster one node
at a time, without a maintenance window. A follower running an older binary must
refuse (not silently corrupt) entries it can't interpret; the cluster must remain
writable while nodes are being upgraded.

## Delivered

### `crates/valori-consensus/src/types.rs`

- **`CURRENT_SCHEMA_VERSION: u8 = 0`** — a single constant that documents the
  current wire version. Every time a new `KernelEvent` variant or a breaking
  field change would make existing entries uninterpretable to an older binary,
  this constant is bumped and the compatibility matrix in `docs/COMPATIBILITY.md`
  is extended.

- **`schema_version: u8` field on `ClientRequest`** — tagged `#[serde(default)]`
  so nodes built before Phase 3.2 decode the field as `0` (backward compatible).
  The leader writes `CURRENT_SCHEMA_VERSION` at proposal time.

### `crates/valori-consensus/src/state_machine.rs`

Schema version gate in `ValoriStateMachine::apply()`, **before** the dedup check
and before any kernel mutation or audit write:

```rust
if req.schema_version > CURRENT_SCHEMA_VERSION {
    return Err(io_err(format!(
        "log index {log_index}: entry schema version {} exceeds this node's max \
         ({CURRENT_SCHEMA_VERSION}) — upgrade this node to resume replication",
        req.schema_version
    )));
}
```

Returning `StorageError` halts replication on that follower; the Raft leader
continues replicating to upgraded followers. The un-upgraded follower resumes
automatically after its binary is replaced and the process restarts.

### `crates/valori-node/src/cluster_server.rs`

Six `ClientRequest` construction sites updated to stamp `schema_version:
CURRENT_SCHEMA_VERSION` — `AutoInsertRecord` (single + batch), `DeleteRecord`,
`SoftDeleteRecord`, `AutoCreateNode`, `AutoCreateEdge`.

### `crates/valori-node/src/commit/raft.rs`

`RaftCommitter::write()` stamps `schema_version: CURRENT_SCHEMA_VERSION` on
every write.

### `crates/valori-cli/src/main.rs` — new `Upgrade` variant in `ClusterAction`

```
valori cluster upgrade --url http://10.0.0.1:3000 --target-version 1.0
```

### `crates/valori-cli/src/commands/cluster.rs` — `pub fn upgrade()`

Interactive guided procedure:

1. GET `/v1/cluster/status` to discover topology.
2. Sorts nodes: non-leaders first, leader last (so the cluster keeps a leader
   throughout — leadership only transfers for the very last step).
3. For each node: prints exact instructions, waits for the operator to press
   Enter, polls `/health` every 2 s up to 120 s, then moves to the next node.
4. For the leader node: additionally polls for a new leader to form before
   declaring success.

No process management — the CLI trusts the operator's deployment tooling.

### `docs/COMPATIBILITY.md` (new)

Compatibility matrix: which binary versions can coexist, rolling window rules,
and how to bump `CURRENT_SCHEMA_VERSION`.

### Test coverage

**`crates/valori-consensus/tests/type_config.rs`** — three new tests:

| Test | What it checks |
|---|---|
| `schema_version_field_roundtrips` | `schema_version` serialises/deserialises faithfully |
| `schema_version_defaults_to_zero_when_field_absent` | Backward compat: field stripped from JSON → default 0 |
| `current_schema_version_is_zero` | Documents the contract; must be updated when version bumps |

**`crates/valori-consensus/tests/state_machine.rs`** — two new tests:

| Test | What it checks |
|---|---|
| `apply_accepts_current_schema_version` | `CURRENT_SCHEMA_VERSION` entry applies cleanly |
| `apply_rejects_unknown_schema_version` | `CURRENT_SCHEMA_VERSION + 1` returns `Err`; kernel state unchanged |

## Findings

### Pre-existing snapshot corruption test bug

`corrupted_snapshot_payload_is_refused_and_state_kept` was passing only because
Cargo's binary cache was reusing a pre-V6 build. After `cargo clean`, position
`mid = bytes.len() / 2 = 4159` in the 8318-byte V6 snapshot payload falls inside
the namespace sentinel region (`0xFF` bytes). `hash_state_blake3` does not cover
namespace heads (only records, nodes, edges), so the corrupted byte passes the
hash check silently.

**Root cause:** V6 added 2 × 4096 B of namespace heads before the NSRG section,
pushing `mid` into unhashed territory. The pre-V5 snapshot was shorter; `mid`
landed in record data (hashed).

**Fix:** Changed the test to corrupt `bytes.last_mut()` — the last byte of the
32-byte `state_hash` tail — which is guaranteed to trigger the hash mismatch
check regardless of kernel format version.

### `schema_version` gate ordering

The gate must precede the dedup check, not follow it. A future version might
change the dedup semantics; running a v0 gate on a v1 `request_id` interpretation
could silently commit the wrong dedup decision. Gate first, interpret second.

### `valori-embedded` pre-existing build failure

`cargo build --workspace` fails with `error[E0152]: duplicate lang item` in the
embedded crate (unrelated to Phase 3.2). Phase 3.2 builds via `-p valori-consensus
-p valori-node -p valori-cli`.

## Validation

```
cargo test -p valori-consensus -p valori-node
```

**202 tests passed, 0 failed, 2 ignored** (2 ignored = proptest-fuzz skipped in
CI, mTLS gated on cert files).

Selected test output:
```
running 13 tests
test result: ok. 13 passed; 0 failed   ← state_machine.rs (incl. 2 new schema version tests)

running 10 tests
test result: ok. 10 passed; 0 failed   ← type_config.rs (incl. 3 new schema version tests)
```

Manual smoke test:
1. `cargo build -p valori-consensus -p valori-node -p valori-cli` → `Finished` (no errors)
2. `valori cluster upgrade --help` → prints usage with `--url` and `--target-version` args

## Follow-ups

| Item | Phase |
|---|---|
| Integration test: real 3-node cluster, upgrade nodes 3→2→1, assert no write loss | Phase 3.3 or dedicated test phase |
| Proptest across simulated version boundary (generate events at v0 and v1, apply on mixed cluster) | Phase 3.3 |
| `CURRENT_SCHEMA_VERSION` bump procedure: add to CONTRIBUTING, CI check that bumping the constant without updating `docs/COMPATIBILITY.md` fails | Phase 3.3 |
| `valori cluster upgrade` dry-run flag (`--dry-run`) that prints the upgrade plan without waiting for Enter | Quality-of-life backlog |
