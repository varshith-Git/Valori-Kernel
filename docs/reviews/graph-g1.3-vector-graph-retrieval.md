# G1.3 — Vector → Graph Retrieval

*Follow-up to [`graph-g1.2-traversal-performance.md`](graph-g1.2-traversal-performance.md). Establishes the deterministic vector→graph bridge contract, and fixes two proven defects found in it.*

---

## 1. Objective

Pin down how vector similarity enters the canonical graph — without making vectors and graph nodes the same thing. G1.0's optional-linkage model (Model B) must survive: a vector insert never requires a node; a node never requires a vector.

---

## 2. Existing Implementation Audit

The bridge already existed end-to-end. Traced from HTTP down:

```
POST /v1/graphrag
  → run_graph_inline → GraphRagTask (valori-effect/src/tasks/graph_rag.rs)
  → ctx.capabilities.kernel.graph_rag(shard, ns, vector, k, depth)
      ├── standalone: EngineKernelCapability   (valori-node/src/capabilities.rs:158)
      └── cluster:    RaftKernelCapability     (valori-node/src/capabilities.rs:834)
  → namespace-scoped vector KNN  (search_l2_ns)
  → RecordId[]
  → record → node resolution
  → seed NodeId[]
  → expand_subgraph BFS (valori-rag/src/graph.rs)
  → { hits, seed_nodes, subgraph:{nodes,edges} }
```

**Linkage cardinality (verified, not assumed)**: `KernelEvent::CreateNode` (`valori-kernel/src/state/kernel.rs:382-395`) validates only that a referenced record *exists* and shares the node's namespace. It imposes **no uniqueness on `record`** — so N nodes may legitimately point at one record. `GraphNode.record` is `Option<RecordId>`; `Record` carries no back-pointer. The relationship is therefore **optional and many-nodes-to-one-record**, one-directional.

**The defect**: the two paths resolved record→node by *different rules*.

| Path | Mechanism | Rule |
|---|---|---|
| Standalone | `eng.record_to_node` — a `HashMap<u32,u32>` cache on `Engine` | last-write-wins (`insert` overwrites) |
| Cluster | `valori_rag::graph::resolve_seed_nodes` — derived from canonical state per call | first-in-pool-order wins (`entry().or_insert()`, i.e. lowest node id) |

---

## 3. Two Proven Defects

Both were demonstrated empirically before any code changed, and each is now pinned by a test that fails without the fix (verified by temporary revert).

**Defect 1 — standalone/cluster parity.** One record with two nodes (`node 0`, `node 1`): standalone `/v1/graphrag` reported `node_id: 1` and seeded from it; cluster reported `node_id: 0`. Identical canonical state, identical query, **different seed → potentially different subgraph**. That violates the dual-path parity principle and the determinism contract.

**Defect 2 — cache staleness across restart.** `record_to_node` is maintained incrementally: `post_apply_derived` inserts on `CreateNode`, and `apply_committed_event{,_ns}` calls `record_to_node.remove(&rid)` on `DeleteNode`. That removal is unconditional — deleting one of two nodes on a record **removes the mapping entirely**, even though the sibling node still points at the record. Measured: after deleting node 0, the cache returned `None` while canonical state clearly still had node 1. Because `try_recover()` calls `rebuild_record_to_node()`, a restart *repaired* the map — so **the same query returned different results before vs. after a restart**, directly violating G1.3's own restart-equivalence exit criterion.

---

## 4. Decision — Option B (small targeted fix)

The bridge's shape was right; its seed resolution was not. One-line-scale change in `crates/valori-node/src/capabilities.rs`: the standalone `graph_rag` now calls `resolve_seed_nodes(&eng.state, &record_ids)` — the same function the cluster path already used — instead of reading the `record_to_node` cache.

This fixes both defects at once and for a structural reason, not by patching symptoms:
- **Parity** becomes true *by construction* — both paths now execute literally the same function, matching the principle `valori-rag/src/graph.rs`'s own module doc already states for traversal ("identical by construction, not by copy-paste").
- **Staleness becomes impossible** — the resolution is stateless, derived fresh from canonical state on every call. There is no cache to drift.
- **Restart equivalence follows** — nothing survives a call to be stale.

