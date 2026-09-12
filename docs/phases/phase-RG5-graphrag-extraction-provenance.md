# RG5 — GraphRAG extraction provenance

## Goal

Make automatically extracted graph evidence auditable and reusable. If Valori
creates entity nodes and relationship edges from text, later retrieval should be
able to show where that graph evidence came from.

## Delivered

| File | Delivered |
|---|---|
| `crates/valori-rag/src/community.rs` | Added optional `source` on `ExtractEntitiesRequest`; added `strength` on `InsertedRelationship`. |
| `crates/valori-node/src/server.rs` | Standalone extraction now stores metadata for each generated entity record, entity node, and relationship edge: kind, source, BLAKE3 `source_text_hash`, collection, IDs, and relationship strength. |
| `crates/valori-node/src/cluster_server.rs` | Cluster extraction writes the same metadata through Raft `KernelEvent::SetMeta`, preserving audit-chain semantics. |
| `crates/valori-node/tests/api_graphrag.rs` | Added a live in-process mock LLM/embedder test proving extraction returns relationship strength and persists queryable record/edge metadata. |
| `python/valoricore/remote.py` | Added `source` to sync and async `extract_entities(...)`. |
| `README.md`, `crates/valori-node/README.md`, `crates/valori-rag/README.md`, `crates/valori-effect/README.md`, `python/valoricore_readme.md`, `CHANGELOG.md` | Documented auditable extraction metadata and its relationship to GraphRAG evidence. |

## Findings

- The biggest remaining quality gap is still extraction correctness: RG5 stores
  provenance for extracted graph facts, but it does not yet guarantee the LLM
  extracted the right entities or relation types.
- Relationship strength is preserved, but traversal scoring still treats all
  allowed edges of the same hop distance uniformly. That belongs in a future
  weighted traversal phase.

## Validation

- `cargo test -j 1 -p valori-rag reachability -- --nocapture` — 5 passed.
- `cargo test -j 1 -p valori-node --test api_graphrag -- --nocapture` — 24 passed.
- `cargo test -j 1 -p valori-node --test planner_parity -- --nocapture` — 5 passed.
- `cargo test -j 1 -p valori-kernel -p valori-node --no-fail-fast` — 641 passed, 0 failed, 2 ignored.

## Follow-ups

- RG6: weight graph traversal by edge kind and relationship strength.
- RG7: replace generic extracted `Relation` edges with typed
  support/contradiction/supersedes/citation edges and confidence thresholds.
- RG8: add evaluator benchmarks that compare extracted graphs against
  human-labeled claim-evidence relations.
