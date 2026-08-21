# G1.2 — Graph Traversal Evolution & Performance

*Follow-up to [`graph-g1.1-query-primitives.md`](graph-g1.1-query-primitives.md) and [`graph-g1.1.1-graph-read-namespace-isolation.md`](graph-g1.1.1-graph-read-namespace-isolation.md). Kept intentionally concise per this phase's explicit "move fast, no speculative design docs" instruction.*

---

## 1. Objective

Determine whether `NodePool`/`EdgePool`'s current representation is sufficient for real graph traversal at realistic scale — by measurement, not by intuition, and add a derived index **only if the numbers justify it**.

---

## 2. Current Implementation (fast audit)

Re-verified against source, no drift from G1.1's account:

- **Node lookup**: `KernelState::get_node` — O(1), direct `Vec` index by `NodeId`.
- **Outgoing/incoming adjacency**: **already an intrusive linked list**, not a scan. `GraphNode.first_out_edge`/`first_in_edge` point at the head; each `GraphEdge.next_out`/`next_in` chains to the next. Walking a node's neighbors is O(degree) — the theoretical minimum for "enumerate this node's neighbors," not an artifact of a naive representation. This was already true before G1.1/G1.2; it's baked into the canonical struct layout itself (`crates/valori-kernel/src/graph/{node,edge,adjacency}.rs`).
- **`query_graph`'s traversal loop**: standard BFS — `VecDeque` frontier, `HashSet<u32>` for visited-membership tests only (not iterated for output order, so its unordered nature can't affect results), one `Vec<GraphQueryHit>` collecting matches, one final `sort_by` on `(depth, node_id)`, then `truncate(limit)`.
- **Allocations on the hot path**: the `HashSet`, the `VecDeque`, and the `Vec<GraphQueryHit>` — all sized by the **visited/result set**, not by total graph size.
- **Namespace validation**: one `O(1)` check on the start node before traversal begins (G1.1.1) — not repeated per visited node, since edges can't cross namespaces.
- **Sorting**: yes, `O(k log k)` where `k` = matched-result count (bounded by `limit`, itself capped at 1000) — not proportional to graph size.

