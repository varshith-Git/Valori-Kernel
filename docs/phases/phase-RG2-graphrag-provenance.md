# RG2 — GraphRAG provenance

## Goal

Make GraphRAG hits explain where their retrieved content came from and which
bounded graph evidence supports graph-expanded results. Preserve existing
`memory_id` compatibility and keep standalone and Raft behavior aligned.

## Delivered

- `crates/valori-node/src/capabilities.rs`
  - GraphRAG now resolves metadata from `record:<id>` first, then `rec:<id>`.
    This connects document-ingest metadata to retrieval responses without
    breaking the stable `rec:<id>` memory identity.
  - Each hit now includes `provenance`: resolved metadata key, source/chunk
    fields when present, graph distance, and a shortest graph evidence path
    inside the already-returned bounded subgraph.
  - Standalone and Raft GraphRAG use the same response-shaping logic.
- `crates/valori-node/src/api.rs`
  - Documented the new `GraphRagHit.provenance` response field.
- `crates/valori-node/tests/api_graphrag.rs`
  - Added HTTP regression coverage proving `record:<id>` metadata is returned
    by GraphRAG and copied into the provenance block.
- `crates/valori-node/tests/planner_parity.rs`
  - Extended the standalone/Raft GraphRAG parity case to require identical
    source metadata and two-hop graph evidence paths.
- `crates/valori-node/README.md`, `README.md`, `python/valoricore/remote.py`,
  `CHANGELOG.md`
  - Updated GraphRAG docs and SDK docstrings for the provenance response.

## Findings

- The main provenance bug was a key mismatch: ingest stores rich chunk/source
  metadata under `record:<id>`, but GraphRAG only read `rec:<id>`, so ingested
  document hits could return `metadata: null`.
- The evidence path is intentionally reconstructed from the returned bounded
  subgraph. If `depth`, `max_nodes`, or `max_edges` prevents an edge from being
  returned, the hit cannot claim that edge in its provenance path.
- This phase does not make metadata reads snapshot-atomic with the Raft graph
  traversal. It keeps the existing Raft pattern: traverse state in one closure,
  then fetch metadata asynchronously in sorted result order.

## Validation

- `cargo test -j 1 -p valori-node --test api_graphrag graphrag_resolves_record_metadata_and_returns_provenance -- --nocapture`: 1 passed.
- `cargo test -j 1 -p valori-node --test planner_parity graph_rag_parent_sibling_and_multiple_seed_reachability_match -- --nocapture`: 1 passed.
- `cargo test -j 1 -p valori-kernel -p valori-node --no-fail-fast`: 637 passed, 0 failed, 2 ignored.
- `rustfmt --edition 2024 crates/valori-node/src/capabilities.rs crates/valori-node/src/api.rs crates/valori-node/tests/api_graphrag.rs crates/valori-node/tests/planner_parity.rs`: passed.
- `python -m py_compile python/valoricore/remote.py`: passed.

## Follow-ups

- RG3 owns candidate scoring: remove the current seed bonus distortion, score
  graph-expanded candidates against the query, and rank the union before
  truncation.
- RG4 owns configurable traversal semantics: typed edge policy, direction
  policy, graph freshness filters, and bounded work accounting.
- RG5 owns extraction quality: canonical entity identity, typed relation
  predicates, source spans, confidence, and replayable extraction receipts.
