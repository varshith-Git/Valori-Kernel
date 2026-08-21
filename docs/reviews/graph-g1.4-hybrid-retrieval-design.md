# G1.4 — Hybrid Vector + Graph Retrieval: Audit & Design Contract

**Status: AUDIT + DESIGN ONLY. No production code was modified for this
document.** No new APIs were added, no `KernelEvent` changed, no
snapshot/WAL/event-log format touched, no BLAKE3 contract touched, no
HNSW/IVF/BQ implementation touched.

This is the design sub-phase G1.0 explicitly deferred to before any
hybrid-ranking implementation could start:

> "G1.4 — Hybrid retrieval with graph constraints (pipeline C)... Explicitly
> requires a design sub-phase before implementation: what a 'graph
> constraint' means in Valori's model must be specified first."
> — [graph-g1.0-evolution-contract.md:327-334](graph-g1.0-evolution-contract.md)

and G1.3 confirmed nothing had been built yet:

> "Hybrid ranking is explicitly G1.4's, not G1.3's — nothing was added here.
> Not starting G1.4." — [graph-g1.3-vector-graph-retrieval.md:71,184](graph-g1.3-vector-graph-retrieval.md)

Everything below labels claims **SOURCE FACT** (verified against current
source, file:line cited), **WEB-RESEARCH-CITED** (external prior art,
citation given), or **MODEL INFERENCE**/**RECOMMENDATION** (my synthesis —
never presented as if the code already does it).

---

## 1. Vector search path — audited end-to-end

**SOURCE FACT.** Two parallel, non-identical search stacks exist:

| | Standalone | Cluster |
|---|---|---|
| Entry | `server.rs:1289 search()` | `cluster_server.rs:954 search()` |
| State | `valori-engine::Engine` (`valori-index` crate: BruteForce/HNSW/IVF/BQ, f32) | `valori-kernel::state::kernel::KernelState` (only BruteForce/BQ, fixed-point Q16.16) |
| Namespace enforcement | Exact path: per-namespace intrusive linked list traversal (`kernel.rs:228-269`). ANN path: **post-filter** a namespace-agnostic global `k`-sized pool (`engine.rs:863-875`) — no pool widening for this filter, unlike metadata_filter's 10x widening. | Structural: `shard_for(ns_id)` routes to a namespace-owning shard (`cluster_server.rs:993`), then searches with `filter: None` inside that shard — no in-call namespace filter at all. |
| `k` cap | `MAX_SEARCH_K = 5000` | same constant |
| Pagination | None — `k`-only, single-shot | None |

**All four indexes** (`valori-index` crate) implement `search(query, k) ->
Vec<(u32, f32)>` sorted **ascending — lower score/distance = better match**,
uniformly:

- **BruteForce**: exact squared L2, O(N), no over-fetch.
- **HNSW**: approximate squared L2, greedy descent + `ef_search` (default
  50) candidate pool at layer 0, deterministic level assignment (FNV-hash of
  id, not RNG — reproducible across restores).
- **IVF**: approximate squared L2 in Q16.16 fixed-point, probes `n_probe`
  nearest centroids (auto-scaled `sqrt(n_list)`), candidate pool = union of
  probed inverted lists.
- **BQ**: two-stage — Hamming distance over packed 1-bit codes for coarse
  ranking (`candidates_cap = max(10×k, 200)`), then exact L2 re-rank on the
  retained pool.

Deterministic tie-break is uniform everywhere in the stack: **ascending
`(score, id)`** — kernel `SearchResult::Ord`, every index's own comparator,
`decay::rerank`. This is the existing convention any new hybrid ranking
should extend, not replace.

**Re-ranking is already layered on top of raw ANN results, never before
candidate generation:**

```
ANN search (k × POOL_FACTOR candidates)
   → metadata_filter post-filter (Phase I7, 10x pool widening when active)
   → BM25 rerank (ValoriReranker, Phase C5) OR decay rerank (Phase C4.1)
   → take(k)
```

- **BM25 rerank** (`valori-search::reranker`): `POOL_FACTOR = 20` over-fetch,
  then `hybrid = 0.5 × norm(1 − L2) + 0.5 × norm(BM25)` — a **static 50/50
  linear blend of two independently min-max-normalised score axes**.
- **Decay rerank** (`valori-search::decay`): `adjusted = distance /
  decay_factor(age, half_life)`, where `decay_factor = 0.5^(age/half_life)`
  — a **multiplicative penalty on the primary axis**, not a weighted sum.
  `SearchHit.score` stays the true undecayed distance; only ranking order
  changes. Never touches canonical state or the BLAKE3 hash.
- These two are **mutually exclusive per request** in current code
  (`half_life == 0` branches to rerank, else to decay) — cannot compose in
  one `/search` call today.

**Discrepancies found, relevant to G1.4's design surface:**

1. `search_l2_ns` filters on `Record::is_active()`, not `is_searchable()` —
   the latter's doc comment says encrypted records "must be excluded" from
   search, but the exact-path kernel function doesn't call it. Needs
   experimental verification; not exercised by this design phase but should
   be a G1.4 test-matrix item if hybrid ranking is expected to compose with
   encrypted records.
2. IVF is documented as an auto-tier option (10k–2M) but
   `effective_index_kind()`'s `Auto` arm never actually selects it — only
   reachable via explicit config. Relevant because a hybrid pipeline that
   assumes "the active index" is one of {BruteForce, BQ, HNSW} in
   auto-mode is accurate; assuming IVF is reachable without explicit config
   is not.
3. Namespace isolation on the standalone ANN path has no pool-widening
   analog to metadata_filter's — a namespace holding a small fraction of
   global records can silently return `< k` results on HNSW/IVF/BQ. Any
   hybrid design that widens the candidate pool for its own purposes should
   not assume this gap is already handled; it should be aware it compounds.
4. Search response (`SearchHit`) carries **no metadata, no namespace echo,
   no distinct `distance` vs `score`** — only `id`, `score`,
   `decay_factor?`, `age_secs?`. A hybrid response schema decision (§6) has
   to decide from scratch what a "graph-aware" hit needs to carry.

Full detail: see the standalone audit transcript folded into this document's
citations above; line numbers were independently verified against current
source, not copied from a prior phase doc.

---

## 2. Graph retrieval path — audited end-to-end

**SOURCE FACT**, re-verified against current source (not just the G1.1/
G1.3/G1.3.1 docs, which remain accurate for the pieces re-checked).

`query_graph` (`valori-rag/src/graph.rs:150-233`) — **BFS**, not DFS:

- Depth = hop count from `start`, clamped `min(MAX_DEPTH=4)`.
- Start node validated against the resolved namespace inline (wrong
  namespace ⇒ `None`, same as "not found" — no existence leak).
- Cycle/dedup: `HashSet<u32>` visited, first-visit-wins ⇒ reported depth is
  shortest-path depth.
- `edge_kind`/`node_kind` filters apply **during traversal** — a
  non-matching edge is never followed (nodes reachable only through it are
  never visited); a non-matching node is recorded neither as a hit nor
  expanded through (dead end, not a filtered-out pass-through).
- Direction: `Outgoing`/`Incoming`/`Both`, `Incoming` walks edges backwards
  via `edge.from`.
- **Ordering contract**: full traversal runs to completion, then
  `sort_by(|a,b| a.depth.cmp(&b.depth).then(a.node_id.cmp(&b.node_id)))`,
  **then** `truncate(limit)` — so `limit` always keeps the closest results
  by the declared order, never an arbitrary BFS-visitation-order subset.
- Result shape: `GraphQueryHit { node_id, kind, record_id: Option<u32>,
  depth }`. **No `EdgeKind` traversed and no `path: Vec<NodeId>` are
  reported** — only the reached node and its shortest depth.

`expand_subgraph` (used by `/v1/graphrag` and `/graph/subgraph`) reports
full node/edge JSON objects (`{id,kind,record}` / `{id,from,to,kind}`) but
**no depth-per-node and no path** — membership only, always outgoing-only,
depth-clamped BFS.

`resolve_seed_nodes` / `nodes_referencing_record` (G1.3/G1.3.1's fixes,
confirmed current): both derive from canonical `KernelState` per call, no
cache — `resolve_seed_nodes` picks the **lowest live `NodeId`** per record
deterministically; both standalone and cluster `graph_rag` now call the
identical function (the G1.3 parity fix holds).

**GraphRAG full pipeline** (`capabilities.rs::graph_rag`, both paths
structurally identical): `search_l2_ns` → `resolve_seed_nodes` →
`expand_subgraph` → assemble. Returns **untyped `serde_json::Value`**, no
typed `GraphRagResponse` exists:

```json
{
  "hits": [{"memory_id","record_id","score","node_id","metadata"}, ...],
  "seed_nodes": [u32, ...],
  "subgraph": {"nodes": [...], "edges": [...]}
}
```

Critically: **`hits[]` keeps pure vector ordering; graph expansion never
reorders it** (confirmed both in code and by explicit prior-phase doc
statement). This is the exact gap G1.4 exists to close — there is no
existing code path anywhere that lets graph structure influence a ranked
result list.

Node/edge/subgraph lookups (`GET /v1/graph/node/:id`,
`/v1/graph/edges/:id`, `/v1/graph/subgraph`) are namespace-enforced on both
paths (G1.1.1's fix, confirmed present via inline "G1.1.1" comments in
current source). `route_parity.rs`'s allowlists contain **zero graph-related
entries** — every graph endpoint exists identically on both routers,
mechanically enforced.

Python SDK (`python/valoricore/remote.py`) exposes `create_node`,
`create_edge`, `get_node`, `get_edges`, `graph_query`, `delete_node`,
`list_nodes`, `subgraph`, `graphrag` — symmetric across
`_SyncGraphMixin`/`_AsyncGraphMixin`, confirmed by matching signatures.

Entity extraction (`valori_rag::extract_entities_via_llm`, wired to `POST
/v1/ingest/extract-entities`) is a **purely synchronous, explicitly-called
endpoint** — not part of any automatic pipeline. Once entities are
inserted as records+nodes, they're ordinary seedable data; nothing special
about them for hybrid retrieval purposes.

---

## 3. Existing hybrid-scoring precedent already in the codebase

**SOURCE FACT.** Three independent precedents exist for "blend two score
axes into one ranking" — none of them touch graph structure, but they are
the architectural template G1.4 should either extend or deliberately
diverge from:

1. **`ValoriReranker` (Phase C5)** — static 50/50 linear blend of
   min-max-normalised vector distance and BM25 term-frequency score, over a
   `POOL_FACTOR=20`-widened candidate pool.
2. **`decay::rerank` (Phase C4.1)** — multiplicative penalty on the primary
   distance axis (`distance / decay_factor(age, half_life)`), not a
   weighted sum of independent axes. Read-time only; never mutates state or
   the BLAKE3 hash.
3. **`tree_hybrid` (Phase I5)** — caller-tunable linear blend
   (`tree_weight` parameter, default 0.6) of tree-relevance score and
   vector similarity, both min-max-normalised, tagged by `source` in one
   merged `hits[]` list. Note: `tree.rs`'s own default (0.6) and
   `capabilities.rs`'s parameter-absent fallback (0.5) diverge — a minor
   pre-existing inconsistency, not introduced by this design.

**Community layer (`rank_communities`, cosine over centroids) is never
merged with vector-hit scores anywhere in the codebase** — confirmed by
exhaustive grep for call sites. If G1.4 wanted "vector score + community
proximity" fusion, that would be a wholly new combination, not an extension
of an existing pattern.

**Conclusion**: every existing Valori hybrid-scoring precedent is a
**static or tunable weighted linear combination of normalised score axes**,
computed read-time on an already-fetched candidate pool, never affecting
canonical state or the hash contract. None use rank-position fusion (RRF).
This is the path of least architectural surprise for G1.4 (see §5, §7).

---

## 4. Determinism constraints (already established, apply unchanged)

**SOURCE FACT**, per `docs/architecture/layers.md` and
[graph-g1.0-evolution-contract.md §9](graph-g1.0-evolution-contract.md):

- Given identical inputs (event stream, snapshot bytes, fixed-point
  format), every node must produce identical `KernelState`, hash, and
  snapshot bytes. No wall-clock, OS RNG, thread scheduling, filesystem
  ordering, or floating-point in the canonical hot path.
- `valori-kernel` stays `no_std` — non-negotiable (CLAUDE.md invariant #7).
- **Existing convention any new ranking must adopt**: ascending `(score,
  id)` tie-break — used uniformly by every index, `decay::rerank`, and
  `query_graph`'s own `(depth, node_id)` ordering.
- Traversal ordering is "deterministic but implementation-defined"
  (adjacency-list construction order); duplicate edges are allowed and
  independently tracked, never deduplicated — any ranking/counting logic
  built on graph structure must be designed against this, not assume a
  clean simple graph.
- **Read-time-only rule** (decay's precedent): a ranking function that
  reorders results without writing new `KernelEvent`s, without touching
  `KernelState`, and without affecting `hash_state_blake3` needs **zero**
  compatibility/format work — it's pure post-processing. This is the
  cheapest, lowest-risk shape for G1.4 to take, and matches the LLM/entity
  extraction non-determinism precedent (canonical events are deterministic;
  the *inputs that produced them* need not be).

---

## 5. Measured performance already available — no new benchmarking needed to design

**SOURCE FACT**, cited directly from prior phase docs so G1.4 doesn't
re-measure what's already answered:

| Primitive | Cost | Source |
|---|---|---|
| `query_graph` traversal, chain/fan-out/cyclic, 1K–100K nodes | 74ns–17µs, flat across N (tracks visited set, not graph size) | [graph-g1.2](graph-g1.2-traversal-performance.md) |
| `query_graph`, pathological hub-spoke (single high-degree node) | 59µs (1K) → 595µs (10K) → 3.19ms (100K), linear in degree | [graph-g1.2](graph-g1.2-traversal-performance.md) |
| `resolve_seed_nodes`, k=10 | 1.1µs (1K) → 10.3µs (10K) → 173µs (100K), O(live_nodes) | [graph-g1.3](graph-g1.3-vector-graph-retrieval.md) |

**Implication**: a hybrid pipeline built on `resolve_seed_nodes` +
bounded-depth `query_graph`/`expand_subgraph` costs low-single-digit
milliseconds even at 100K nodes, except the known pathological hub-spoke
case (already flagged, not new). No index-building work is justified by
current measurements — same conclusion G1.2 already reached for pure graph
traversal, and it carries over unchanged to a hybrid pipeline that
composes the same primitives.

---

## 6. What "graph-constrained/hybrid retrieval" could mean — the undecided question G1.0 flagged

G1.0 named this "pipeline C" and explicitly left the semantics open. Based
on the audit above, there are three genuinely distinct capabilities hiding
under the single phrase "hybrid retrieval," and Valori has zero
implementation of any of them today:

### Option 1 — Graph-structure re-rank (extends the existing precedent)

Vector search returns its normal ranked candidate pool (as today); a
**read-time-only** post-process re-ranks (not filters) that pool using a
graph-structure signal — e.g. "boost records whose node is within N hops of
a set of anchor nodes." Architecturally identical in shape to
`decay::rerank`/`ValoriReranker`: candidate pool in, adjusted scores out,
`(score, id)` tie-break preserved, zero canonical-state/hash impact.

- **Pro**: smallest, safest, most consistent with existing precedent (§3).
  No new determinism argument needed beyond "read-time re-rank," already
  established.
- **Con**: doesn't change *which* records are candidates, only their order
  — cannot express "only return records reachable from X," only "prefer
  records reachable from X."

### Option 2 — Graph-reachability pre-filter (a real constraint)

Compute a reachable-node set via `expand_subgraph`/`query_graph` from one
or more anchor nodes *first*, map it to a `RecordId` set (inverse of
`resolve_seed_nodes`/`nodes_referencing_record`), then constrain vector
search to only that set — either as a hard filter (candidates must be in
the set) or as the seed set itself (search only within it).

- **Pro**: actually answers "vector search constrained by graph structure,"
  which is what G1.0's "pipeline C" name implies and what §6's audit shows
  nothing in Valori currently does.
- **Con**: requires either (a) a new `VectorIndex` capability to search a
  restricted id subset — none of the four index types expose this today
  (their `search(query, k)` trait signature has no filter parameter beyond
  the kernel's own generic `filter: Option<u64>` tag, which is unrelated),
  or (b) brute-force scoring only the reachable-set records (only
  reasonable if the reachable set is small — the hub-spoke case in §5 shows
  it can be large). This is real new design work, not a read-time
  composition of existing pieces.

### Option 3 — Fused ranking across both signal types (RRF or weighted sum)

Run vector search and a graph-signal query (e.g. proximity to a set of
"important" nodes, or community membership) independently, then fuse the
two ranked lists into one, either via weighted linear combination
(matching §3's precedent) or reciprocal rank fusion (RRF — used by
Weaviate/Qdrant as their default hybrid-fusion algorithm, and by Neo4j's
"Weighted RRF" pattern combining full-text/vector/graph-topology signals —
**WEB-RESEARCH-CITED**, see §7).

- **Pro**: most powerful, most aligned with prior art elsewhere in the
  industry.
- **Con**: RRF specifically would be a new fusion paradigm for this
  codebase (rank-position-based, not score-based) — every existing Valori
  blend is a normalised weighted linear combination (§3), so RRF needs its
  own determinism argument (which I have not built here — flagged as
  deferred design work, not decided).

**These three are not mutually exclusive** — Option 1 could ship first
(cheapest, safest, closest to shippable today) with Option 2 or 3 layered
on later once the "what does a graph constraint mean" question gets a
concrete answer backed by a real product use case (G1.0's own
recommendation for deferring this).

---

## 7. Prior art (WEB-RESEARCH-CITED)

- **Reciprocal Rank Fusion (RRF)**: `score = Σ 1/(k+rank)` across ranked
  lists, method-agnostic. Weaviate's default hybrid-fusion algorithm;
  Qdrant exposes `Fusion.RRF` as a first-class mode. [Reciprocal Rank
  Fusion explained](https://blog.serghei.pl/posts/reciprocal-rank-fusion-explained/)
- **Neo4j Weighted RRF / hybrid search**: combines full-text, vector, and
  graph-topology signals via weighted re-ranking; typical pattern is
  "hybrid search finds starting points, graph traversal expands into
  connected context." [Hybrid Search in Neo4j](https://neo4j.com/blog/developer/hybrid-search-in-neo4j-full-text-vectors-and-graph-topology-with-cypher/)
- **Weighted linear combination of lexical + semantic scores**: the
  standard "hybrid search" pattern in the broader RAG literature. [Hybrid
  Search: BM25, Vector & Reranking Reference 2026](https://www.digitalapplied.com/blog/hybrid-search-bm25-vector-reranking-reference-2026)

**MODEL INFERENCE**: Valori's own precedent (§3) is architecturally closer
to weighted-linear-combination than to RRF — recommend that path for
consistency unless a specific product need (e.g. combining signals with
wildly different score distributions where normalisation is unreliable)
argues for RRF instead.

---

## 8. Recommendation

**Do not implement Option 2 or Option 3 in this phase.** Both require new
design decisions this document has surfaced but not resolved (index
subset-search capability, RRF's determinism argument) and both were the
explicit reason G1.0 flagged this as needing its own design sub-phase
rather than being implementable directly.

**If G1.4 proceeds to implementation, start with Option 1** (graph-
structure re-rank, read-time-only, weighted-linear-combination pattern
matching `decay::rerank`/`ValoriReranker`/`tree_hybrid`):

- Zero canonical-state/event/snapshot/BLAKE3 impact — same class of change
  as decay.
- Reuses `resolve_seed_nodes` + bounded `query_graph`/`expand_subgraph`
  exactly as measured in §5 — no new kernel-level work.
- Gives a concrete, shippable answer to "graph influences vector ranking"
  without yet answering "graph constrains vector candidates," which stays
  open for a future G1.4.x once a real product use case defines what
  "constrained" should mean (G1.0's own deferred question, still open).

**This recommendation is not implemented in this phase.** Per the explicit
instruction governing this phase (audit + design only), no code changes
were made. The next step, if you approve Option 1's direction, is a
follow-up implementation phase (tentatively G1.4.1) that specifies: the
exact blend formula and tunable weight parameter (mirroring `tree_weight`),
the exact API surface (new `/search` parameter vs. new endpoint), the exact
response schema addition, and the full determinism/namespace/standalone-
cluster-parity test matrix — none of which this audit-only phase should
pre-decide without your sign-off, consistent with the discipline you've
held every prior phase to.

---

## 9. Non-goals (explicit)

- No change to `KernelEvent`, snapshot format, WAL/event-log format, or the
  BLAKE3 hash contract.
- No change to HNSW/IVF/BQ/BruteForce implementations.
- No new API endpoints or request/response fields added.
- No resolution of the `is_active()`/`is_searchable()` discrepancy (§1) or
  the IVF auto-tier gap (§1) — both flagged for their own follow-up, out of
  this phase's scope.
- No decision on Option 2 vs. Option 3 (§6) — left open pending a concrete
  product use case, per G1.0's own precedent for deferring undecided
  semantics rather than guessing.

## Final status

**Audit complete. Design options presented. No implementation started.**
Awaiting your decision: proceed with Option 1 as a G1.4.1 implementation
phase, pursue Option 2/3 with additional design work first, or hold here.