**Conclusion going in**: adjacency is already indexed by construction. The open empirical question is whether O(degree) per-node adjacency walks or the BFS bookkeeping become expensive at scale — not whether "unindexed" lookups are happening (they aren't).

---

## 3. Baseline Benchmark

Dependency-free `#[ignore]`d test, following G1.1's established convention (no criterion in this workspace): `crates/valori-rag/src/graph.rs::tests::query_graph_g1_2_scale`, run via `cargo test -p valori-rag --release --lib graph::tests::query_graph_g1_2_scale -- --ignored --nocapture`.

Sizes: 1K / 10K / 100K nodes (100K was practical — no reason found to stop there, but scale beyond it wasn't measured since nothing in the data trends toward needing it). Shapes: chain (low-degree, deep), fan-out tree b=3 (medium-degree, per G1.1's precedent), hub-spoke (one node, huge out-degree — the deliberate high-degree stress case), cyclic (fan-out + back-edges, exercises visited-set dedup). Depths 1–4 (traversal's own cap), directions outgoing/incoming/both, filtered vs. unfiltered.

Real numbers (release build, this machine):

| Shape | N=1,000 | N=10,000 | N=100,000 |
|---|---|---|---|
| Chain, depth 1–3, outgoing | 197–306ns | 296–866ns | 174–290ns |
| Chain, depth 4, incoming | 74ns | 263ns | 103ns |
| Fan-out (b=3), depth 4, outgoing (120 results) | 14.4µs | 14.4µs | 7.8µs |
| Fan-out (b=3), depth 4, outgoing, **edge_kind filtered** | 5.9µs | 14.3µs | 8.7µs |
| Fan-out (b=3), depth 4, **both** directions | 13.7µs | 14.0µs | 8.2µs |
| Cyclic, depth 4, outgoing (120 results) | 16.4µs | 8.5µs | 7.4µs |
| **Hub-spoke, depth 1, outgoing (walks N-1 direct edges)** | **59µs** | **595µs** | **3.19ms** |

---

## 4. Bottleneck Identified

**Classification: mostly J (nothing significant), with D (outgoing adjacency) real but architecturally inherent for one specific shape.**

- Chain/fan-out/cyclic are **flat across two orders of magnitude of total graph size** — fan-out depth 4 costs the same whether N=1,000 or N=100,000, because it always visits exactly 120 nodes (the reachable set at that depth/branching factor). **Traversal cost tracks the visited set, not total graph size** — direct confirmation that adjacency is already effectively indexed; there is no scan-based cost hiding anywhere.
- Filters showed no measurable overhead — `edge_kind`/`node_kind` checks are O(1) per candidate and don't change the algorithm's complexity class.
- Sorting/HashSet bookkeeping is not visible as a cost at these result sizes (≤1,000 elements; `O(k log k)` is microseconds at most).
- **The one real, measurable cost**: `hub_spoke` at depth 1 scales linearly with the hub's own out-degree (999 → 9,999 → 99,999 edges: 59µs → 595µs → 3.19ms, ≈10x per 10x). This is **not a defect in the current representation** — enumerating a node's neighbors is inherently O(degree) for *any* data structure (a hash-based or CSR-based adjacency index would still need to touch every one of that node's edges to discover them before sorting/limiting). What an index *could* improve here is the **constant factor** (a contiguous CSR array has better cache locality than linked-list pointer-chasing), not the complexity class — and even the current worst case (3.19ms, for a synthetic 100,000-degree single node) is fast in absolute terms, with no evidence any real workload approaches that degree.

---

## 5. Decision

**OPTION A — NO NEW INDEX.**

The current `NodePool`/`EdgePool` representation, unchanged, is sufficiently fast at every measured scale and shape. No derived adjacency index, no CSR conversion, no caching layer was added. This is a measurement-driven conclusion, not a default: the benchmark specifically included a shape (hub-spoke) designed to surface exactly the kind of cost an adjacency index would address, and even there the cost is inherent to the operation, not to the representation.

---

## 6. Optimization

None implemented — Option A. Nothing in §§7–11 of the requested template (canonical/derived boundary for a new index, determinism of index construction, recovery of a derived index, before/after comparison, memory tradeoff) applies, since no index exists to describe. Restated for completeness: `NodePool`/`EdgePool` remain exactly as G0 established them — canonical, and the *only* graph-adjacency structure in the system. There is no second, derived adjacency representation to keep in sync, rebuild, or reason about separately.

---

## 7. Canonical vs. Derived Boundary

Unchanged. `NodePool`, `EdgePool`, the event log, and snapshots remain canonical (G0). `query_graph`'s BFS bookkeeping (`HashSet`, `VecDeque`, the result `Vec`) is per-call, stack-local, and discarded on return — it was already correctly "derived" in the trivial sense of not persisting anywhere, and that hasn't changed.

---

## 8. Determinism Guarantees

Unchanged from G1.1 — no new construction-order question exists because no index was built. `query_graph`'s existing guarantee (same canonical graph + same query → same result, same declared `(depth, node_id)` order) still holds, re-confirmed by the full existing G1.1 test suite passing unmodified (§10) and by the new restart test (§9) proving it holds across a real `Engine::try_recover()` cycle too, not just kernel-level replay/decode. The G0.2 hash contract was not touched.

---

## 9. Recovery Behavior

Unchanged — no derived index exists to need rebuilding after restart/snapshot/replay. One gap in G1.1's own coverage was closed: G1.1 proved kernel-level replay- and snapshot-decode-equivalence for `query_graph`, but not a full `Engine::try_recover()` cycle (event log → fresh `Engine` → recovered `KernelState`). Added `crates/valori-node/tests/graph_query_restart_recovery.rs::query_graph_result_survives_engine_restart`: builds a small graph (3 nodes, 2 edges, 1 self-loop) in a real `Engine` backed by a real event-log path, drops it (simulating a crash), boots a fresh `Engine`, calls `try_recover()`, and asserts `query_graph` returns the byte-identical result before and after.

---

## 10. Tests

No new *behavioral* tests were needed — G1.1's 26 unit tests plus G1.1.1's 8 namespace-isolation tests already cover every item in G1.2's Part 9 checklist (depths 1–5 via the existing `MAX_DEPTH` clamp behavior, all three directions, edge/node-kind filtering, cycles, self-loops, duplicate edges, namespace isolation, result limits, deterministic ordering, replay, snapshot restore) — re-run and confirmed green, unmodified, in §11. The one new test is the restart-recovery case (§9), since it exercises a layer (`Engine::try_recover()`) G1.1 didn't reach. Items 19–23 (index rebuild, rebuild after deletion/creation/cascade, construction-order independence) don't apply — no index was built.

---

## 11. Before/After Benchmark

Not applicable — Option A. §3's numbers **are** the final numbers; nothing changed. No memory/rebuild-time tradeoff to report, because nothing was built.

---

## 12. Deferred Work

- If a real workload ever produces a node with genuinely pathological out-degree (thousands+) *and* low-depth, high-frequency queries against it become a measured problem in practice, a CSR-style contiguous adjacency representation would be the natural next step — but that's a constant-factor cache-locality improvement, not a complexity fix, and nothing in this benchmark shows it's needed today.
- The G1.1.1-noted kernel-level `DeleteNode`/`DeleteEdge` namespace-blindness (single enforcement layer, not defense-in-depth) remains an open note from that phase, unrelated to traversal performance.

---

## 13. Verification

| Check | Result |
|---|---|
| `cargo fmt --check` (workspace) | Clean |
| `cargo check --workspace` | Clean |
| `cargo clippy -p valori-rag -p valori-node --all-targets -- -D warnings` | Clean |
| `cargo test -p valori-rag` | 37/37 passing, 2 ignored (the G1.1 and G1.2 benchmark tests, by design) |
| `cargo test -p valori-node` | 309/309 passing (308 prior + 1 new restart test) |
| `cargo test -p valori-node --test route_parity` | Passing — no route/API changes this phase |
| `wasm32-unknown-unknown` kernel build | Not re-run — `valori-kernel` was not touched this phase (only `valori-rag`'s test module and `valori-node`'s test suite), so CLAUDE.md's trigger condition for this check did not fire |

No new failures, no pre-existing failures encountered, no environment issues.

---

## 14. Final Verdict

**G1.2 complete. Option A chosen and measurement-justified**: the current canonical `NodePool`/`EdgePool` representation, with its already-intrusive-linked-list adjacency, is sufficiently fast for graph traversal from 1,000 to 100,000 nodes across chain, fan-out, hub-spoke, and cyclic shapes, at every tested depth, direction, and filter combination. The single real cost (a high-degree node's own adjacency walk) is inherent to the operation, not fixable by an index, and not currently a practical problem. No new infrastructure was introduced. Canonical state, the event/snapshot formats, the BLAKE3 contract, the vector indexes, and the graph query API surface are all exactly as G1.1/G1.1.1 left them.

Not starting G1.3.
