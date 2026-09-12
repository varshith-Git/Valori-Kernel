# RG4 — GraphRAG traversal semantics

## Goal

Make GraphRAG traversal configurable without changing the default retrieval
behavior. Users should be able to prove when graph relations help by running the
same vectors with different graph edge policies.

## Delivered

| File | Delivered |
|---|---|
| `crates/valori-rag/src/reachability.rs` | Added `TraversalPolicy` and `expand_retrieval_subgraph_with_policy`; default policy preserves all-edge traversal plus reverse incoming `ParentOf`; added unit coverage for edge-kind filtering and disabling reverse parent traversal. |
| `crates/valori-effect/src/capability.rs` | Extended `KernelCapability::graph_rag` with `edge_kinds` and `reverse_parent_of`; corrected scoring docs to the RG3 capped evidence boost. |
| `crates/valori-effect/src/tasks/graph_rag.rs` | Added RG4 fields to GraphRAG task inputs and passed them through to capabilities. |
| `crates/valori-node/src/server.rs` | Added `edge_kinds` and `reverse_parent_of` request fields on standalone `POST /v1/graphrag`. |
| `crates/valori-node/src/cluster_server.rs` | Added the same request fields on cluster `POST /v1/graphrag`. |
| `crates/valori-node/src/capabilities.rs` | Applied the traversal policy in both `EngineKernelCapability` and `RaftKernelCapability`. |
| `python/valoricore/remote.py` | Added `edge_kinds` and `reverse_parent_of` to direct and HA sync/async GraphRAG clients. |
| `crates/valori-node/tests/api_graphrag.rs` | Added HTTP regression proving the same vectors retrieve different graph candidates when `edge_kinds` filters traversal. |
| `README.md`, `crates/valori-node/README.md`, `crates/valori-rag/README.md`, `crates/valori-effect/README.md`, `python/valoricore_readme.md`, `CHANGELOG.md` | Documented the RG4 API and defaults. |

## Findings

- The old GraphRAG traversal was one-size-fits-all. That was useful for demos,
  but not enough for production retrieval where `Contradicts`, `Supersedes`,
  citation, and generic relation edges may need different policies.
- The default had to remain permissive for backward compatibility; explicit
  policy fields are now the safer way to tune retrieval.

## Validation

- `cargo test -j 1 -p valori-rag reachability -- --nocapture` — 5 passed.
- `cargo test -j 1 -p valori-node --test api_graphrag -- --nocapture` — 24 passed.
- `cargo test -j 1 -p valori-node --test planner_parity -- --nocapture` — 5 passed.
- `cargo test -j 1 -p valori-kernel -p valori-node --no-fail-fast` — 641 passed, 0 failed, 2 ignored.

## Follow-ups

- RG6 should add weighted traversal policies per edge kind, not just allow/deny.
- Extraction should eventually classify support/contradiction/supersedes edges
  automatically so `edge_kinds` maps to richer product semantics.