No canonical state, event, snapshot, hash, vector-index, or API-surface change. `record_to_node` still exists and is still used by the record-delete cascade (`soft_delete_record`/`delete_record`) — deliberately untouched, see §11.

---

## 5. Semantics (now explicit)

- **Seed rule**: for each vector hit's `RecordId`, the seed is the **live node with the lowest `NodeId` whose `record` field equals it**. Deterministic: `iter_nodes()` walks the node pool in ascending-slot order and `entry().or_insert()` keeps the first.
- **Missing linkage is never an error.** A vector hit with no node yields `node_id: null`, contributes no seed, and **remains a full vector hit** in `hits[]`. A node with no record is valid and simply can never be seeded into.
- **Seed order** follows vector-hit order (itself deterministic by `(score, id)` from G0.1), filtered to hits that resolved.
- **Traversal**: `expand_subgraph` BFS, depth clamped to `MAX_DEPTH=4`, nodes/edges de-duplicated across seeds — unchanged from G1.1/G1.2, proven deterministic in G0.1 §9a.
- **Ranking**: unchanged. `hits[]` keeps pure vector ordering; graph expansion never reorders it. Hybrid ranking is explicitly G1.4's, not G1.3's — nothing was added here.

---

## 6. Namespace Isolation

Vector search is already namespace-scoped (`search_l2_ns`), so only in-namespace `RecordId`s ever reach seed resolution. `resolve_seed_nodes` itself carries no namespace filter — and does not need one: `CreateNode` enforces `node.namespace_id == record.namespace_id`, so any node whose `record` matches an in-namespace record **is itself in that namespace**. Airtight by construction, same reasoning pattern as G1.1.1 §3.

Tested explicitly with the collision scenario the phase asked for: two namespaces holding *byte-identical vectors*, so a namespace-blind search would happily cross over. Namespace A's search returns only A's record; seed resolution returns only A's node; both nodes' `namespace_id` are asserted distinct.

---

## 7. Determinism

Every stage is deterministic and no stage depends on hash-map iteration order for its *output*: vector KNN orders by `(score, id)`; seed resolution uses ascending-node-id pool order (the `HashMap` it returns is only ever probed by known key, never iterated for ordering); BFS ordering is G0.1-proven; the final `seeds` vector follows hit order. The G0.2 hash contract was not touched.

---

## 8. Missing-Link Matrix (all tested)

| Case | Behavior |
|---|---|
| Vector hit **with** node | seeds expansion |
| Vector hit **without** node | valid hit, `node_id: null`, no seed — not an error |
| Node **without** record | valid; never seeded into |
| **Multiple** nodes per record | lowest node id wins, deterministically |
| Deleted node (only one) | no seed; **record still a vector hit** |
| Deleted node (one of several) | surviving node still seeds — *the Defect-2 regression* |
| Deleted edge | seed unaffected; expansion shrinks |
| Soft-deleted record | drops out of vector results entirely, so never seeds |
| Namespace mismatch / ID collision | isolated, proven |
| Empty vector result / empty expansion | empty, not an error |
| Duplicate paths (diamond) | node emitted once |
| Cycles | terminate, dedupe |

---

## 9. Entity Extraction (audited, not modified)

`POST /v1/ingest/extract-entities` (`server.rs:3909`) calls an LLM, embeds entity descriptions, inserts them as records, then creates `Concept` nodes **linked to those records** and relationship edges. So it produces exactly the record↔node linkage this phase's bridge consumes — extracted entities are seedable by vector search like any other record.

Node/edge creation goes through normal canonical events, so the *resulting graph* replays deterministically. **The LLM call itself is not deterministic** — re-running extraction on the same text may produce a different graph. This is consistent with the project's existing stance (memory `determinism-via-logged-output`: determinism means replaying logged output, never re-invoking the LLM), and the canonical event log does record the resulting events. Not a G1.3 concern; not redesigned here.

---

## 10. Performance

The fix trades an O(1) cache lookup for an O(live_nodes) scan per call. Measured honestly (`resolve_seed_nodes_cost`, release):

| Live nodes | Cost per `graph_rag` call (k=10) |
|---|---|
| 1,000 | 1.1µs |
| 10,000 | 10.3µs |
| 100,000 | 173µs |

