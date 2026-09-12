# RG3 — GraphRAG candidate scoring

## Goal

Make vector seeds and graph-expanded records compete in one ranking space before
`final_k` truncation. Graph reachability should admit candidates and provide a
bounded evidence-path boost, but candidate relevance must still be measured
against the original query.

## Delivered

- `crates/valori-node/src/capabilities.rs`
  - Added query-distance scoring for graph-expanded records with usable vectors
    in both standalone and Raft GraphRAG.
  - Removed the old seed-status graph bonus. A vector hit no longer receives
    `graph_score = 1.0` merely because it has a graph node.
  - Replaced the old linear blend with a capped evidence boost:
    `final_score = semantic_rel + graph_weight * graph_rel * (1 - semantic_rel)`.
  - Kept scores bounded to `[0, 1]`, with semantic relevance as the base score
    and graph relevance only for graph-only candidates reached through the
    returned subgraph.
  - Added deterministic tie-breakers: `final_score` descending, semantic
    relevance descending, `graph_distance` ascending, then `record_id`
    ascending.
- `crates/valori-node/tests/api_graphrag.rs`
  - Added a regression test proving a graph-discovered candidate is scored
    against the query before `final_k`, receives a bounded path boost, and can
    displace a slightly stronger pure-vector hit.
  - Added a regression test proving a weak graph neighbor does not outrank a
    stronger vector hit merely because it is reachable.
  - Updated previous score-shape expectations for RG3: graph-only records with
    vectors now carry `score`/`vector_score`; seeds no longer receive a graph
    bonus by default.
- `crates/valori-node/src/api.rs`, `crates/valori-node/README.md`,
  `README.md`, `python/valoricore/remote.py`, `CHANGELOG.md`
  - Updated GraphRAG scoring docs and SDK docstrings.

## Findings

- The historical scoring bug was real: the old formula gave every vector hit
  with a graph node maximum graph relevance at distance zero, even if traversal
  added no useful evidence.
- Exact KNN means a graph candidate that is strictly more semantically relevant
  than all vector seeds would already be in the vector top-k. The practical RG3
  quality win is admitting graph candidates outside `retrieval_k`, scoring them
  against the query, and allowing a bounded evidence-path boost to move them
  into `final_k` when they are close enough.
- `graph_weight=1.0` is no longer "pure graph ranking"; it is the maximum
  bounded path boost on top of semantic relevance. This is safer because an
  exact semantic match cannot be pushed below weak graph evidence.

## Validation

- `cargo test -j 1 -p valori-node --test api_graphrag -- --nocapture`: 22 passed.
- `cargo test -j 1 -p valori-node --test planner_parity -- --nocapture`: 5 passed.
- `python -m py_compile python/valoricore/remote.py`: passed.
- `cargo test -j 1 -p valori-kernel -p valori-node --no-fail-fast`: 639 passed, 0 failed, 2 ignored.
- `rustfmt --edition 2024 crates/valori-node/src/capabilities.rs crates/valori-node/src/api.rs crates/valori-node/tests/api_graphrag.rs crates/valori-node/tests/planner_parity.rs`: passed.

## Follow-ups

- RG4 owns traversal semantics: request-level edge policies, direction policies,
  graph freshness filters, and richer path selection.
- RG5 owns extraction quality: canonical entity identity, typed relation
  predicates, source spans, confidence, and replayable extraction receipts.
- Future benchmark work should rerun the public retrieval comparison after RG3
  because graph discoveries now have real query-distance scores rather than
  path-only scores.
