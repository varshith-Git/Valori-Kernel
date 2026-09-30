# Determinism rules

**Source of truth:** [`INVARIANTS.md`](../../INVARIANTS.md) at the repo root — 15 numbered, code-referenced
invariants (I-01 through I-15), each tagged with the crates it governs. Read the ones tagged for the
crate you're touching before writing code, not after. This file does not restate them — it's the
checklist for *when* to go re-read which one, plus the concrete verified rules from the code itself.

## Verified rules (from `crates/valori-kernel` and `.agent/rules/valori-rules.md`'s constraints, cross-checked
against current code)

- **No `f32`/`f64` in the kernel hot path.** Vector math uses `FxpScalar`, Q16.16 fixed-point
  (`crates/valori-kernel/src/fxp/`). `f32` conversions exist only in test helpers / FFI boundary code,
  never inside `apply_event_ns()` or `search_l2_ns()`.
- **No heap allocation inside `valori-kernel`'s core paths** — static pools with const-generic capacities
  (`RecordPool<MAX_RECORDS, D>`, `NodePool<MAX_NODES>`, `EdgePool<MAX_EDGES>`), not `Vec`/`Box`/`HashMap`.
- **No `HashMap`/`HashSet` iteration order feeding deterministic output**, anywhere in `valori-kernel` or
  `valori-consensus`'s `ValoriStateMachine::apply()`. Use index-ordered scans or explicit sort keys.
- **Deterministic tie-breaking**: search results with equal scores break ties by `RecordId` (or the
  equivalent stable field) — never by insertion-order coincidence or hash-map iteration.
- **No wall-clock time in hashed/replayed state.** `engine.rs` explicitly holds `created_at` as
  "derived, non-hashed" for decay — that pattern (timestamp present but excluded from the hash) is the
  model to follow if you need time-adjacent behavior; don't fold a `SystemTime::now()` into anything that
  feeds `state_hash`.
- **`valori-kernel` MUST remain `no_std`** (`AGENTS.md` invariant 7) — no `use std::` inside
  `crates/valori-kernel/src/`, new deps need `default-features = false`. Verify with:
  ```
  cargo build -p valori-kernel --target wasm32-unknown-unknown
  ```

## Mandatory review checklist — when touching any of these

| You're touching | Re-read | Then run |
|---|---|---|
| Index algorithms (`valori-index`, `valori-search`) | `INVARIANTS.md` I-03 | `cargo test -p valori-kernel`, relevant `benchmarks/*.py` |
| Graph traversal / adjacency | `INVARIANTS.md` I-03, I-04 | `cargo test -p valori-kernel` |
| WAL / event log (`valori-storage`) | `INVARIANTS.md` I-04, I-08, I-11, I-15 | `cargo test -p valori-node` (WAL replay tests), `cargo miri test -p valori-kernel --test proof` |
| Snapshot encode/decode (`valori-kernel/src/snapshot/`) | `INVARIANTS.md` I-12, I-15; `docs/SNAPSHOT_FORMAT.md` | `cargo test -p valori-kernel`; bump the version constant + update `tests/format.rs` per `AGENTS.md`'s "Where to add things" table |
| Hashing / state hash / receipts | `INVARIANTS.md` I-02, I-09, I-10 | `cargo miri test -p valori-kernel --test proof` |
| Replay / recovery | `INVARIANTS.md` I-11 | `cargo test -p valori-node` |
| Ranking / scoring | tie-breaking rule above | `cargo test -p valori-kernel` |
| Anything in `crates/valori-kernel/src/fxp/` | this file's fixed-point rule | `cargo miri test -p valori-kernel --test fxp` |
| Cross-machine behavior | — | `.github/workflows/multi-arch-determinism.yml` runs on CI; don't skip it locally if you touched arithmetic |

`cargo miri test -p valori-kernel --test fxp` and `--test proof` are real, CI-enforced (`ci.yml`'s `miri`
job) — they exist specifically because UB in fixed-point arithmetic or the Merkle/receipt path is a
determinism bug, not just a correctness bug. Run them for anything in this table, not just on CI.

## What "deterministic replay" means here, concretely

Per `INVARIANTS.md` I-11: `KernelState` has exactly one reconstruction path — snapshot (or empty) +
replay every `KernelEvent` since. If your change makes a second path *seem* to work (e.g. reading a
partial WAL segment directly), that's the bug, not a feature — the mandatory single-node/cluster test
matrix in `.claude/rules/testing.md` exists to catch this class of regression.
