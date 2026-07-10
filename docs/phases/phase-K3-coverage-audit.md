# Phase K3 — Coverage audit

## Goal
Establish a baseline coverage measurement using `cargo-tarpaulin` and identify
modules with 0%, low (<50%), or missing test coverage that represent real risk.

## Delivered

Coverage tool installed: `cargo-tarpaulin 0.37.0`

Command run:
```bash
cargo tarpaulin -p valori-kernel --features std --out Stdout
```

**Overall kernel coverage: 36.24% (963/2657 lines)**

## Findings

### Zero coverage (0%) — highest risk

| Module | Lines | Risk |
|---|---|---|
| `kernel/src/hnsw.rs` | 0/265 | HIGH — 265 lines of HNSW implementation completely untested; any correctness issue is invisible |
| `kernel/src/proof.rs` | 0/24 | HIGH — BLAKE3 receipt / proof construction path; this is a key differentiator |
| `kernel/src/fxp/ops.rs` | 0/21 | HIGH — Fixed-point arithmetic ops; bugs here break determinism silently |
| `kernel/src/types/mod.rs` | 0/48 | MEDIUM — InsertPayload, CMD constants, old type aliases; some may be dead |
| `kernel/src/adapters/ivecs.rs` | 0/11 | MEDIUM — ivecs binary format reader |
| `kernel/src/verify.rs` | 0/4 | MEDIUM — chain verification entry point |

### Low coverage (<60%)

| Module | Lines | Coverage | Gap |
|---|---|---|---|
| `kernel/src/event.rs` | 56/162 | 35% | Serialization round-trip only tests ~⅓ of event variants; AutoCreate* and namespace lifecycle events have no direct tests |
| `kernel/src/types/vector.rs` | 7/21 | 33% | `FxpVector` from_f32, arithmetic helpers untested |
| `kernel/src/graph/adjacency.rs` | 11/24 | 46% | Remove/iteration paths untested |
| `kernel/src/state/kernel.rs` | 207/405 | 51% | Most uncovered lines are in the HNSW/BQ code paths, encrypted-record paths, and cascade delete |

### Decent coverage (>75%)

| Module | Lines | Coverage |
|---|---|---|
| `kernel/src/index/bq.rs` | 83/109 | 76% |
| `kernel/src/snapshot/decode.rs` | 185/211 | 88% |
| `kernel/src/snapshot/encode.rs` | 91/95 | 96% |
| `kernel/src/snapshot/blake3.rs` | 39/41 | 95% |
| `kernel/src/math/l2.rs` | 37/42 | 88% |
| `kernel/src/fxp/format.rs` | 12/12 | 100% |

### Non-kernel zeroes (expected — no integration harness)

`valori-consensus`, `valori-node` (HTTP handlers), `valori-storage` (event log),
`valori-mcp`, `embedded/` — all zero because these require a running server or
Raft cluster. Not actionable with unit tests alone; covered by the integration
test suite (Python client tests, `crash_durability.rs`).

## Priority coverage gaps for next phase (K4)

Severity-ranked:

1. **`fxp/ops.rs` (0/21)** — Fixed-point ops are load-bearing for determinism.
   A single shift or saturation bug changes every state hash silently. Tests
   should cover: `add`, `sub`, `mul_fxp`, `div_fxp`, `saturating_*`,
   overflow behaviour, negative values.

2. **`proof.rs` (0/24)** — The BLAKE3 receipt construction is the key
   verifiability primitive. Tests should: build a receipt after inserts,
   verify the old_root → new_root chain, check proof bytes are non-empty.

3. **`verify.rs` (0/4)** — Chain verification entry point. Add at least a
   smoke test: valid chain passes, tampered chain fails.

4. **`event.rs` (35%)** — Add roundtrip serde tests for all Auto* variants and
   the namespace lifecycle events (AutoCreateNamespace, DropNamespace,
   AutoInsertRecordEncrypted).

5. **`graph/adjacency.rs` (46%)** — Add tests for `remove_neighbor`,
   `remove_all`, and iteration over an empty adjacency list.

6. **`state/kernel.rs` (51%)** — Gaps are primarily: `DeleteRecord` hard-delete
   path, `DropNamespace` cascade, `InsertRecordEncrypted` apply path,
   `ShredKey` flag propagation. Add targeted unit tests for each.

7. **`types/mod.rs` (0/48)** — Audit first: many lines may be dead code from
   the `ValoriKernel` era. Delete dead code before writing tests.

8. **`hnsw.rs` (0/265)** — Low priority until the HNSW index is wired into
   the production `ActiveIndex` enum. For now, just ensure it compiles.

## Validation

- `cargo-tarpaulin 0.37.0` installed
- Run: `cargo tarpaulin -p valori-kernel --features std`
- Baseline established: 36.24% (963/2657 lines) — 2026-07-10
