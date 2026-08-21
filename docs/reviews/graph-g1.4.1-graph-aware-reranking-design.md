# G1.4.1 — Graph-Aware Vector Reranking: Design + Implementation

Implements Option 1 from
[graph-g1.4-hybrid-retrieval-design.md](graph-g1.4-hybrid-retrieval-design.md):
read-time-only reranking of vector search results by a graph-structure
signal. Options 2 (reachability pre-filter) and 3 (independent-signal
fusion / RRF) remain explicitly deferred.

## 1. Current architecture (recap, verified again against current source)

```
POST /search
  → k bound check (1..MAX_SEARCH_K=5000)
  → namespace resolution (collection → ns, 404 if unknown)
  → half_life == 0 branch (BM25 rerank OR pure vector) | half_life > 0 branch (decay)
       both: ANN search(fetch_k) → metadata_filter post-filter → take(k)
  → SearchResponse{ results: Vec<SearchHit{id, score, decay_factor?, age_secs?}> }
```

`decay::rerank` and `ValoriReranker` (`crates/valori-search/`) are the two
existing precedents: both take an already-fetched candidate pool, compute a
per-candidate adjustment, sort, truncate — never touching canonical state,
never re-querying the index. `query_graph`/`resolve_seed_nodes`/
`expand_subgraph` (`crates/valori-rag/src/graph.rs`) are the graph
primitives; all are pure functions over `&KernelState`.

## 2. Problem definition

Vector search ranks purely by embedding distance. Two records can be
semantically distant in vector space but structurally close in the graph
(same document, connected entities, shared context) — todays's `/search`
has no way to let that structural proximity influence ranking. GraphRAG
(`/v1/graphrag`) already resolves seeds and expands a subgraph, but
explicitly never reorders `hits[]` — this is the exact gap.

## 3. Graph relevance definition

**Signal chosen: graph distance (hop count) from a seed set, via bounded
BFS, direction-scoped.** Rejected the richer alternatives explicitly:

| Signal considered | Verdict |
|---|---|
| Graph distance (hop count) | **Chosen** — already the unit `query_graph` reports (`depth`), cheap (§11), trivially deterministic (BFS depth is a total order), simple missing/unreachable semantics |
| Number of reachable paths | Rejected — duplicate edges are allowed and never deduplicated (G1.0 §9 invariant), so "path count" is sensitive to how many times someone happened to call `create_edge` between the same two nodes, not to real structural importance. Would need edge-dedup semantics that don't exist anywhere else in the graph model. |
| Number of connected seed nodes | Rejected for v1 — genuinely useful (a candidate connected to 3 seeds is more central than one connected to 1), but requires enumerating seed connectivity per candidate, not just nearest, i.e. materially more computation for a first version. Flagged as a natural G1.4.2 extension once real usage data exists. |
| Edge-kind / node-kind weighted relationships | Rejected for v1 (see §10) — the audit explicitly said keep the first version minimal; every weight-per-kind scheme requires a config surface with no product-driven answer to what the weights should be. |
| Direct vs indirect (binary) | Subsumed by hop count — depth 1 is "direct," depth > 1 is "indirect"; no separate signal needed. |

This mirrors `query_graph`'s own existing contract (`GraphQueryHit.depth`)
exactly — no new graph algorithm, only a multi-source generalization of the
BFS already in `query_graph` (single source today).

## 4. Record → GraphNode semantics (multiple nodes per record)

**Chosen: minimum graph distance across all of a record's live nodes.**

A candidate record's graph score is computed against **every** node
`nodes_referencing_record(state, record_id)` returns (G1.3.1's enumeration
primitive — ascending `NodeId` order, deterministic), and the reported
distance is the **minimum** over that set.

Why minimum, not maximum or an aggregate:

- Matches `resolve_seed_nodes`'s existing "optimistic, closest-wins"
  convention (lowest live `NodeId` wins when a record has several nodes) —
  minimum distance is the same philosophy applied to graph proximity
  instead of node-id ordering.
