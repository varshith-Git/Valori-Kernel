# Phase S17 — CRTS/BCRP snapshot roundtrip tests

## Goal

Add coverage for the two Engine trailing snapshot sections (CRTS and BCRP) that were
added in a prior session but had no roundtrip tests, meaning a regression in decode
would be invisible until decay or reranking silently broke at runtime.

## Delivered

### `crates/valori-node/tests/engine_snapshot_roundtrip.rs` (new)

Five tests:

| Test | What it verifies |
|---|---|
| `crts_timestamps_survive_roundtrip` | `created_at` map is identical before and after snapshot/restore |
| `crts_absent_in_old_snapshot_does_not_panic` | Snapshot truncated after NSRG (no CRTS) restores successfully; `created_at` is empty |
| `bcrp_corpus_survives_roundtrip` | Reranker corpus entry count matches; a term query against a known token returns the right record after restore |
| `bcrp_absent_in_old_snapshot_does_not_panic` | Snapshot truncated at BCRP tag restores successfully; corpus is empty |
| `crts_and_bcrp_both_survive_roundtrip` | Both sections survive in one shot |

### `crates/valori-node/src/engine.rs` (additions)

- `Engine::reranker_corpus_len() -> usize` — delegates to `reranker.corpus_len()`.
- `Engine::reranker_rerank(query_text, query_vec, candidates) -> Vec<(u32, f32)>` —
  thin wrapper converting u32↔u64 IDs for tests.

### `crates/valori-node/src/valori_reranker.rs` (addition)

- `ValoriReranker::corpus_len() -> usize` — `self.corpus.len()`.

## Findings

- `Engine::record_created_at()` already existed; the test uses it directly. No rename needed.
- CRTS encodes `HashMap<u32, u64>` via bincode; decode is infallible (silently skips
  on malformed bytes) — the "absent" tests confirm this guarantees forward compat.
- BCRP stores `(HashMap<u64, Vec<String>>, usize)` — corpus + total_tokens for avgdl.
  The total_tokens field is essential for BM25 avgdl to be correct after restore.
- The `bcrp_corpus_survives_roundtrip` test checks both count AND semantic correctness
  (right record ranks first for its terms), not just byte presence.

## Validation

- `cargo test -p valori-node --test engine_snapshot_roundtrip`: **5 passed, 0 failed**
- `cargo test -p valori-node --test persistence_tests`: **1 passed, 0 failed**
- `cargo test -p valori-node --test api_keys`: **8 passed, 0 failed**
- `cargo test -p valori-node --test cluster_namespaces`: **16 passed, 0 failed**

## Follow-ups

None. The V7 raw-f32 note in the user prompt referred to the kernel's V7 `meta` sidecar
(`KernelState.meta`) which already has a roundtrip test in
`crates/valori-kernel/tests/snapshot_roundtrip.rs::v7_meta_roundtrips`. No further kernel
snapshot work needed.
