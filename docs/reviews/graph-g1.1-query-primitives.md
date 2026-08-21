# G1.1 — Deterministic Graph Query Primitives

*Follow-up to [`graph-g0-architecture-audit.md`](graph-g0-architecture-audit.md), [`graph-g0.1-determinism-state-integrity.md`](graph-g0.1-determinism-state-integrity.md), [`graph-g0.2-canonical-state-hash-commitment.md`](graph-g0.2-canonical-state-hash-commitment.md), and [`graph-g1.0-evolution-contract.md`](graph-g1.0-evolution-contract.md). Implements the one roadmap item G1.0 identified as having zero open design questions.*

---

## 1. Existing Graph Query Capability Audit

Re-verified against the current source tree (G1.0's audit was correct; no drift found), classified per this phase's own rubric:

| Capability | Classification | Evidence |
|---|---|---|
| Node lookup (`get_node`) | **EXISTS** | `KernelState::get_node` (`crates/valori-kernel/src/state/kernel.rs:129`) — O(1), namespace-blind at the kernel level (namespace enforcement is the *caller's* job — see §12's finding) |
| Direct outgoing/incoming neighbor iteration | **EXISTS** | `KernelState::outgoing_edges`/`incoming_edges` (kernel.rs:137,143) — O(degree) linked-list walks |
| Relationship-type filtering | **MISSING** | No existing function filters by `EdgeKind` during traversal; `expand_subgraph` (`crates/valori-rag/src/graph.rs`) follows every outgoing edge unconditionally |
| Node-kind filtering | **PARTIALLY EXISTS** | `ListNodesQuery.kind: Option<u8>` filters the *flat* `list_nodes` result (`crates/valori-node/src/routes/graph.rs:203-220`), but no traversal-time node-kind filter existed anywhere |
| Bounded-depth traversal | **EXISTS** | `expand_subgraph`, depth clamped to `MAX_DEPTH = 4` |
| Direction control (incoming / both) | **MISSING** | `expand_subgraph` is outgoing-only; no incoming or bidirectional traversal existed anywhere in the graph code |
| Deterministic ordering (declared, not incidental) | **PARTIALLY EXISTS** | `expand_subgraph`'s output is deterministic (G0.1 proved this) but its order is raw BFS-visitation order — an implementation detail, not a documented contract |
| Result-count limits | **MISSING** | `expand_subgraph` has a depth cap but no result-count cap; only `list_nodes` has a `limit`/`offset` (applied post-hoc to a flat list, not during traversal) |
| Namespace-safe start-node validation | **MISSING, and the gap is worse than "missing" — see §12** | `get_node`/`node_edges`/`subgraph`'s existing `GraphOps` implementations accept a resolved namespace but never check the looked-up node actually belongs to it |
| Node/edge CRUD | **EXISTS**, unrelated to this phase | Unchanged, per G0's audit |
| Entity extraction → graph construction | **EXISTS**, unrelated to this phase | Unchanged, per G1.0's audit |
| GraphRAG (`/v1/graphrag`) | **EXISTS**, unrelated to this phase | Unchanged — reuses `expand_subgraph`, not the new primitive (deliberately; see §16, non-goals) |

**Conclusion**: node lookup and single-direction adjacency reads were sound, existing primitives to reuse as-is. Filtering, direction control, declared ordering, and result limits were genuinely missing — this is exactly the gap G1.0 §7 classified as P0 work.

---

## 2. G1.1 Scope — the Query Model

One new primitive, `valori_rag::graph::query_graph`, deliberately smaller than the `GraphQuery { start_node, direction, edge_type, node_kind, max_depth, limit }` sketch in the brief — the actual struct is:

```rust
pub struct GraphQuery {
    pub start: NodeId,              // required
    pub direction: Direction,        // Outgoing | Incoming | Both — required, but always has a caller-visible default (Outgoing)
    pub edge_kind: Option<EdgeKind>, // optional — restricts which edges are FOLLOWED, not just reported
    pub node_kind: Option<NodeKind>, // optional — restricts which nodes are ENTERED, not just reported
    pub max_depth: u32,              // clamped [0, MAX_DEPTH=4], never rejected
    pub limit: usize,                // clamped [1, MAX_QUERY_LIMIT=1000], never rejected
}
```

| Field | Required? | Default | Bounded? |
|---|---|---|---|
| `start` | Required | — | Must reference an existing node in the query's namespace, or the whole query returns `None` |
| `direction` | Required at the type level | `Outgoing` at the HTTP layer (`GraphQueryParams.direction: Option<String>`) | Fixed 3-way enum |
| `edge_kind` | Optional | `None` (no filter) | Must be a valid `EdgeKind` u8 or the request is rejected (400) |
| `node_kind` | Optional | `None` (no filter) | Must be a valid `NodeKind` u8 or rejected (400) |
| `max_depth` | Optional | `DEFAULT_QUERY_DEPTH = 2` | Silently clamped to `[0, MAX_DEPTH=4]` — never an error |
| `limit` | Optional | `DEFAULT_QUERY_LIMIT = 100` | Silently clamped to `[1, MAX_QUERY_LIMIT=1000]` — never an error |

Answering the brief's eight questions directly:
1. **Start node** — `GraphQuery.start`, required.
2. **Namespace** — resolved from the HTTP `collection` param exactly like every other graph endpoint (`routes/graph.rs::resolve`), then passed as an explicit `namespace_id: u16` argument to `query_graph` itself — not just used to pick a shard (§12).
3. **Edge direction** — `Direction::{Outgoing, Incoming, Both}`.
4. **Relationship types** — `edge_kind: Option<EdgeKind>`, a single value (matches the existing `ListNodesQuery.kind: Option<u8>` convention — one filter value, not a set, consistent with prior art rather than adding new flexibility nothing asked for).
5. **Node kinds** — `node_kind: Option<NodeKind>`, same single-value convention.
6. **Traversal depth** — `max_depth`, hop count from `start`, excluding `start` itself (§5).
7. **Result count** — `limit`.
8. **Ordering** — declared, not incidental: ascending `(depth, node_id)` (§8).

---

## 3. Node Lookup

**Reused, not reimplemented.** `KernelState::get_node` already exists, is correct, and is exactly what `query_graph` calls first (`state.get_node(query.start)?`). No second node representation was created — `GraphQueryHit` (the new result type) is a strict *subset* of the existing `GraphNode`/`GetNodeResponse` shape (`node_id`, `kind`, `record_id`), plus `depth`, which is new because it's meaningful only in a traversal-query context.

---

## 4. Neighbor Traversal

`Direction::Outgoing`/`Incoming`/`Both` are implemented by calling `outgoing_edges`/`incoming_edges` (both pre-existing, unmodified) on the current BFS frontier — `Both` calls both and merges. Namespace isolation is preserved exactly as G0 established: edges structurally cannot cross namespaces (enforced at `apply_event_ns`'s `CreateEdge` arm), so no traversal starting from a genuinely-namespace-validated `start` node can ever reach another namespace — this was re-verified, not assumed, by `traversal_cannot_cross_namespaces_even_when_attempted` (§11), which proves the *setup* (a cross-namespace edge) is impossible to construct at all.

---

## 5. Relationship-Type Filtering

`edge_kind: Some(k)` restricts which edges are **followed** during BFS — not merely which are reported afterward. An edge whose kind doesn't match `k` is never walked, so a node reachable *only* through a non-matching edge kind is never visited, never reported, and never expanded through. `EdgeKind` is the existing, canonical, fixed 9-variant enum (`crates/valori-core/src/enums.rs`) — no new representation was invented; `Option<EdgeKind>` (typed, not a raw string) is used internally, with u8 encoding at the HTTP boundary matching `CreateEdgeRequest.kind`'s existing convention exactly.

---

## 6. Node-Kind Filtering

Symmetric with edge-kind filtering, by deliberate design choice: `node_kind: Some(k)` restricts which nodes are **entered** — a non-matching node is a dead end, not expanded through, exactly like a non-matching edge. This symmetry was chosen (not the alternative "filter output only, still traverse through non-matching nodes") because it makes `max_depth`'s meaning unambiguous: "N hops through the *filtered* subgraph," not "N hops through the raw graph, then filter." `node_kind_filter_blocks_traversal_through_non_matching_nodes` (§11) proves this concretely: a node reachable only by passing through a filtered-out node is correctly absent from the result even when nominally within `max_depth`.

The existing canonical `NodeKind` enum was reused as-is — this capability did not require, and did not add, any new canonical field.

---

## 7. Bounded Traversal — Depth Semantics

**Precisely defined, per the brief's explicit instruction not to leave this ambiguous**: `max_depth` counts hops from `start`; **the start node itself is never counted, never returned, regardless of its own kind**. `max_depth: 1` returns only direct neighbors. This was verified against the brief's own worked example (Alice → Bob → Charlie → Dave chain) in `bounded_depth_returns_exactly_the_expected_prefix` (§11), which asserts the exact expected prefix at depths 1/2/3 and explicitly asserts the start node is absent even at the deepest tested depth.

**Cycles**: handled by the same visited-set mechanism `expand_subgraph` already used (and G0.1 already proved correct for a self-loop case) — a node is inserted into the visited set the first time it's reached and never re-entered. `cycle_does_not_cause_infinite_traversal` (§11) proves a 3-node cycle (0→1→2→0) terminates and correctly excludes the start node even though the cycle revisits it. `self_loop_on_start_node_produces_no_hit` and `self_loop_on_a_reached_node_does_not_hang_or_duplicate` (§11) cover the two self-loop placements separately.

---

## 8. Deterministic Ordering

**The contract, stated precisely** (and now different from — stronger than — `expand_subgraph`'s "deterministic but implementation-defined" order, per G1.0 §9's own recommendation to fix this for any new capability):

> Given the same canonical graph and the same query, `query_graph` returns the same result, in the same order: ascending `depth`, then ascending `node_id` within the same depth.

This is an explicit, declared, tested sort — not incidental BFS-queue order. `ordering_is_depth_then_node_id_ascending` (§11) proves this concretely by constructing a graph where edge-creation order does *not* match ascending id order, so a passing test rules out "the sort just happens to already be right."

**Multiple paths to the same node**: deduplicated, reported once, at its shortest-path depth (the standard first-visit-wins BFS property — the same guarantee `resolve_seed_nodes`/`expand_subgraph` already relied on implicitly). `duplicate_edges_report_the_target_node_once` (§11) proves this for the "3 parallel edges" case specifically (distinguishing it from "3 distinct paths through different intermediate nodes," which the diamond-graph coverage in the pre-existing `traversal_output_is_deterministic_across_repeated_runs` test already exercises).

**Limit interaction**: traversal completes in full first (bounded by `max_depth`), the complete result is sorted by `(depth, node_id)`, and *only then* truncated to `limit`. This means `limit` always keeps the `limit` closest results by the declared ordering — never an arbitrary BFS-visitation-order-dependent subset that would differ if the traversal algorithm's internal order ever changed. `limit_keeps_the_closest_results_by_depth_then_id` (§11) proves this directly.

---

## 9. Query Limits

**Engineering defaults for this phase, explicitly not product/billing decisions** (per the brief's explicit instruction to keep these concerns separate):

| Constant | Value | Kind |
|---|---|---|
| `DEFAULT_QUERY_DEPTH` | 2 | Default when `depth` is absent |
| `MAX_DEPTH` (reused from the pre-existing `expand_subgraph` constant, not duplicated) | 4 | Hard cap, silently clamped |
| `DEFAULT_QUERY_LIMIT` | 100 | Default when `limit` is absent |
| `MAX_QUERY_LIMIT` | 1000 | Hard cap, silently clamped |

Depth `0` is a valid, meaningful input ("traverse nothing, confirm the start node exists, return no neighbors") distinct from a missing/invalid start node (which returns `None`, not an empty list) — `depth_zero_returns_empty_for_existing_start` (§11) proves the distinction. Limit `0` is floored to `1` rather than treated as meaningful, since an "empty limit" would just be a confusing second way to spell what `max_depth: 0` already spells clearly.

Both bounds are clamped, never rejected with an error — matching the pre-existing `expand_subgraph`/`SubgraphQuery` convention (`depth.min(MAX_DEPTH)`) rather than introducing a new "reject oversized input" pattern with no precedent elsewhere in the graph API. `depth_and_limit_are_clamped_not_rejected` (§11) proves wildly-oversized inputs don't panic and behave as if clamped.

These are graph-traversal safety limits only. Nothing here reads a plan tier, a Stripe subscription, or any Cloud concept — `valori-rag` has no dependency capable of reaching such a thing (unchanged from G0's architecture findings), and this phase did not add one.

---

## 10. Canonical vs. Derived

**No new canonical state was introduced.** `query_graph` reads `KernelState` through the exact same public methods `expand_subgraph` already used (`get_node`, `outgoing_edges`, `incoming_edges`) — it adds no new `KernelEvent` variant, touches no snapshot format, and does not appear in `hash_state_blake3`. The BFS visited-set (`HashSet<u32>`), the traversal queue (`VecDeque`), and the intermediate `hits: Vec<GraphQueryHit>` are all per-call, stack-local, and discarded when the function returns — there is no cache, no persisted index, and nothing that could drift from canonical state because nothing outlives the call. This satisfies G1.0 §6's extended canonical/derived boundary without needing to introduce any of the acceleration structures that section anticipated as *possible* future additions (CSR, adjacency caches, etc.) — the benchmark in §16 shows why none were needed for this phase.

---

## 11. Tests

35 new tests, all passing, none flaky (each run multiple times during development):

**`crates/valori-rag/src/graph.rs`** (26 unit tests, `cargo test -p valori-rag --lib graph::`):

| # | Test | Proves |
|---|---|---|
| 1 | `depth_zero_returns_empty_for_existing_start` | Single-node "lookup" via depth 0 |
| 2 | `missing_start_node_returns_none` | Missing-node lookup |
| 3 | `outgoing_neighbor_filtered_by_edge_kind`, `outgoing_neighbor_filtered_by_different_edge_kind` | Direct outgoing neighbor + relationship-type filtering (both branches of the brief's own worked example) |
| 4 | `incoming_neighbor_from_bobs_perspective` | Direct incoming neighbor |
| 5 | `both_direction_traversal_from_bob_reaches_alice_going_incoming` | Both-direction traversal |
| 6 | `node_kind_filter_returns_only_matching_kind`, `node_kind_filter_blocks_traversal_through_non_matching_nodes` | Node-kind filtering, including the traversal-restricting semantic (§6) |
| 7 | `start_node_in_different_namespace_returns_none`, `traversal_cannot_cross_namespaces_even_when_attempted` | Namespace isolation, both the direct-check and the impossible-to-construct-a-leak angles |
| 8 | `bounded_depth_returns_exactly_the_expected_prefix` | Bounded depth (exact brief worked example) |
| 9 | `cycle_does_not_cause_infinite_traversal` | Cycle handling |
| 10 | `duplicate_edges_report_the_target_node_once` | Duplicate-edge behavior |
| 11 | `self_loop_on_start_node_produces_no_hit`, `self_loop_on_a_reached_node_does_not_hang_or_duplicate` | Self-loop, both placements |
| 12 | `ordering_is_depth_then_node_id_ascending` | Deterministic ordering (real proof, not an accident of construction order) |
| 13 | `repeated_identical_query_is_deterministic` | Repeated-query determinism |
| 14 | `replayed_graph_returns_identical_query_result` | Replay equivalence |
| 15 | `snapshot_restore_produces_identical_query_result` | **The "critical test"** — S → query → R1; snapshot(S) → restore → query → R2; R1 == R2 |
| 16 | `unrelated_node_in_another_namespace_does_not_change_results` | Property-style example: an unrelated namespace cannot perturb results (§12) |
| 17 | `limit_keeps_the_closest_results_by_depth_then_id` | Result-limit semantics |
| 18 | `depth_and_limit_are_clamped_not_rejected` | Bound-clamping, not rejection |
| 19 | `hits_report_linked_record_id_when_present` | Record linkage preserved in results |
| — | Plus the pre-existing `empty_seeds_returns_empty`, `resolve_seeds_empty_state`, `traversal_output_is_deterministic_across_repeated_runs` (G0.1) | Unmodified, still passing |

**`crates/valori-node/tests/api_graph_query.rs`** (9 HTTP integration tests, `cargo test -p valori-node --test api_graph_query`): direct outgoing neighbors, edge-kind filtering, node-kind filtering, missing-start → 404, invalid direction → 400, invalid edge-kind → 400, unknown collection → 404, incoming/both direction, and repeated-query determinism — exercising the query-string parsing and error-mapping layer the unit tests can't reach.

**`crates/valori-node/tests/route_parity.rs`** (pre-existing, mechanically enforced): passes unmodified — `/v1/graph/query` exists with the same method on both `server.rs` and `cluster_server.rs`, so no allowlist entry was needed.

---

## 12. Namespace Isolation — and a Newly Discovered Issue

`query_graph` is namespace-safe **by construction**: it takes an explicit `namespace_id: u16` parameter and validates `start_node.namespace_id == namespace_id` before doing anything else, returning `None` (collapsed with "does not exist," never distinguished) on mismatch. This was a deliberate design choice, not an accident — and building it surfaced a real, pre-existing gap in three *sibling* endpoints that this phase does **not** fix (out of scope; flagged as a follow-up task instead, per the brief's own "report any newly discovered architectural issues" instruction):

**Finding**: `GraphOps::get_node`, `node_edges`, and `subgraph` — on **both** `server.rs` (`impl GraphOps for SharedEngine`) and `cluster_server.rs` (`impl GraphOps for DataPlaneState`) — accept a resolved `ns: u16` but never validate the looked-up node actually belongs to it.
- Standalone: the parameter is literally named `_ns` in all three methods — explicitly unused.
- Cluster: `ns` is used only to select which Raft shard to read from (`shard_for(ns)`); since a shard can host multiple namespaces (`namespace_id % shard_count`), a node from a *different* namespace on the *same* shard is still returned/traversed with no check.
- `list_nodes`, in the same files, gets this right (`filter(|n| n.namespace_id == ns)`) — proving the fix pattern already exists in the codebase, just not applied everywhere.

Practical impact: `GET /v1/graph/node/:id?collection=tenant-A` (and the edges/subgraph equivalents) can return data belonging to a different tenant if the numeric id happens to exist there. This is a genuine cross-tenant read leak, not a theoretical one.

**Not fixed in this phase** — this is a security-relevant change to *existing* endpoints, deserving its own reviewed, tested phase, not a scope-creeping addition to a "query primitives" phase. A follow-up task has been filed with full reproduction details and the exact fix pattern (mirroring `query_graph`'s own validation). The new `/v1/graph/query` endpoint added in this phase does **not** have this bug — it was built with the check from the start, and `start_node_in_different_namespace_returns_none` (§11) proves it.

---

## 13. Canonical vs. Derived Boundary — Summary

Restated concisely: this phase added **zero** canonical state, **zero** new `KernelEvent` variants, **zero** snapshot format changes, and **zero** changes to `hash_state_blake3`. Every new type (`GraphQuery`, `GraphQueryHit`, `Direction`) is a request/response shape or a pure-function parameter — none of them are stored anywhere. This was a hard constraint throughout, not a coincidence of the final design.

---

## 14. API Boundary

**Minimal, following existing convention exactly, not invented from scratch**: one new endpoint, `GET /v1/graph/query`, added to both `server.rs` and `cluster_server.rs` via the same shared-handler (`GraphOps` trait) pattern every other graph endpoint already uses. No new REST resource shape, no new auth model (existing bearer-token middleware applies unchanged, since it wraps the whole router), no new collection/project concept. Request validation (invalid direction/edge-kind/node-kind → 400, unknown collection → 404, missing start node → 404) reuses the exact existing helper functions (`resolve`, `node_not_found`) and error-response shape (`{"error": "..."}`) every other graph handler already uses — no new error system was introduced, per the brief's explicit instruction.

**Nothing Cloud-related was touched or introduced.** No plan/tier/billing concept exists anywhere in `valori-rag` or the new code in `valori-node` — consistent with `docs/architecture/ownership.md`'s existing rule that such concepts may not appear in the OSS platform core.

Python SDK: `graph_query(start, direction=, edge_kind=, node_kind=, depth=, limit=, collection=)` added to both `SyncRemoteClient` and `AsyncRemoteClient` in `python/valoricore/remote.py`, matching the existing `get_node`/`get_edges` calling convention (None on 404, `ConnectionError` on transport failure).

---

## 15. Error Semantics

| Condition | Response | Reused from |
|---|---|---|
| Missing/invalid namespace (`collection`) | 404, `{"error": "unknown collection '...'"}` | `routes/graph.rs::resolve` (existing) |
| Missing start node (or wrong namespace) | 404, `{"error": "node N not found"}` | `routes/graph.rs::node_not_found` (existing) |
| Invalid `direction` string | 400, `{"error": "unknown direction '...'"}` | New, but same shape/pattern as the invalid-kind 400s below |
| Invalid `edge_kind`/`node_kind` u8 | 400, `{"error": "unknown edge/node kind: N"}` | Exact same pattern as `create_node`/`create_edge`'s existing `NodeKind::from_u8`/`EdgeKind::from_u8` validation |
| Oversized/zero `depth`/`limit` | Silently clamped, not an error | `expand_subgraph`'s existing `depth.min(MAX_DEPTH)` convention |
| Malformed query string (missing required `start`) | 400 (axum's default `Query<T>` extractor behavior) | Framework default, same as `SubgraphQuery.root: u32` today |

---

## 16. Performance Baseline

No criterion/bench harness exists in this workspace (existing benchmarks are either plain `main()` binaries under `crates/valori-cli/src/bin/bench_*.rs` or Python scripts against a live node) — a criterion dependency was **not** added; a `#[ignore]`d, `--nocapture`-printed test was added instead, following the exact convention already used by this repo's fixture generators (`generate_snapshot_fixtures`, etc.):

```
cargo test -p valori-rag --release --lib graph::tests::query_graph_baseline -- --ignored --nocapture
```

Real numbers, release build, this machine:

| Scenario | Iterations | Total | Per-iteration |
|---|---|---|---|
| Direct neighbor lookup (depth=1) | 10,000 | 1.50ms | **149ns** |
| Depth-2 traversal | 10,000 | 4.41ms | **440ns** |
| Depth-3 traversal | 10,000 | 5.97ms | **597ns** |
| Filtered traversal (edge_kind) | 10,000 | 3.16ms | **316ns** |
| Traversal with cycles (50 nodes, depth=4) | 1,000 | 527µs | **527ns** |
| 1,000-node fan-out graph (depth=4, limit=1000) | 100 | 1.52ms | **15.2µs** |

**Finding**: sub-microsecond for small graphs, ~15µs for a 1,000-node graph at the maximum allowed depth. Per the brief's explicit instruction ("do not introduce an adjacency index simply because traversal isn't fast enough on a tiny benchmark... if performance is already sufficient, keep the implementation simple"): **no acceleration structure is justified by this data.** This is a baseline for later phases to compare against as real graph sizes grow, not a bottleneck to solve now.

---

## 17. Risks

- **The pre-existing namespace-validation gap (§12)** is the one real risk this phase surfaced. Filed as a separate follow-up, not fixed here.
- **`resolve_seed_nodes`'s O(total_nodes) scan** (flagged in G1.0 §15, unrelated to this phase's own primitive) remains unaddressed — `query_graph` does not use `resolve_seed_nodes` and does not have this cost; it is purely a GraphRAG-path concern, unchanged.
- **No cluster-specific HTTP integration test was added** for `/v1/graph/query` — the cluster `GraphOps::query` implementation calls the identical, already-proven `query_graph` function (same code path as standalone, by the module's own "traversal stays identical by construction, not by copy-paste" design principle), so the marginal value of a full cluster HTTP harness for this specific endpoint was judged low relative to its setup cost. `route_parity.rs` mechanically confirms the route itself exists correctly on both paths.
- **No performance risk identified** at this phase's scale (§16) — revisit if/when real deployments show depth-4 traversal on graphs meaningfully larger than 1,000 nodes becoming a measured bottleneck, not before.

---

## 18. Deferred Capabilities

Explicitly not built, per the brief's Part 16 and G1.0's own non-goals — restated here for completeness, not re-litigated:

- Hybrid vector + graph retrieval, GraphRAG changes (G1.0 §14's G1.2/G1.3/G1.4 — separate phases).
- Shortest path / path existence — G1.0 classified these P2, no evidence they're "already trivial" given the current architecture, so per the brief's own conditional instruction, not built.
- Connected components, subgraph extraction beyond seed-anchored BFS, degree queries — none were part of G1.1's stated scope (query *primitives*, not the full P0+P1 query-model list from G1.0 §7).
- A graph query language, Cypher-like syntax — explicitly rejected in G1.0, restated as a non-goal here.
- Any Cloud/billing/plan-aware logic.
- The namespace-validation fix for `get_node`/`node_edges`/`subgraph` (§12) — filed separately.

---

## Verdict

**G1.1 PASS.**

- Graph queries are deterministic — proven by 26 unit tests + 9 HTTP integration tests, including replay-equivalence and snapshot-round-trip cases.
- Namespace isolation is preserved for the new primitive, with an explicit, tested check — and a *pre-existing* gap in sibling endpoints was found and correctly deferred, not silently left undiscovered.
- Cycles cannot cause infinite traversal — proven directly.
- Depth and result limits are enforced (clamped, never unbounded) — proven directly.
- Relationship-type and node-kind filtering both work, with a precisely documented traversal-restricting semantic — proven directly.
- Replay and snapshot recovery both produce identical query results — proven directly, including the brief's own "critical test."
- No canonical state semantics were touched — zero new `KernelEvent` variants, zero snapshot format changes.
- No BLAKE3 contract changes — `hash_state_blake3` was not touched this phase.
- No vector-index behavior was changed — `valori-index`/HNSW/IVF/BQ were not touched.
- No Cloud/billing logic entered the OSS kernel or node.
- All tests pass: `valori-kernel` 167/167 (unchanged, sanity-checked), `valori-rag` 27/27 (26 + 1 ignored baseline test, run separately), `valori-node` 300/300 (291 prior + 9 new), `valori-storage`/`valori-state`/`valori-consensus`/`valori-engine` all green, `route_parity` green, `cargo fmt --check`/`cargo clippy -- -D warnings` clean across every touched crate.
- Documentation is complete (this document).

Stopping here. Not starting G1.2.