**Framing**: the old O(1) path was *incorrect*, so this is not a performance regression against a valid baseline — it is the cost of correctness. Two mitigating facts: the cluster path has always paid exactly this cost, so this aligns standalone with shipping behavior rather than introducing a new cost class; and 173µs at 100K nodes sits alongside a call that also does vector KNN, BFS, and per-hit metadata fetch.

**Deferred, with a stated trigger**: the scan is linear, so ~1M nodes would be ~1.7ms — enough to matter. If that scale becomes real, the fix is a *correctly maintained* record→node index (multi-valued, honoring lowest-id-wins, with delete handling that does not drop surviving siblings), or rebuild-on-demand. Per G1.2's standing conclusion, that is **not** justified by current measurements and was not built.

---

## 11. Deferred Finding — `record_to_node` in the delete cascade

`record_to_node` remains in use by `Engine::soft_delete_record`/`delete_record`, which consult it to cascade-delete a record's node. That cache retains the staleness bug described in §3 (Defect 2): if a record has several nodes and one is deleted, the entry is dropped, so a later record deletion will **not** cascade to the surviving node — leaving a live node pointing at a deleted record.

Not fixed here: it is a *mutation-path* defect, whereas G1.3's scope is vector→graph *retrieval*, and correcting it means changing delete-cascade semantics (which node should a multi-node record's deletion cascade to — all of them?) — a semantic decision deserving its own reviewed phase, not a silent change inside a retrieval phase. Flagged with evidence, same discipline G1.1 used for the namespace gap it found.

---

## 12. Recovery

- **Restart** (`Engine::try_recover()` → event-log replay): tested — seed resolution identical before and after, for the exact multi-node/deleted-sibling shape that previously diverged.
- **Snapshot → restore**: tested — seed and full subgraph (nodes + edges) byte-identical.
- **Vector index**: remains derived and is never required for correctness — untouched by this phase.

---

## 13. Tests

**`crates/valori-node/tests/vector_graph_retrieval.rs`** (13, new): the full Part-15 matrix — with/without node, node without record, multi-node determinism, sibling-delete survival, only-node delete, edge delete, soft-deleted record, namespace collision, empty cases, diamond + cycle dedupe, multi-hit seed ordering, restart equivalence, snapshot equivalence.

**`crates/valori-node/tests/api_graphrag.rs`** (+2): HTTP-level proof the fix landed on the standalone `/v1/graphrag` path — `graphrag_seed_for_a_multi_node_record_is_the_lowest_node_id` and `graphrag_still_seeds_from_the_surviving_node_after_a_sibling_delete`. Both **verified to fail without the fix** (`Some(1)` vs `Some(0)`; `None` vs `Some(1)`).

**`crates/valori-rag/src/graph.rs`** (+1 `#[ignore]`d): the §10 cost benchmark.

---

## 14. API

**No API change.** `/v1/graphrag` keeps its request and response shape on both paths; only the *value* of `node_id`/`seed_nodes` changes — and only in the multi-node-per-record case that was previously path-dependent and restart-unstable. No new endpoint, so no SDK addition and no route-parity impact (parity re-run anyway, green).

---

## 15. Verification

| Check | Result |
|---|---|
| `cargo fmt --check` (workspace) | Clean |
| `cargo check --workspace` | Clean |
| `cargo clippy -p valori-rag -p valori-node --all-targets -- -D warnings` | Clean |
| `cargo test -p valori-rag` | Green — 37 passed, 3 ignored (benchmarks, by design) |
| `cargo test -p valori-node` | Green — 324 passed (309 prior + 13 new matrix + 2 new HTTP) |
| `route_parity` | Green — no API change |
| wasm32 kernel build | Not applicable — `valori-kernel` untouched this phase |

No new failures; no pre-existing failures encountered.

---

## 16. Final Verdict

**G1.3 complete — Option B.** The vector→graph bridge existed but resolved record→node by two different rules on the two execution paths, one of them a cache that went stale across deletes and silently repaired itself on restart. Both defects were proven empirically, fixed structurally by making both paths derive seeds from canonical state through one shared function, and pinned by tests confirmed to fail without the fix.

Optional linkage is preserved exactly as G1.0 specified. Namespace isolation, determinism, and the canonical/derived boundary are intact. No hybrid ranking, no graph index, no canonical-state change, no hash-contract change.

Not starting G1.4.
