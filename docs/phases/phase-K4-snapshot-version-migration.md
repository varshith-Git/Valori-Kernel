# Phase K4 — Snapshot Version Migration Tests

## Goal

Prove that every `schema_ver` 1–6 backward-compat branch in `decode_state` is actually exercised
and correct. The encoder always writes the current version (V7), so without this phase all five
historic cutover branches were reachable only at boot time from old files on disk — never by any
test run.

## Delivered

| File | What landed |
|---|---|
| `crates/valori-kernel/tests/snapshot_version_migration.rs` | 10 tests — hand-rolled legacy encoders, per-version decode assertions, a cross-version hash-stable reencode chain, and two decoder-hardening tests |

### Tests shipped

| Test | What it proves |
|---|---|
| `v1_decodes_correctly` | V1 buffer: no tag, no metadata, no incoming-edge wire field, no namespace — defaults correct |
| `v2_decodes_correctly` | V2 buffer: metadata byte added; tag still absent |
| `v3_decodes_correctly` | V3 buffer: tag byte added |
| `v4_decodes_correctly` | V4 buffer: incoming-edge back-pointer on the wire |
| `v5_decodes_correctly` | V5 buffer: arithmetic-format byte present |
| `v6_decodes_correctly` | V6 buffer: `namespace_id` + `next_in_ns` + `prev_in_ns` fields present |
| `v1_hole_slot_decodes_as_absent_without_shifting_ids` | A V1 hole entry (id=u32::MAX sentinel) decodes as absent, does not shift subsequent record IDs |
| `cross_version_decode_reencode_chain_is_hash_stable` | For every V1–V7: decode → compare hash against `apply_event`-built reference → re-encode (current encoder) → decode again (lossless) → re-encode → decode again (fixed-point, zero drift) |
| `v6_out_of_range_namespace_head_is_rejected` | A V6 namespace-head byte ≥ 1024 (MAX_NAMESPACES) causes `decode_state` to return an error, not silently truncate |
| `schema_version_zero_is_rejected` | `schema_ver=0` (pre-V1 or corrupt header) is cleanly rejected |

## Findings

- **Real V1–V6 conditional branches were dead code under test.** `cargo-tarpaulin` post-K3 would
  have shown them as uncovered; nothing in the integration suite touched an old-format file.
- **Mutation test confirmed test sensitivity.** Temporarily disabling the V1–V3 incoming-edge
  reconstruction block in `decode.rs` caused exactly 4 failures (`v1`, `v2`, `v3`,
  `cross_version_decode_reencode_chain_is_hash_stable`); `v4`/`v5`/`v6` stayed green (they have
  the explicit wire path). Reverted; `git diff src/` is empty.
- **Hand-rolled encoder discipline.** Each legacy encoder in the test file mirrors `encode_state`'s
  exact field layout, trimmed per-version to what `decode_state` actually reads. Verified
  byte-for-byte against both source files, not guessed.

## Validation

```
cargo test -p valori-kernel
# 148 passed, 0 failed (was 138 before K4)
```

All 10 new tests pass. No regressions in the existing 138 kernel tests.

## Follow-ups

- **P5** — Benchmark suite (insert throughput, search latency p50/p95/p99, snapshot encode/decode,
  replay events/sec, BruteForce vs BQ recall+latency, memory bytes/vector).
- **P6** — Receipt system (`InsertReceipt { old_root, new_root, proof, sequence, timestamp,
  state_hash }`).
- **P7** — WAL validation tests (partial records, checksum mismatch, truncated WAL,
  duplicate/out-of-order sequence).
- **P8** — CI hardening (separate jobs, coverage reporting, clippy, fmt, miri).
