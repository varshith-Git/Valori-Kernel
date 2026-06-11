# Phase 1.1b — Per-crate test layout + the kernel bugs it caught

**Status:** done · commit `1db62c9` on `multinode`
**Roadmap:** not a numbered roadmap item — user-directed insertion:
every crate follows the standard `src/ + tests/ + README.md` layout

## Goal

Give every crate a real `tests/` directory and a README. The kernel had
**zero running unit tests**: its `src/tests/` directory (11 modules) was
never declared in `lib.rs` and targeted an API deleted long ago
(~100 compile errors when wired up). Replaced wholesale with fresh
integration tests against the current public API.

## Delivered

| Crate | tests/ | README |
|---|---|---|
| valori-kernel | NEW: `state_machine`, `determinism`, `snapshot_roundtrip`, `search` (22 tests); stale `src/tests/` deleted | moved `src/README.md` → crate root |
| valori-node | already had 17 test files | `API_README.md` → `README.md` |
| valori-verify | NEW: `wire_format.rs` incl. the cross-crate node-writes/verify-reads contract test | existing |
| valori-cli | existing | existing |
| valori-consensus | placeholder test (crate links) | NEW — Phase 2 scope |
| valori-ffi | via `pip install` + pytest (host cargo test impossible) | existing |
| embedded | n/a (firmware, `test = false`) | NEW — cross-compile instructions |

Junk removed: stray per-crate `Cargo.lock`s, `build_err*.txt`, leftover
test logs/databases.

## Findings — three real kernel bugs, all pre-existing

1. **The state hash ignored `tag` and `metadata`.** Tags drive filtered
   search: two replicas could serve different search results while
   presenting identical BLAKE3 state roots. Metadata carries the
   per-record proofs the README explicitly claims are "included in the
   state root" — they were not. Both now hashed (length-prefixed,
   `None` = u32::MAX sentinel). ⚠️ **Every state hash changed at this
   commit** (pre-1.0 break, accepted and documented).
2. **Rejected events mutated state.** `apply()` inserted into the pool
   first and validated the claimed ID afterwards, with no rollback — a
   rejected `InsertRecord`/`CreateNode` left a phantom object behind.
   IDs are now pre-validated before any mutation; a test asserts a
   rejected event leaves the state hash bit-identical. Critical for
   Phase 2, where Raft applies committed entries to the kernel directly.
3. **`node_count()`/`edge_count()` counted tombstones** — never
   decreased after deletes (and silently consumed HTTP-507 capacity
   forever). Pools gained `live_count()`; `len()` keeps slot semantics
   because ID allocation depends on it.

Plus one flaky test fixed: `test_chain_head_deterministic` only passed
when two logs were written within the same wall-clock second.

## Validation

- **149 tests passing across 49 binaries, 0 failures** (up from 120).
- The cross-crate wire test (`wire_format.rs`) is the test that would
  have caught both historical format drifts before they shipped.

## Follow-ups

- State-hash change means externally stored hashes from older builds no
  longer match — formalized hash-domain versioning arrives with
  Phase 1.3.
