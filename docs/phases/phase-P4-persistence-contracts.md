# Phase P4 — Persistence Contract Corpus

## Goal

Lock the complete persistence pipeline against accidental format drift. Roundtrip tests (encode→decode→equal) don't catch serialization regressions because both sides of the test evolve together. Committed binary fixtures with pinned hashes are immutable — any encoding change breaks them immediately in CI.

## Delivered

### `crates/valori-kernel/tests/snapshot_compat.rs`

Five forever-decode tests against committed V7 binary fixtures:

| Test | Fixture | Pins |
|---|---|---|
| `snapshot_v7_empty_decodes_forever` | `snapshot_v7_empty.bin` | state_hash hardcoded |
| `snapshot_v7_single_decodes_forever` | `snapshot_v7_single.bin` | state_hash from `.hash` file |
| `snapshot_v7_multi_decodes_forever` | `snapshot_v7_multi.bin` | state_hash from `.hash` file |
| `snapshot_v7_multi_can_continue_after_restore` | same | restored + 1 event == replay-from-scratch + 1 event |

Generator: `generate_snapshot_fixtures` (#[ignore]) — writes `.bin` + `.hash`.

### `crates/valori-storage/tests/wal_compat.rs`

Two forever-replay tests:

| Test | Fixture | Events | Pins |
|---|---|---|---|
| `wal_v1_inserts_replays_forever` | `wal_v1_inserts.wal` | 20 (ns 0) | state_hash |
| `wal_v1_namespace_replays_forever` | `wal_v1_namespace.wal` | 16 (ns 0 + ns 1) | state_hash |

### `crates/valori-state/tests/event_log_compat.rs` + TOML manifests

End-to-end corpus exercising both recovery paths:

**Path 1**: `EventLogWriter` → `recover_from_event_log` → `hash_state_blake3`  
**Path 2**: same bytes → `valori_verify::verify_log_file` (audit path)

Each fixture is paired with a TOML manifest pinning four invariants:

```toml
event_count  = 24
record_count = 24
chain_head   = "74c436e1..."
state_hash   = "2c4ae8a2..."
```

| Test | Fixture | Events |
|---|---|---|
| `event_log_inserts_verifies_forever` | `event_log_inserts.log` | 24 InsertRecord (ns 0) |
| `event_log_namespace_verifies_forever` | `event_log_namespace.log` | 20 events (12 ns 0, 8 ns 1) |

**Malformed artifact hardening**:

| Test | Fixture | What it checks |
|---|---|---|
| `bad_magic_returns_err_not_panic` | `bad_magic.log` | `verify_log_file` returns `Err`, not panic |
| `truncated_log_returns_err_not_panic` | `truncated.log` | no panic at any truncation point |
| `chain_tampered_log_is_detected` | `chain_tampered.log` | verdict is not "verified" (V4 CRC catches as "tampered_structural") |

### `crates/valori-state/Cargo.toml`

Added dev-dependencies: `tempfile`, `valori-verify`, `toml`.

## Findings

1. **`chain_head` is nested** — `verify_log_file` returns `report["replay"]["chain_head"]`, not `report["chain_head"]`. Test code must use the nested path.

2. **V4 per-entry CRC catches byte flips before chain check** — flipping an arbitrary data byte triggers `Failure::Decode` → verdict "tampered_structural", not "tampered_chain". The test now asserts `verdict != "verified"` rather than checking a specific tamper variant.

3. **`EventLogWriter::open` appends to existing files** — the generator `#[ignore]` test must delete fixture files before writing, or a second run appends duplicate RecordIds and `recover_from_event_log` fails with `InvalidOperation` (next_id mismatch).

4. **`*.log` in `.gitignore`** — fixture `.log` files require `git add -f` to commit.

## Validation

```
cargo test -p valori-kernel -p valori-storage -p valori-state
```

All suites: **0 failures**. Counts:
- valori-kernel: 31 + 11 + 12 + 6 + 22 + 5 = 87 tests
- valori-storage: 12 + 8 + 4 + 4 + 11 = 39 tests
- valori-state: 11 + 3 + 5 + 23 + 2 = 44 tests (5 new in event_log_compat)

## Follow-ups

- **P5 benchmarks** — `bench_1m` and `bench_persistence` need CI tracking (currently manual)
- **P6 receipt corpus** — BLAKE3 receipt chain tests for the GraphRAG / memory path
- **P7 WAL validation** — `bad_magic` + `truncated` hardening tests for `WalReader` (currently only in event_log_compat for `EventLogReader`)
- **P8 CI** — wire all three fixture test suites into CI; add `cargo deny` check
- **Expand snapshot corpus** — V5 fixture (backward-compat decode); malformed snapshot hardening tests (bad magic, truncated, wrong schema version)
