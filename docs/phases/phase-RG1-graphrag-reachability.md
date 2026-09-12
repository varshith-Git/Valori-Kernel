# RG1 — GraphRAG reachability

## Goal

Make existing document structure and all record-to-node relationships reachable
from vector hits in both standalone and cluster GraphRAG. This is the first step
in reachability → provenance → candidate scoring → traversal semantics →
extraction quality, not completion of the entire sequence.

## Delivered

- `crates/valori-rag/src/reachability.rs`: namespace-scoped all-node seed
  resolution; outgoing traversal plus incoming `ParentOf` traversal; one walk
  returns nodes, edges, and shortest discovered hop distances. Node budgets
  include seeds; edge budgets count examined adjacency entries across the walk.
  Every emitted edge has both endpoints in the returned subgraph. Four unit
  regressions cover parent/sibling discovery, directed semantic links, depth,
  budgets, namespace checks and seed-order determinism.
- `crates/valori-rag/src/lib.rs`: export the new module. Existing generic graph
  expansion APIs remain outgoing-only.
- `crates/valori-node/src/capabilities.rs`: wire the same primitive into
  `EngineKernelCapability` and `RaftKernelCapability`; remove the second BFS
  that previously ignored node/edge budgets. Preserve representative `node_id`
  on vector hits while using all referencing nodes as expansion seeds.
- `crates/valori-node/tests/planner_parity.rs`: one new HTTP regression creates
  two nodes for the seed record, attaches the useful link to the second, and
  verifies parent/sibling retrieval and node-budget exclusion in both modes.
  Compare full standalone and Raft GraphRAG responses.
- `crates/valori-node/tests/api_graphrag.rs`: update the old single-seed contract
  assertion to require both expansion seeds while retaining the lowest-ID
  representative on the hit.
- `crates/valori-node/src/server.rs`, `cluster_server.rs`, and
  `python/valoricore/remote.py`: describe the tightened edge budget semantics.
- `crates/valori-rag/README.md`, `crates/valori-node/README.md`, main `README.md`,
  phase index and `CHANGELOG.md`: document the changed reachability contract.

No kernel mutations, storage formats, endpoints, SDK signatures, or ranking
formula changed. Existing unrelated working-tree changes were preserved.

## Findings

- A document's outgoing `ParentOf` links were unreachable from a chunk seed.
  Depth two now permits chunk → parent → sibling; depth one only reaches the
  parent. Semantic relations such as `RefersTo` are not reversed.
- Choosing only one node per record omitted relationships attached to another
  node. All eligible referencing nodes now seed expansion, ordered by node ID.
- Budgeting expansion and then independently walking for distances could
  bypass work limits and describe paths outside the returned traversal.
- `max_edges` now counts examined adjacency entries, including incoming entries
  that cannot be followed. This can yield fewer returned edges than the budget;
  absent limits remain unbounded, with the existing depth cap of four.
- Candidate scoring still favors vector seeds. Reaching a sibling does not
  guarantee inclusion under a tight `final_k`; this phase makes no quality
  uplift claim and does not repair the previous benchmark protocol.

## Validation

- `cargo test -p valori-rag --lib`: **50 passed, 0 failed, 3 ignored**.
- `cargo test -j 1 -p valori-kernel -p valori-node --no-fail-fast`:
  **635 passed, 1 failed, 2 ignored**. The only failure was the old HTTP test
  requiring one expansion seed for a multi-node record. Updated that assertion
  to the new contract without removing its representative-node check.
- After that test-only correction,
  `cargo test -j 1 -p valori-node --test api_graphrag --test planner_parity`:
  **19/19 GraphRAG tests and 5/5 parity tests passed**. The full suite was not
  repeated after this assertion update; the other suites passed in the full run.
- Initial parallel post-change compilation exhausted the Windows paging file
  (`os error 1455`) before tests ran. Retried with `-j 1`; no machine settings
  changed.
- `git diff --check`: passed for the current working tree.
- Targeted `rustfmt --check` and Python SDK syntax compilation: passed.
- HTTP regression uses in-process routers and a self-elected Raft node; this
  verifies the cluster execution path, not a three-machine deployment.
- No live benchmark, public-site deployment, or extraction model run performed.

## Follow-ups

- **RG2 — Provenance:** unify `record:`/`rec:` metadata resolution; return source
  text and traceable evidence paths, and bind provenance to source versions.
- **RG3 — Candidate scoring:** remove seed-only graph bonuses; score graph
  discoveries against the query and rerank before final truncation.
- **RG4 — Traversal semantics:** query-dependent edge filters, path templates,
  candidate diversity and explicit truncation reporting. RG1 only adds the
  structural reverse-parent rule needed for reachability.
- **RG5 — Extraction quality:** source-grounded typed assertions, persistent
  entity identity, safe namespace handling, and true contradiction verification.
- **Benchmark follow-up:** remove gold-answer graph leakage and equalize eligible
  result budgets before claiming retrieval superiority or updating public claims.