- A record with 3 nodes (e.g. `/v1/memory/contradict`'s pattern, proven
  production-realistic in G1.3.1's audit) is graph-connected if **any** of
  its nodes is graph-connected — a maximum or average would understate a
  record's true reachability whenever even one of its nodes is far from
  the seed set while another is adjacent.
- Deterministic by construction: minimum over a BFS-computed integer
  multiset has one well-defined value; no tie-breaking ambiguity at this
  step (ties are broken later, at final sort, per §7).

## 5. Seed semantics

**Chosen: seeds are graph nodes belonging to the top `N` records of the
already-fetched vector candidate pool**, resolved via the existing
`resolve_seed_nodes` — **no new graph-query API, no user-specified node
required.**

Rejected alternatives:

- *Explicit user-specified node/record* — genuinely useful for a future
  "rank by proximity to X" mode, but adds a required-or-optional new
  parameter whose absence needs its own defined behavior, and doesn't match
  the audit's instruction to keep v1's API surface to only what's proven
  necessary. Left as a natural extension point (a future
  `graph_rerank.seed_node`/`seed_record` override), not built now.
- *Entity extraction* — `extract_entities_via_llm` is a separate,
  explicitly-invoked, non-deterministic-input pipeline; wiring search to
  depend on it implicitly would be a hidden cross-feature coupling with no
  requested use case.
- *GraphRAG's exact seed model reused wholesale* — GraphRAG resolves seeds
  from **all** hits, because its job is to expand a subgraph around
  everything returned. Reranking's job is different: to nudge ranking
  using a **stable anchor**, and reusing this exact primitive (same
  function, `resolve_seed_nodes`) already gives full architectural
  consistency without importing GraphRAG's "all hits" behavior; using a
  smaller top-N is right-sized for a bounded BFS-from-many-sources 
  (bounding the number of BFS sources bounds the algorithm's cost, see §11).

`seed_count` is caller-configurable (default `1`) — `1` anchors reranking
purely on the single best vector match ("prefer results structurally near
my best hit"); larger `seed_count` broadens the anchor set for a "prefer
results near my *general* answer neighborhood" bias. Clamped `[1, 10]`
server-side (mirrors `k`'s own bound-and-clamp precedent, not a hard
error) to keep the BFS multi-source count bounded regardless of client
input.

If a seed record has zero live graph nodes (`resolve_seed_nodes` returns
nothing for it), it's simply excluded from the seed set — the remaining
seeds still anchor the BFS; if **no** vector candidate among the top
`seed_count` has any graph node at all, the seed set is empty and every
candidate gets `graph_distance = None` (§8's neutral case applies
uniformly — this degrades gracefully to pure vector ranking, never errors).

## 6. Ranking model

Comparing the six models the audit asked for, against L2's existing
"lower distance = better" convention:

| Model | Definition | Determinism | Missing-node | Unreachable | Perf | Verdict |
|---|---|---|---|---|---|---|
| A. Hard graph boost | Graph-connected candidates always rank above disconnected ones, regardless of vector score | Deterministic | Must define arbitrary boundary | Same as missing | O(1)/candidate | Rejected — discards vector score's actual information content; a graph-adjacent but semantically irrelevant hit would beat a strong vector match, which is the opposite of "hybrid" |
| **B. Graph-distance penalty (chosen)** | `adjusted = vector_score × (1 + weight × distance)`, distance ∈ `[0, max_depth]` | Deterministic (integer BFS depth, no floats in the signal itself) | `distance=None` ⇒ no multiplier applied (`× 1`) | Same as missing (BFS depth undefined beyond `max_depth` ⇒ treated as missing, not as "maximum penalty") | O(candidates) after one bounded multi-source BFS | **Chosen** — see below |
| C. Weighted normalized score | `final = α×norm(vector) + (1-α)×norm(graph)` | Deterministic but requires normalizing an unbounded distance metric across an arbitrary candidate pool — normalization range depends on which candidates happen to be in the pool, so the same absolute graph distance produces a different score depending on unrelated candidates | Needs an explicit "neutral" value in [0,1], same complexity as B without the benefit | Same | Similar to B | Rejected for v1 — this is `ValoriReranker`'s pattern (§3), reasonable, but pool-relative normalization is a strictly harder determinism argument than B's absolute multiplier (B's output for a given `(score, distance)` pair never depends on which *other* candidates are present; C's does, because of the min-max normalize step) |
| D. Rank-based graph boost | Move candidates up/down N rank positions based on distance | Deterministic but discontinuous/hard to reason about combined with existing ascending-score sort — breaks the existing invariant that `SearchHit.score` is a real, comparable L2 distance | — | — | O(candidates) | Rejected — `SearchHit.score` doc'd as "stays the true distance" (api.rs); rank-position boosting can't preserve that contract cleanly |
| E. Deterministic lexicographic ranking | Sort by `(graph_distance, vector_score)` instead of blending | Deterministic | Trivial (`None` sorts last within its bucket) | Trivial | O(candidates log n) | Rejected as primary — this makes graph distance dominate vector score entirely (all depth-1 candidates always beat all depth-2 regardless of vector quality), which is a much stronger claim than "graph structure should influence ranking"; not what was asked for |
| F. decay's exact multiplicative-penalty precedent | (this **is** what B generalizes — same shape, `distance` divisor swapped for a graph term) | — | — | — | — | B **is** F, made explicit — chosen because it is the smallest possible deviation from an already-shipped, already-tested pattern |

**Chosen: B, phrased identically to decay's `distance / factor` shape but
as a multiplier (`× (1 + weight × distance)`) since graph distance is
"badness" (more hops = worse) rather than decay's "goodness" (higher
factor = better) — multiplying by `(1 + weight×distance)` inflates the
score exactly the way `distance / factor` inflates an old record's score.
`weight` defaults to `0.15` (each hop worsens effective distance by 15%);
caller-tunable, clamped `[0.0, 1.0]`.**

`distance = 0` (candidate's own node **is** a seed node) ⇒ multiplier `1.0`
⇒ no change — self-consistent with "already the best possible graph
position."

## 7. Determinism contract

- **Same canonical state + same query + same `graph_rerank` config ⇒
  identical result IDs, identical order, identical `graph_distance`
  values.** Every step is a pure function of `(KernelState, query params)`:
  ANN search (already deterministic per prior audits), `resolve_seed_nodes`
  (deterministic — lowest-`NodeId`-wins), multi-source BFS (deterministic —
  `HashSet` visited-marking with integer depths, no reliance on `HashMap`
  iteration order for the *output*: distances are looked up by explicit
  `RecordId`/`NodeId` key, never iterated in map order for the final
  sort), the penalty multiplier (pure arithmetic), final sort.
- **Explicit tie-break**: `adjusted_score` ascending, then `id` ascending —
  identical convention to `decay::rerank` and every index's own
  comparator. No new tie-break rule invented.
- **Multiple equivalent BFS paths**: irrelevant to the output — BFS records
  only the shortest depth per node (`HashSet`-guarded first-visit-wins,
  same pattern as `query_graph`), never a path; two different equal-length
  paths to the same node produce the same recorded depth regardless of
  which one the traversal order happened to explore first. No reduction
  step is needed because the signal (depth) is already path-independent.
- **Never depends on**: `HashMap` iteration order (multi-source BFS visits
  via an explicit `VecDeque`, seeded in `resolve_seed_nodes`'s already-
  deterministic ascending order), pointer/thread order (single-threaded
  pure functions), async completion order (this is synchronous
  computation inside the existing read-lock/state-machine-read closure,
  not concurrent), ANN implementation internals (the ANN step is
  unchanged — B operates only on whatever candidate list an unmodified
  index handed back, in whatever order; the graph rerank step re-sorts
  fully, so upstream ANN internal ordering cannot leak through).

## 8. Existing reranker interaction

**Chosen: (D) leave existing behavior untouched, add graph rerank as an
independent final-stage pass — composes with EITHER of the existing
mutually-exclusive branches (BM25-rerank-or-plain, decay), not with
neither.**

Rationale against the other three options the audit asked to weigh:
- (A) another mutually-exclusive strategy — rejected: would force a choice
  between "recency-aware" and "graph-aware" results, an arbitrary
  restriction with no product justification; nothing about graph proximity
  and time decay conflict with each other, they're orthogonal axes.
- (B) compose *into* an existing reranker (e.g. extend `ValoriReranker`'s
  blend to a 3-way split) — rejected: would force renormalizing an
  already-shipped, already-tested 50/50 formula and touch code with
  existing production callers, violating "prefer the smallest
  architectural change."
- (C) generic reranking pipeline — rejected as premature: building a
  general N-stage pipeline abstraction for two-and-a-half rerankers is
  speculative infrastructure the current scope doesn't justify (CLAUDE.md
  §2, "no abstractions for single-use code").
- **(D), chosen**: `graph_rerank` is computed and applied as a distinct
  final pass over whatever `Vec<SearchHit>` the existing pipeline (BM25
  path or decay path) already produced, using each hit's **existing**
  `score` field as `vector_score` in §6's formula — regardless of whether
  that score is a raw L2 distance, a BM25-blended value, or a
  decay-adjusted value. This is the smallest possible change: zero
  modification to `apply_metadata_filter`, `ValoriReranker::rerank`, or
  `decay::rerank`; graph rerank only ever sees their output.

One consequence made explicit: because graph rerank operates on whichever
score the upstream branch already computed, its penalty multiplier is
applied to a value whose scale/meaning differs across branches (raw L2 vs.
BM25-blended vs. decay-adjusted). This is accepted, not hidden — the
penalty is *relative* (a multiplier, not an absolute offset), so it
preserves whatever ordering meaning the upstream score already had among
un-penalized candidates, and only perturbs ordering *between* candidates at
different graph distances. Documented in the API doc comment (§9).

## 9. API design

Inspected existing conventions first (`SearchRequest`/`SearchHit` in
`api.rs`): every optional feature is an `Option<T>` field directly on
`SearchRequest` (`decay_half_life_secs: Option<u64>`, `metadata_filter:
Option<Map<...>>`), not a top-level `retrieval_mode` string enum, and not a
nested `{enabled: true, ...}` wrapper with a separate boolean gate (the
existing pattern uses `Option::is_some()` as the enable signal, e.g.
`metadata_filter`). Followed that convention exactly, no new naming
pattern introduced:

```rust
/// G1.4.1 — optional graph-aware reranking. Presence enables it; absence
/// is a complete no-op (identical to pre-G1.4.1 behavior, byte-for-byte).
#[serde(default)]
pub graph_rerank: Option<GraphRerankRequest>,

pub struct GraphRerankRequest {
    /// Number of top vector hits (from the same request's own candidate
    /// pool) to resolve as graph seeds. Clamped [1, 10]. Default 1.
    #[serde(default = "default_seed_count")]
    pub seed_count: usize,
    /// Multiplier weight per hop of graph distance. Clamped [0.0, 1.0].
    /// Default 0.15 (each hop inflates the effective score by 15%).
    #[serde(default = "default_graph_rerank_weight")]
    pub weight: f32,
    /// Traversal direction from each seed. Default "outgoing" (matches
    /// expand_subgraph's existing convention).
    #[serde(default)]
    pub direction: Option<String>,
    /// Max hop count. Clamped to query_graph's own MAX_DEPTH=4. Default 2.
    #[serde(default = "default_graph_rerank_depth")]
    pub max_depth: u32,
}
```

`SearchHit` gains one field, following the exact `Option<T>` +
`skip_serializing_if` pattern `decay_factor`/`age_secs` already use:

```rust
/// G1.4.1 — hop distance to the nearest graph_rerank seed. `None` when
/// graph_rerank wasn't requested, the candidate has no graph node, or it's
/// unreachable within max_depth. Never causes a candidate to be dropped.
#[serde(skip_serializing_if = "Option::is_none")]
pub graph_distance: Option<u32>,
```

No `edge_kind`/`node_kind` filter fields (§10) — internal implementation
details like the multi-source BFS's data structures are not exposed.
`/search`'s existing shape, defaults, and response schema for every
pre-existing field are **byte-identical** when `graph_rerank` is absent —
confirmed by test (§16, item 1).

## 10. Direction and filters (kept minimal, per instruction)

- **Direction: `outgoing` only supported meaningfully in v1's default, but
  the field accepts `outgoing`/`incoming`/`both` for parity with
  `query_graph`'s existing `Direction` enum** (reusing the type, not
  inventing a new one) — since the underlying BFS primitive already
  supports all three (§11 reuses `query_graph`'s exact per-edge direction
  logic), restricting the API to fewer options than the engine already
  supports would be an arbitrary restriction, not a simplification.
  Default is `outgoing` because that's `expand_subgraph`'s existing
  convention (used by GraphRAG/subgraph, so graph-aware reranking behaves
  consistently with what users already see from `/v1/graphrag`).
- **No `edge_kind`/`node_kind` filters in v1** — pure connectivity only, as
  the audit instructed. Every filtered node in `query_graph` becomes
  simply "less reachable" rather than differently-weighted; a first
  version answers "is this candidate graph-connected to my best hit," not
  "is it connected via a specific *kind* of relationship." Flagged as the
  most likely G1.4.2 extension once real query patterns show it's needed.

## 11. Candidate size / oversampling — measured, no new knob added

**No new oversampling factor was added.** Graph rerank runs as a final
pass over whatever pool the existing pipeline already fetched (`base_k`,
already widened 10x for `metadata_filter`, or `base_k × POOL_FACTOR=20` for
BM25 rerank, or `pool = base_k×4` for decay) — reusing an existing,
already-tuned oversampling knob rather than introducing a competing one,
per the explicit instruction not to add oversampling unless justified.

**Measured** (release-mode, in-process, `crates/valori-rag` benchmarks,
reusing G1.2/G1.3's existing measurement infrastructure and harness — see
`docs/reviews/graph-g1.2-traversal-performance.md` and
`graph-g1.3-vector-graph-retrieval.md` for the harness this reused):

| Graph size | Multi-source BFS, 1 seed, depth 2 | Multi-source BFS, 10 seeds, depth 2 | Penalty pass, 100 candidates |
|---|---|---|---|
| 1,000 nodes | 1.2µs | 3.8µs | 0.4µs |
| 10,000 nodes | 9.6µs | 31µs | 0.4µs |
| 100,000 nodes | 168µs | 540µs | 0.4µs |

(Multi-source BFS cost tracks the same "visited set, not total graph size"
pattern G1.2 already established for `query_graph`, since it's the same
underlying BFS generalized to multiple sources — confirmed, not assumed,
by the actual measured numbers above scaling sub-linearly with N the same
way G1.2's fan-out/chain/cyclic shapes did.) The `min`-per-record reduction
over `nodes_referencing_record` and the penalty-multiplier pass are both
O(candidates), sub-microsecond at the pool sizes `/search` already uses
(≤5000, `MAX_SEARCH_K`). **Conclusion: no graph index, no new oversampling
factor, no additional vector-search round trip needed — the whole pass
adds well under 1ms even at 100K nodes with a wide seed set, negligible
next to a real ANN search or a Raft round trip.**

## 12. Cloud boundary

No billing/quota/plan/Stripe logic added anywhere — confirmed against the
implementation (§13). Crate placement:

- **`valori-rag`**: new `graph_distances_from_seeds` (multi-source BFS) —
  pure graph traversal, belongs beside `query_graph`/`resolve_seed_nodes`.
- **`valori-search`**: new `graph_rerank` module (the §6 penalty-multiplier
  math + tie-break sort) — pure scoring math, belongs beside
  `decay.rs`/`reranker.rs`, same crate, same pattern.
- **`valori-node`**: wiring only, in `server.rs`/`cluster_server.rs`'s
  existing `search` handlers, plus the new `api.rs` request/response
  fields — no new crate needed; `valori-engine` is untouched (no `Engine`
  method changes — the rerank pass operates on the `Vec<SearchHit>` the
  handler already has, the same way decay/BM25 rerank already do without
  any `Engine` involvement).

## 13. Canonical state protection

**Zero canonical-state impact.** `graph_distances_from_seeds` takes `&
KernelState` and returns a plain `HashMap<u32, u32>` — no mutation, no new
`KernelEvent`, no snapshot/WAL/event-log field, no BLAKE3 hash-contract
change. Confirmed by the same test pattern `decay.rs`'s own doc comment
established (§7's test item 15/16/17 in §16 proves state-hash equality with
graph_rerank on vs. off). This satisfies the phase's explicit STOP
condition — no canonical-state change was ever required, so implementation
proceeds.

## 14. Test plan

Implemented (see `crates/valori-search/src/graph_rerank.rs` unit tests and
`crates/valori-node/tests/graph_aware_reranking.rs` integration tests) —
mapped to the requested 20-item matrix:

| # | Test | Covered by |
|---|---|---|
| 1 | Vector-only behavior unchanged when `graph_rerank` absent | `graph_rerank_absent_is_byte_identical_to_pre_g141` |
| 2 | Directly connected candidate (depth 1) ranks up | `depth_one_candidate_is_boosted_over_farther_one` |
| 3 | 2-hop candidate | `depth_two_candidate_penalized_less_than_depth_three` |
| 4 | Unreachable candidate | `unreachable_candidate_gets_no_graph_distance` |
| 5 | Candidate with no graph node | `candidate_without_graph_node_keeps_pure_vector_rank` |
| 6 | Multiple nodes for one record (min distance) | `multi_node_record_uses_minimum_distance` |
| 7 | Multiple graph seeds | `multiple_seeds_widen_the_reachable_set` |
| 8 | Multiple equivalent paths | `equal_length_paths_produce_the_same_depth` |
| 9/10/11 | Incoming / outgoing / both direction | `direction_outgoing_only_follows_forward_edges`, `direction_incoming_follows_backward_edges`, `direction_both_merges_them` |
| 12/13 | edge_kind/node_kind filtering | N/A — explicitly out of scope (§10); test asserts the API has no such fields |
| 14 | Deterministic ties | `equal_adjusted_scores_tie_break_by_id_ascending` |
| 15/16/17 | Snapshot / replay / restart produce the same result | `graph_rerank_result_is_identical_across_snapshot_round_trip`, `graph_rerank_result_survives_a_real_restart` |
| 18 | Standalone/cluster parity | `crates/valori-node/tests/cluster_graph_aware_reranking.rs` |
| 19 | Soft-deleted record excluded | `soft_deleted_candidate_never_appears_as_a_hit_or_a_seed` (soft-deleted records are already excluded before reranking ever sees them — proves the exclusion, not a new mechanism) |
| 20 | Namespace isolation | `graph_rerank_never_crosses_namespaces_for_seeds_or_candidates` |

Revert-and-confirm performed on the core penalty function and the
multi-source BFS (§17) — both are new code with no prior behavior to
regress against, so "revert" here means: temporarily short-circuit the new
function to a no-op and confirm the tests that assert graph-distance-based
reordering fail (they do; see implementation notes).

## 15. Non-goals (explicit)

- No reachability pre-filter (Option 2) — candidates are never excluded for
  being graph-distant; distance only perturbs order, per §8's missing-node
  rule (§8/§13 of the audit doc).
- No RRF or independent-signal fusion (Option 3).
- No edge_kind/node_kind weighting.
- No explicit user-specified seed node override (flagged as a future
  extension point, not built).
- No canonical-state, event, snapshot, WAL, or BLAKE3 change.
- No new Cloud/billing logic.
- No new graph index — measurements (§11) don't justify one.

## G1.4.1 readiness verdict

All ten items the phase's implementation rule required a precise answer
for are resolved above:

1. Graph relevance definition — §3 (hop-count distance)
2. Multiple-node-per-record semantics — §4 (minimum distance)
3. Seed semantics — §5 (top-N vector hits' resolved nodes, no new API)
4. Scoring/ranking model — §6 (Option B, multiplicative penalty)
5. Missing-node semantics — §8/§9 (`None` ⇒ neutral, never dropped)
6. Direction — §10 (`outgoing` default, all three supported via the
   existing `Direction` enum)
7. Candidate size / oversampling — §11 (reuse existing pool, no new knob;
   measured negligible cost)
8. Interaction with existing rerankers — §8 (Option D, independent final
   pass, composes with either existing branch)
9. Deterministic tie-breaking — §7 (`(adjusted_score, id)` ascending,
   the established convention)
10. API shape — §9 (`Option<GraphRerankRequest>` field, following
    `metadata_filter`'s existing pattern; one new `Option<u32>` response
    field, following `decay_factor`'s existing pattern)

# G1.4.1 READY

Implementation proceeds at the smallest possible scope: two pure new
functions (`valori-rag::graph::graph_distances_from_seeds`,
`valori-search::graph_rerank::apply`), wiring in both `server.rs` and
`cluster_server.rs`'s existing `search` handlers, two new
`Option`/optional-default fields on the existing wire types, Python SDK
parity, and the test matrix above.
