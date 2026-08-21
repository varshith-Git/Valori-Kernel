# G1.0 — Valori Graph Evolution Contract

*Planning document only. No source code was modified. This is not an implementation phase — it defines the product and architectural contract that must be settled before any G1 graph feature work begins.*

Legend: `[CODE VERIFIED]` fact traced to source · `[SOURCE FACT]` external, cited, verifiable · `[MODEL INFERENCE]` reasonable synthesis, not independently verified · `[RECOMMENDATION]` a judgment call, flagged as such.

---

## 1. Executive Summary

Valori's graph today is a real, canonical, well-tested relationship store — node/edge CRUD, cascade delete, BLAKE3-committed persistence, BFS-based `GraphRAG` combining vector search with subgraph expansion, and (less widely known) a working LLM-driven entity-extraction pipeline that turns free text into real graph nodes and edges. It is **not** a knowledge-graph platform, has no query language, no path-finding beyond BFS, no edge properties, and a fixed 9-variant relationship-type vocabulary. G0–G0.2 proved the canonical layer is deterministic and correctly committed; this document is about what to build **on top of** that foundation, not whether the foundation is sound (it is).

The central finding of Part 2's analysis: Valori's graph does not need to choose between "relationship store" and "GraphRAG" — the codebase already has working, narrow slices of both (§2, §7). The product question is not "which direction" but **how much further to invest in each**, and in what order. §11 proposes a contract; §12 a roadmap; §14 a verdict.

**Verdict: G1 NOT READY as a single monolithic phase** — but a narrowly-scoped **G1.1 (query primitives)** is ready to start immediately once this contract is approved. See §16.

---

## 2. Current Graph Implementation Audit

Re-verified against the current source tree in this session, not assumed from G0/G0.1/G0.2 (which themselves were correct as of this check — no drift found).

| # | Question | Answer | Classification |
|---|---|---|---|
| 1 | What is a GraphNode? | `struct GraphNode { id: NodeId, kind: NodeKind, record: Option<RecordId>, first_out_edge, first_in_edge, namespace_id, next_in_ns, prev_in_ns }` — `crates/valori-kernel/src/graph/node.rs` | IMPLEMENTED |
| 2 | What is a GraphEdge? | `struct GraphEdge { id: EdgeId, kind: EdgeKind, from: NodeId, to: NodeId, next_out, next_in }` — `crates/valori-kernel/src/graph/edge.rs`. No properties field. | IMPLEMENTED |
| 3 | Node identity | `NodeId(u32)`, sequential slot allocation, never reused | IMPLEMENTED |
| 4 | Edge identity | `EdgeId(u32)`, same scheme | IMPLEMENTED |
| 5 | Record ↔ node relationship | One optional, one-directional pointer: `GraphNode.record: Option<RecordId>`. `Record` carries no back-reference to any node. | IMPLEMENTED |
| 6 | Record without a graph node? | Yes — plain `/v1/records` insert never creates a node | CONFIRMED |
| 7 | Node without a record? | Yes — `record: Option<RecordId>` (e.g. purely structural `Concept` nodes) | CONFIRMED |
| 8 | Edge without corresponding records? | Yes — an edge only requires both endpoint *nodes* to exist; if both nodes have `record: None`, the edge connects two vectorless structural nodes | CONFIRMED |
| 9 | Node/edge metadata | `NodeKind` (7 fixed variants), `EdgeKind` (9 fixed variants) only — **no free-form metadata field on either struct**. (`Record.metadata: Option<Vec<u8>>` exists but is a *different* struct.) | NOT IMPLEMENTED (as a graph-level capability) |
| 10 | Namespace guarantees | Every node/record carries `namespace_id`; `CreateEdge` rejects cross-namespace endpoints; `CreateNode` rejects a record from a different namespace. Enforced at `apply_event_ns`, not just the API. | IMPLEMENTED |
| 11 | Graph mutations that exist | `CreateNode`/`AutoCreateNode`, `CreateEdge`/`AutoCreateEdge`, `DeleteNode` (cascades incident edges), `DeleteEdge`. **No `UpdateNode`/`UpdateEdge`** — a node's `kind`/`record` and an edge's `kind`/`from`/`to` cannot be changed after creation; the only mutation is delete-and-recreate. | IMPLEMENTED (create/delete); NOT IMPLEMENTED (update) |
| 12 | Graph queries/traversals | `get_node`, `get_edge`, `outgoing_edges`, `incoming_edges` (kernel-level, O(1)/O(degree) reads); `expand_subgraph` — **BFS only**, depth hard-capped at 4, no DFS, no shortest-path, no relationship-type filtering during traversal | PARTIALLY IMPLEMENTED |
| 13 | Kernel-exposed graph functionality | Mutation via `apply_event_ns`; reads via the four methods above. **No traversal logic lives in the kernel** — BFS is in `valori-rag` (std-only), by design (kernel stays `no_std`). | IMPLEMENTED (data layer only, by design) |
| 14 | `valori-node`-exposed graph functionality | Full CRUD HTTP API on both standalone and cluster paths: `POST/GET/DELETE /v1/graph/node`, `POST/GET /v1/graph/edge`, `GET /v1/graph/nodes`, `GET /v1/graph/subgraph` (BFS); `POST /v1/graphrag` (vector KNN + BFS in one call); **`POST /v1/ingest/extract-entities`** — a real, working LLM-driven pipeline: text → LLM entity/relationship extraction → entity descriptions embedded as vectors → inserted as real `Concept` nodes with real relationship edges (`crates/valori-node/src/server.rs:3903-3990`, confirmed by reading the handler, not assumed from its doc comment) | IMPLEMENTED |
| 15 | Cloud/API-exposed graph functionality | Python SDK has full parity: `create_node`, `create_edge`, `get_node`, `get_edges`, `delete_node`, `list_nodes`, `graphrag`, `extract_entities`, `community_detect`/`community_search`/`community_overview`. **No Cloud-specific graph product surface exists in this repository** — Cloud control-plane concerns (billing, project/collection provisioning UI, multi-tenant graph scoping) live in a private repository not visible here; nothing here establishes what Cloud does or should do with the graph beyond what the OSS node already exposes. | Not established by current code (Cloud-specific) |
| 16 | Structure vs. usable capability | See table below | — |

### Structure-vs-capability classification (per the audit's own strict rule: a struct existing is not a feature)

| Capability | Classification |
|---|---|
| Node/edge CRUD (create, read, delete) | **IMPLEMENTED + USED** |
| Cascade delete, both-direction adjacency | **IMPLEMENTED + USED**, tested |
| Namespace isolation for graph | **IMPLEMENTED + USED**, tested at kernel + replay level (G0.1) |
| Snapshot/replay determinism for graph | **IMPLEMENTED + PROVEN** (G0.1, G0.2) |
| BFS subgraph expansion | **IMPLEMENTED + USED** |
| GraphRAG (vector KNN → seed resolution → BFS) | **IMPLEMENTED + USED**, but narrow — no reranking of the subgraph, no relationship-type awareness in the response, hard depth cap of 4 |
| LLM entity extraction → real graph nodes/edges | **IMPLEMENTED + USED**, but a separate, manually-invoked endpoint — not wired into the default `/v1/ingest` document pipeline |
| Community detection (label propagation) | **IMPLEMENTED + USED**, but entirely derived (no `KernelEvent`, not canonical, not part of the audit chain — confirmed in G0 §4) |
| Node/edge update (change kind, re-point an edge) | **NOT IMPLEMENTED** |
| Edge properties / weights / timestamps | **NOT IMPLEMENTED** |
| Custom/free-form relationship types | **NOT IMPLEMENTED** (fixed 9-variant enum) |
| DFS, shortest-path, path-existence, connected-components, degree queries | **NOT IMPLEMENTED** |
| Relationship-type-filtered or node-property-filtered traversal | **NOT IMPLEMENTED** |
| Graph query language | **NOT IMPLEMENTED** |
| Graph-aware reranking (rank subgraph results by relationship relevance) | **NOT IMPLEMENTED** |
| Per-collection or per-project graph scoping distinct from vector namespace scoping | **NOT IMPLEMENTED** — today the graph shares the exact same namespace substrate as vectors (§5) |

No purely-decorative "data structure with zero callers" was found anywhere in the graph path — unlike the vector-index audit's finding of `BqIndex::snapshot()`/`restore()` being dead stubs, the graph code has no equivalent dead weight. This is a clean foundation to build on.

---

## 3. What Valori's Graph Actually Is

A **typed, directed, namespace-scoped relationship store** over records and structural concepts, canonical and replayable, with two working (if narrow) retrieval capabilities layered on top: BFS-based subgraph expansion and an LLM-driven entity-extraction-to-graph pipeline. It is explicitly **not**: a property graph, a general-purpose graph database, a system with a query language, or (today) a fully automated knowledge-graph construction pipeline wired into default ingestion.

---

## 4. Product Problem Statement

**"What problem does Valori's graph solve that a vector database alone does not?"**

Evaluated without assuming GraphRAG is the answer, against the nine candidate directions:

| Direction | User problem solved | Why vectors alone are insufficient | Fits Valori's architecture today? |
|---|---|---|---|
| **A. Relationship store** | "This document supersedes that one," "this chunk belongs to that document," "this record contradicts that one" — explicit, typed facts a similarity score cannot express | Cosine/L2 similarity has no notion of *directed, typed* relationships; two contradictory records can be highly similar | **Yes — already the best-supported direction.** `Supersedes`/`Contradicts`/`ParentOf` edge kinds already exist and are used by the C4.2/C4.3 consolidation/contradiction endpoints per CLAUDE.md's SDK reference |
| **B. GraphRAG / knowledge retrieval** | Multi-hop questions ("who reports to the person who approved X?") that no single embedding captures | Single-vector retrieval flattens relational structure | **Partially.** The entity-extraction pipeline (§2) is real infrastructure toward this; full Microsoft-GraphRAG-style community summarization is not attempted and — per research (§10) — is expensive and not universally beneficial |
| **C. Vector → graph expansion** | "Show me not just the top-K similar chunks, but their document/section context" | Vector search returns isolated points with no structural context | **Yes — this is exactly what `/v1/graphrag` already does today** |
| **D. Graph → vector retrieval** | "Given this entity, find semantically related content across the whole namespace, not just its direct neighbors" | Pure graph traversal is bounded by explicit edges; misses semantic-but-unlinked matches | **Not implemented.** No endpoint starts from a graph node and fans out into vector similarity |
| **E. Recommendation / related entities** | "Users who engaged with X also engaged with Y" | Requires relationship data vectors don't carry | Partially — Weaviate's Ref2Vec (§10) is the closest external precedent; Valori has no equivalent today |
| **F. Multi-hop retrieval** | Chained reasoning across 2+ relationship hops | Single vector search is one hop by definition | Partially — `expand_subgraph`'s depth parameter supports this up to 4 hops, but with no relevance decay or path scoring |
| **G. Dependency/workflow graphs** | "What does this task depend on; what breaks if I remove it" | Needs directed acyclic structure with typed edges | Structurally supported (`ParentOf`, directed edges), but no cycle detection, no topological query, and no evidence this is a targeted use case |
| **H. General-purpose graph database** | Arbitrary graph modeling (social networks, org charts, arbitrary schemas) | N/A — this is graph-DB territory, not a vector-DB extension | **Explicitly a poor fit** — no query language, no property graph, and building one would compete directly with Neo4j-class systems on their own turf (§10) |
| **I. Hybrid vector + graph retrieval** | Combining semantic similarity with structural constraints in one query | Neither modality alone is sufficient for entity-disambiguation-heavy or compliance-heavy retrieval | **Yes — this is the union of C, D, and F**, and is where Valori's single-system determinism (§9) is a genuine, verifiable differentiator versus the Neo4j+Qdrant-pairing pattern most competitors use (§10) |

**`[RECOMMENDATION]`** The problem Valori's graph should be positioned to solve is **A + C + I**: typed relationship facts, vector-to-graph context expansion, and hybrid retrieval — because these are (a) already substantially implemented, (b) architecturally consistent with a deterministic single-system design, and (c) do not require Valori to become a general-purpose graph database (H, explicitly rejected) or commit to the heaviest form of GraphRAG (B, only partially justified — see §10's critical-perspective research).

---

## 5. Vector ↔ Graph Model

Current implementation is **Model B** (verified, not inferred):

```
Record
  ├── vector (FxpVector)
  ├── metadata (Option<Vec<u8>>)
  └── (no back-reference to any GraphNode)

GraphNode
  ├── independent NodeId
  └── record: Option<RecordId>   ← optional, one-directional pointer
```

Evaluating all four proposed models against the current architecture and G0's invariants:

| Model | Description | Advantages | Disadvantages |
|---|---|---|---|
| **A** — fully independent (`Record.Vector`, `GraphNode` with no linkage at all) | No linkage mechanism exists | Simplest possible separation; zero coupling | Loses the one thing that makes GraphRAG possible today — there would be no way to resolve a vector hit to its graph context. Not what's implemented, and regressing to it would delete real, working functionality. |
| **B** — optional linkage (current) | `Record` has a vector; `GraphNode` optionally points at a `Record` | Vectors and graph are each independently useful (a record needs no node; a node needs no record); linkage is opt-in per use case; matches the audit's "vector identity and graph identity are separate id spaces" finding | Reverse lookup (record → node) requires a scan (`resolve_seed_nodes` walks every live node, O(total_nodes) per GraphRAG call) or an `Engine`-level index (`record_to_node`, itself derived and standalone-only) — a real, measured cost at scale (see §12, indexing) |
| **C** — node references record (one-directional, node owns the pointer) | This is a restatement of B, viewed from the node's side | Same as B | Same as B — this is not actually a distinct model from B; the audit's own framing conflates "which struct holds the pointer" with "is the relationship optional," which are separate questions |
| **D** — `RecordId == NodeId` (shared identity space) | Every record IS a node; no separate allocation | Eliminates the reverse-lookup cost entirely — O(1) both directions; simpler mental model for "everything is a node" use cases | **Breaks a real, already-relied-upon invariant**: G0 confirmed records can exist without nodes and nodes can exist without records, and this is used today (bulk vector-only inserts never touch the graph; `Concept` nodes have no record). Forcing shared identity would mean either (a) every insert silently allocates a node too (defeats the "opt-in" design and doubles canonical-state growth for pure-vector workloads), or (b) two disjoint id spaces pretending to be one (fragile, error-prone). Also a strictly larger, riskier change than anything justified by the current evidence. |

**`[RECOMMENDATION]`** Preserve **Model B** as the stable conceptual model going forward: *"vectors and graphs are independently useful, with optional linkage."* This is not just the current implementation — it is the correct model for Valori's actual usage pattern (bulk vector-only ingestion coexisting with sparser, deliberate graph construction). The reverse-lookup cost in Model B (§ above) is a real, measurable performance concern for a future phase (an indexed, canonical-adjacent `record_to_node` structure, kept derived — never promoted to canonical state, consistent with G0's derived-state boundary), not an architecture defect requiring Model D.

---

## 6. Canonical vs. Derived Boundary (extended for future capabilities)

G0's boundary (`RecordPool`/`NodePool`/`EdgePool`/event log canonical; all indexes derived) is preserved and extended here to structures G1 might plausibly need:

| Structure | Canonical or derived? | In event log? | In state hash? | Survives snapshot/replay? | How rebuilt |
|---|---|---|---|---|---|
| Node/edge existence, identity, kind, from/to, record linkage | **Canonical** (unchanged from G0) | Yes | Yes (v3, G0.2) | Yes | N/A — it IS the source |
| **Edge properties** (if added — e.g. a weight or a free-form label) | **Canonical**, if added | Would need a new `KernelEvent` field/variant | Would need to join v3's coverage | Would need snapshot format work | N/A |
| **Custom relationship-type strings** (if the fixed enum is loosened) | **Canonical**, if added | Same as above | Same as above | Same as above | N/A |
| Adjacency acceleration (CSR, compressed adjacency) | **Derived** | No | No | No (or optionally cached in snapshot, like HNSW's index section — but never authoritative) | Rebuilt from `NodePool`/`EdgePool` in a single deterministic pass |
| BFS/DFS traversal caches | **Derived** | No | No | No | Recomputed per query; may be memoized outside canonical state |
| Shortest-path indexes | **Derived** | No | No | No | Recomputed or incrementally maintained, never authoritative |
| GraphRAG-style community/summary caches | **Derived** (already true today for label-propagation communities, G0 §4) | No | No | No | Recomputed on demand |
| Graph embeddings (e.g. node2vec-style vectors) | **Derived** | No | No | Optionally cached, never authoritative | Recomputed from canonical graph + model |
| Materialized neighborhoods / degree indexes | **Derived** | No | No | No | Rebuilt from `NodePool`/`EdgePool` |

**`[RECOMMENDATION]`** The only future addition that would plausibly *need* to enter canonical state is edge properties or a widened relationship-type vocabulary (§4's finding A), because those are *facts about the graph itself*, not acceleration structures. Everything else in Part 4/6/7's query-model and hybrid-retrieval discussion (§7, §8) should be derived, by the same logic G0.2 already applied to `hash_state_blake3`: canonical state commits to *facts*, not to *how a particular index chose to represent them*.

---

## 7. Query Model

Classified per the requested P0/P1/P2/NOT-A-FIT scale, evaluated against what's implemented today (§2) and the product directions retained in §4:

| Capability | Classification | Rationale |
|---|---|---|
| Direct neighbor lookup (outgoing/incoming) | **Already IMPLEMENTED** (`outgoing_edges`/`incoming_edges`) | — |
| N-hop / bounded traversal | **Already IMPLEMENTED** (`expand_subgraph`, capped at 4) | — |
| Filtered traversal (by relationship type) | **P0** | Directly needed by direction A (relationship store) — "give me only `Supersedes` edges" is a basic, expected query, and the fixed `EdgeKind` enum makes this cheap to filter on |
| Node filtering (by kind) during traversal | **P0** | Same rationale; `NodeKind` is already a filterable field on `list_nodes` (per CLAUDE.md's `kind` query param) but not on `expand_subgraph` |
| Vector → graph expansion | **Already IMPLEMENTED** (`/v1/graphrag`) | — |
| Graph → vector retrieval (direction D) | **P1** | Real, useful, moderate complexity (needs to gather a neighborhood's linked records, then run vector search scoped/boosted by that set) |
| Vector + graph hybrid retrieval (constraint-based, direction I) | **P1** | The union of existing capabilities plus filtering — high value, moderate complexity given the pieces already exist |
| Graph traversal + reranking | **P1** | Natural extension of the existing `/v1/graphrag` response — currently returns raw BFS output with no relevance ordering beyond BFS order |
| Degree queries (in-degree/out-degree of a node) | **P1** | Cheap (O(degree) walk of the existing adjacency lists), broadly useful for both product and debugging/observability |
| Path existence (is B reachable from A within N hops) | **P2** | Useful for dependency-graph-style use cases (direction G); moderate complexity, low urgency given no confirmed strong use case yet |
| Shortest path | **P2** | Same rationale as path existence, higher complexity (requires weighted or unweighted Dijkstra/BFS-shortest, plus a decision on tie-breaking determinism) |
| Subgraph extraction (arbitrary, not seed-anchored) | **P2** | Useful for visualization/export tooling, not core to any retained product direction |
| Connected components | **NOT A FIT** | This is graph-analytics territory (direction H, explicitly rejected in §4); no evidence of a product need, and it invites exactly the "general-purpose graph database" scope creep the audit rules warn against |
| Full graph query language (Cypher-equivalent) | **NOT A FIT** | Same rationale — this is what Neo4j is for (§10); building one is a multi-year commitment with no evidence of product demand, and it would compete with the vector-database-first identity Valori has already established (its own paper, §10, makes zero graph claims) |

---

## 8. Hybrid Retrieval Possibilities

Evaluating the four requested pipelines against what exists today:

| Pipeline | Problem solved | Complexity | Infrastructure required | Sufficiency of current graph |
|---|---|---|---|---|
| **A** — vector ANN → top-K → graph expansion → rerank | "Show me the semantically closest content plus its structural context" | Low-moderate | Already exists end-to-end except reranking (`/v1/graphrag` returns raw hits + subgraph; no rerank step combines them) | **Sufficient** — reranking is the only gap, and `valori-search`'s existing `ValoriReranker`/decay infrastructure (per CLAUDE.md) is a plausible reuse target, not new infrastructure |
| **B** — graph traversal → candidate records → vector similarity → rerank | "Start from a known entity, find semantically relevant content among its structural neighborhood" | Moderate | Needs a new "resolve node set → collect linked records → vector-score them" step; no such step exists today | **Not sufficient** — this is genuinely new work (direction D in §4, classified P1) |
| **C** — vector retrieval + graph constraints → hybrid candidate set → rerank | "Find semantically similar content, but only within/excluding certain relationship constraints" | Moderate-high | Needs constraint expression (e.g. "only records reachable from node X" or "only records NOT contradicted by anything") plus a way to apply it during or after vector search | **Not sufficient** — no constraint language exists; this is the most product-defining gap and deserves explicit design work before building, not incidental implementation during a later phase |
| **D** — graph neighborhood → vector similarity → semantic expansion | "Given a neighborhood, semantically expand beyond its explicit edges" | Moderate | Same underlying primitive as B, applied recursively/iteratively | **Not sufficient**, same gap as B |

**`[RECOMMENDATION]`** Pipeline A is nearly free (rerank-only gap) and should be first. B/D share one missing primitive ("resolve a node set to its linked records, then vector-score them") and should be built together. C is the highest-value, highest-risk pipeline — it requires a genuine design decision about what a "graph constraint" even means in Valori's model (a set of reachable node ids? a relationship-type predicate? both?) and should not be started until that's explicitly specified, not discovered mid-implementation.

---

## 9. Determinism Requirements

Every capability in §7/§8 evaluated against G0.1/G0.2's established contract (`events → replay → same canonical GraphState → same committed state hash`):

- **Already deterministic** (proven, G0.1/G0.2): node/edge creation/deletion, cascade delete, BFS traversal output (§9a of G0.1 — proven for a nontrivial graph, T1==T2==T3), the state hash itself.
- **Query operations that may safely be non-deterministic** (derived, read-only, never re-enter canonical state): traversal *internal implementation* details (e.g., which order a future CSR-backed index happens to store adjacency in memory) — as long as the *logical result set* (which nodes/edges are returned) is deterministic given the canonical graph and the query parameters. This is the same principle G0.2 established for the hash contract itself: commit to semantic content, not incidental reconstruction topology (§6 of this document; G0.2 §12 discussion).
- **Derived indexes that MUST still produce deterministic results**: any future adjacency-acceleration structure (CSR, compressed adjacency) must produce the same *query answers* as a direct walk of `NodePool`/`EdgePool`, regardless of what order it was built in — this is a correctness requirement on the index, not a determinism requirement on its internal representation.
- **Tie-breaking**: existing precedent is `(score, id)` ascending — used by the vector index's `SearchResult` ordering (`crates/valori-kernel/src/index/mod.rs`) and by BQ's candidate ranking. **`[RECOMMENDATION]`** any new graph-ranking capability (§7/§8's rerank steps, path-scoring) should adopt the same convention: primary sort by relevance/distance, secondary by ascending id, for reproducible output.
- **Traversal ordering**: `expand_subgraph`'s BFS order is a direct consequence of the edge adjacency list's construction order (most-recently-created-edge-first per node, since new edges prepend to the list head — G0's finding). This is deterministic but not intuitive (not creation order, not alphabetical, not any externally meaningful order). **`[RECOMMENDATION]`** this should be explicitly documented as "deterministic but implementation-defined" in any future query-model spec, so API consumers don't assume a stronger ordering guarantee than exists.
- **Duplicate edges**: G0.1 already established and tested the contract — duplicates are allowed, independently tracked, not deduplicated. Any future traversal/query capability must be designed against this reality (e.g., a "count of relationships" query returning duplicate-inclusive counts, or explicitly deduplicating and documenting that it does).
- **Multiple valid traversal paths**: not currently representable — `expand_subgraph` returns a flat node/edge set, not paths. Any future shortest-path or path-existence capability (§7, both P2) will need to decide whether multiple equally-short paths are all returned, or one is chosen deterministically (recommend the latter, via the same `(length, id)` tie-break convention) — **this decision should be made at design time for that specific phase, not now**, since no concrete requirement justifies it yet.
- **Can query results depend on derived-index construction order?** No, by the same principle as the state hash: two replicas building a traversal-acceleration index differently (e.g., different parallelism, different memory layout) must still return the same logical query answer for the same canonical graph and same query. This is a **new invariant this document proposes** for any future derived graph index, directly modeled on G0.2's hash-contract precedent.

---

## 10. Cloud/Product Implications

**`[CODE VERIFIED]`** Today, the graph shares the exact same scoping unit as vectors: `GraphNode.namespace_id`, and `Collection` (`crates/valori-metadata/src/collection.rs`) is defined as *"a named, isolated namespace of records within a project"* — mapping a name to a `NamespaceId`. There is no separate "graph collection" concept anywhere in the codebase; a graph node's scope is entirely determined by which collection's namespace it was created in.

Evaluating the five scoping options against this reality and against isolation/permissions/billing/UX:

| Option | Description | Fit |
|---|---|---|
| **A. Collection-scoped** | Graph lives inside a vector collection, sharing its namespace (current de facto reality) | **Best fit for today's architecture.** Isolation is already enforced at this level (namespace boundary = tenant boundary, per G0's namespace invariant). No new permission model needed — collection-level ACLs (whatever they are in Cloud) already cover the graph for free. |
| **B. Project-scoped** | One graph per project, spanning all collections | **Poor fit** — would require crossing namespace boundaries, which G0 found is explicitly *rejected* at the canonical mutation layer (`CreateEdge` refuses cross-namespace endpoints). Adopting this would be a real architectural change, not a product-layer decision. |
| **C. Independent graph collections** | A graph is its own first-class collection type, separate from vector collections, but still namespace-scoped | **Plausible future option**, not justified by current evidence — would require a new `Collection` variant/kind distinct from today's implicit "namespace holds both vectors and graph." Worth deferring until there's a concrete use case for a graph with no vector content at all (e.g., a pure relationship/workflow graph). |
| **D. Attached to vector collections** | Restatement of A, from the vector collection's point of view | Same as A |
| **E. Some combination** | Collection-scoped by default (A), with C available later if a pure-graph use case emerges | **`[RECOMMENDATION]`**: ship A now (zero new work — it's already true), explicitly leave the door open to C as a later, separately-justified addition, and never build B. |

**Billing implications** — `[UNKNOWN, not established by current code]`: nothing in this repository determines whether graph nodes/edges count toward a billing metric distinct from vector record count. This is squarely a Cloud control-plane decision (per `ownership.md`'s admission rule: billing concepts are Cloud-private and must never enter the OSS kernel) and out of scope for this document to decide. Flagging it as an open question for whoever owns Cloud pricing.

**User mental model implication**: keeping the graph collection-scoped (A) means a customer's mental model stays simple — "my collection has vectors and, optionally, a graph over some of them" — rather than introducing a second top-level resource type to reason about. This is consistent with §5's Model B recommendation (vectors and graph are independently useful, with optional linkage) applied one layer up, at the product/Cloud level.

---

## 11. Competitor / Research Findings

All claims below are tagged `[SOURCE FACT]` (verified via search/fetch in this session, with citation), `[MODEL INFERENCE]` (reasonable synthesis not independently re-verified), or `[RECOMMENDATION]`.

### The dominant pattern: paired systems, not unified ones

`[SOURCE FACT]` The most common production pattern today is **pairing** a vector database with a separate graph database — typically Qdrant (or similar) for vector similarity, Neo4j for relationship traversal, glued together at the application layer: a query is vectorized and searched in the vector store, the resulting record IDs are used to look up linked entities in the graph store, and results are combined and reranked ([Data Graphs / Qdrant case study](https://qdrant.tech/blog/case-study-datagraphs/); [rileylemm/graphrag-hybrid](https://github.com/rileylemm/graphrag-hybrid)).

`[MODEL INFERENCE]` This pairing pattern is exactly what Valori's single-system architecture avoids by construction — there is no cross-system round-trip, no synchronization problem, and (per G0's GraphRAG findings) both the vector KNN and the graph BFS read the *same consistent kernel snapshot* in one call. This is a real, defensible differentiation point, not a marketing claim — it follows directly from the canonical-state architecture G0 audited.

### Vector databases adding graph-lite features

- **Weaviate**: [cross-references](https://docs.weaviate.io/weaviate/manage-collections/cross-references) — directional links between objects/collections, used for retrieval, not vectorized themselves. **Ref2Vec** — vectorizes an object *from* the centroid of its cross-referenced neighbors' vectors, used for recommendation ([Weaviate blog](https://weaviate.io/blog/ref2vec-centroid)). `[SOURCE FACT]` This is the closest existing precedent to Valori's direction E (recommendation/related entities, §4) — a graph-lite feature bolted onto a vector-first system, not a full graph database.
- **TigerGraph**: added native vector search (TigerVector) to its existing graph-native core (`[SOURCE FACT]`, per search results) — the *opposite* direction from Valori (graph DB adding vectors, vs. vector store with native graph).
- **Neo4j**: graph-native, added vector indexes; markets a "Hybrid GraphRAG architecture" combining vector embeddings with graph structure, official `neo4j-graphrag-python` package exists ([Neo4j docs](https://www.neo4j.com/docs/neo4j-graphrag-python/current/user_guide_rag.html)). Same "graph DB adding vectors" direction as TigerGraph.
- **Samyama** (academic/research system, arXiv 2603.08036): a from-scratch unified graph-vector database with vectors embedded at the node level, CSR-based analytics engine, cost-based query planner. `[SOURCE FACT, per fetched abstract]` — the closest architectural precedent to "one deterministic system, not a pairing," though Samyama does not claim determinism/reproducibility as a design goal the way Valori does.
- **Milvus, Pinecone, LanceDB**: pure vector databases; no native graph capability found in current sources (`[SOURCE FACT]`, per search results — "these are vector databases optimized for semantic similarity search, not traditional graph databases").

### GraphRAG specifically

`[SOURCE FACT]` Microsoft's GraphRAG pattern (as commonly described in current sources, e.g. [Ideasthesia summary](https://www.ideasthesia.org/microsoft-graphrag-architecture-and-lessons-learned/)) is a heavy pipeline: LLM-driven entity/relationship extraction, Leiden-algorithm community detection, hierarchical community summarization, and a two-tier (entity-level + community-level) index used for "local" vs. "global" search. This is substantially more than Valori's current entity-extraction endpoint (§2) — Valori extracts entities/relationships and inserts them as real graph nodes/edges, but does not build community summaries or a global/local search split.

`[SOURCE FACT]` A 2026 paper explicitly questions the universal value of this pattern: *"Do We Still Need GraphRAG? Benchmarking RAG and GraphRAG for Agentic Search Systems"* (arXiv 2604.09666) exists and directly investigates whether GraphRAG's added complexity is justified versus plain RAG in agentic settings. **This document could not extract the paper's specific quantitative conclusions** (the fetch tool could not parse the full PDF content) — flagging its existence and topic only, not its findings, per the instruction to state "not established by the current code/source" rather than guess. What general industry commentary consistently agrees on (`[SOURCE FACT]`, seen across multiple 2025–2026 sources): GraphRAG-style construction has real upfront indexing cost, and the recommended practice is to start with vector search + reranking and add graph/GraphRAG only when multi-hop questions, entity disambiguation, or compliance requirements actually demand it — not by default.

### Determinism/verifiability as a differentiator

`[SOURCE FACT]` Valori itself already has a public paper — *"Valori: A Deterministic Memory Substrate for AI Systems"* (arXiv 2512.22280, Varshith Gudur) — which makes **no graph claims at all**. Its abstract and content describe a purely vector-based deterministic memory system (Q16.16 fixed-point, bit-identical states/snapshots/search results across platforms). `[MODEL INFERENCE]` This means the graph capability, however real and tested (§2), is currently **unannounced in Valori's own public academic positioning** — this is either a timing artifact (the paper predates the graph work) or a deliberate scoping choice; this document cannot determine which, and flags it as a decision for whoever owns Valori's public narrative, not something to assume either way.

`[SOURCE FACT]` Other projects in the "deterministic AI memory" space exist and make similar reproducibility claims (Memvid V2, DMF — per search results) — none found claim graph capabilities either; determinism and graph richness appear to be pursued as largely separate concerns across the field, not commonly combined. `[MODEL INFERENCE]` This suggests Valori combining both (deterministic canonical state + a real, if narrow, graph) is not something a direct competitor is currently doing — a genuine, if narrow, differentiation opportunity, though this document has not found evidence of market demand for that specific combination, only its absence among what was found.

### What's overkill for Valori

`[RECOMMENDATION]`, synthesizing the above:
- A full graph query language (Cypher-equivalent) — Neo4j's territory, no evidence of demand, contradicts Valori's own vector-database-first public positioning.
- Full Microsoft-GraphRAG-style community detection + hierarchical summarization as a default pipeline — real research questions its universal value (§10 above), and it would be a large new capability (LLM summarization at index time) with no current implementation to build from.
- Hardware-accelerated graph analytics (Samyama's GPU/TPU direction) — no evidence this is a bottleneck at Valori's current or near-term scale.

### Where Valori could differentiate

`[RECOMMENDATION]`: not "having a graph" (many systems now have graph-lite features) — the differentiator is that Valori's graph is **canonical, event-sourced, and BLAKE3-committed in the same system as the vectors**, provably (G0.1/G0.2), where every competitor found here either pairs two systems (Qdrant+Neo4j pattern) or bolts graph-lite features onto a system with no equivalent determinism/audit story. This is a narrow but real and defensible claim, consistent with what the audit trail (G0→G0.2) actually proved, not aspirational.

---

## 12. Recommended G1 Architecture Contract

1. **What is Valori's graph?** A canonical, typed, directed, namespace-scoped relationship store over records and structural concepts — see §3.
2. **What is it NOT?** A general-purpose graph database, a property graph, a system with a query language, or a default participant in every ingestion (entity extraction remains opt-in).
3. **What problem does it solve?** Typed relationship facts (§4-A), vector-to-graph context expansion (§4-C), and hybrid vector+graph retrieval (§4-I) — not general graph analytics (§4-H, rejected) and not, by default, the heaviest form of GraphRAG (§4-B, only partially justified).
4. **What is canonical?** Node/edge existence, identity, kind, endpoints, record linkage — unchanged from G0. If edge properties or custom relationship types are ever added, they join canonical state (§6).
5. **What is derived?** All acceleration structures — adjacency indexes, traversal caches, shortest-path indexes, community/summary caches, graph embeddings (§6) — never authoritative, always rebuildable, never entering the event log or the state hash unless they represent a new *fact*, not a new *index*.
6. **How does it relate to vectors?** Model B (§5): independently useful, optionally linked, one-directional pointer from node to record. Do not adopt Model D (shared identity).
7. **What query model should eventually exist?** §7's P0/P1 items (filtered traversal, node/kind filtering, graph→vector retrieval, hybrid retrieval, reranking, degree queries) — not §7's NOT-A-FIT items (connected components, full query language).
8. **What must remain deterministic?** Every canonical mutation and its replay (unchanged, G0.1/G0.2). Every derived index's *logical query results*, regardless of the index's internal construction order (§9, new invariant this document proposes, modeled on G0.2's hash-contract precedent).
9. **What belongs in the kernel?** Only canonical data and its direct mutation/read primitives (unchanged, `no_std`-constrained). Never traversal logic, never LLM calls, never HTTP.
10. **What belongs in `valori-node`?** Traversal algorithms (BFS today; any future DFS/shortest-path/filtered traversal), the entity-extraction pipeline, hybrid-retrieval orchestration, reranking — all std-only, all read-mostly against `KernelState`.
11. **What belongs in Cloud?** Nothing established by current code (§10) — this document explicitly declines to decide Cloud-specific scoping/billing questions it has no evidence for, beyond recommending collection-scoping (§10-A) as the default that requires zero new architecture.
12. **What should never enter the kernel?** Any LLM/embedding call, any HTTP concern, any billing/tenant concept (per `ownership.md`'s existing, correct rule), any traversal algorithm beyond the primitive adjacency reads that already exist.
13. **What should never become canonical state?** Any acceleration/index structure (§6), any derived community/summary output, any per-query-session state, any cache — the same boundary G0 already drew for vector indexes, extended to graph indexes.

---

## 13. Explicit Non-Goals

- A full property-graph data model (edge properties are a *possible* future canonical addition, §6 — but not a commitment made here).
- A graph query language.
- Automatic, always-on entity extraction during default ingestion (the existing endpoint stays opt-in unless a future phase makes an explicit, separately-justified case for the default-on behavior).
- Full Microsoft-GraphRAG-style community summarization as a default pipeline.
- Cross-namespace/cross-collection graphs (violates G0's namespace invariant; would require a real architectural change, not a feature addition).
- Hardware-accelerated graph analytics.
- Competing with Neo4j/TigerGraph on general graph-database functionality.
- Any Cloud billing/provisioning decision — explicitly deferred to whoever owns that surface.

---

## 14. Proposed G1 Roadmap

Derived from this analysis, not the example template in the prompt — reordered and rescoped based on what's actually justified (§4, §7, §8) versus merely plausible.

### G1.1 — Graph query primitives *(P0 work; smallest, safest next step)*

- **Goal**: close the P0 gaps in §7 — relationship-type-filtered traversal, node-kind-filtered traversal on `expand_subgraph`.
- **Why it exists**: directly needed by the retained "relationship store" product direction (§4-A); zero new architecture, pure additive filtering on an already-correct BFS.
- **Code areas touched**: `crates/valori-rag/src/graph.rs` (`expand_subgraph` signature gains optional filters), `crates/valori-node/src/routes/graph.rs` and both routers (new query params).
- **Canonical vs. derived changes**: none — this is a read-path filter, no new canonical state.
- **Determinism implications**: none beyond what's already proven (filtering a deterministic BFS output is trivially still deterministic).
- **Testing requirements**: filtered-traversal correctness tests (kind/type combinations), a determinism-repeatability test analogous to G0.1's `traversal_output_is_deterministic_across_repeated_runs`.
- **Performance benchmarks**: not required — no new algorithmic complexity.
- **Must be proven before moving on**: filters produce correct, deterministic results on the existing test graphs; no regression in unfiltered behavior.

### G1.2 — Graph → vector retrieval primitive

- **Goal**: the missing "resolve a node set → collect linked records → vector-score them" primitive identified in §8 (shared by pipelines B and D).
- **Why it exists**: unblocks direction D (§4) and pipelines B/D (§8), both classified P1.
- **Code areas touched**: new function in `valori-rag` alongside `expand_subgraph`; new endpoint(s) in `valori-node` routes (both paths).
- **Canonical vs. derived changes**: none.
- **Determinism implications**: must define tie-breaking for the vector-scoring step (recommend the existing `(score, id)` convention, §9).
- **Testing requirements**: correctness (right record set retrieved from a given neighborhood), determinism, and a namespace-isolation test (neighborhood resolution must not leak cross-namespace, consistent with G0's invariant).
- **Performance benchmarks**: needed here, unlike G1.1 — this touches the O(total_nodes) `resolve_seed_nodes` scan pattern (§5); should be benchmarked before deciding whether an indexed `record_to_node` structure (derived, never canonical) is justified.
- **Must be proven before moving on**: the primitive is correct and namespace-safe; a data point on whether the O(total_nodes) scan is acceptable at realistic scale or needs the indexed follow-up.

### G1.3 — Reranking for `/v1/graphrag`

- **Goal**: pipeline A (§8) — the nearly-free gap. Rerank the existing `/v1/graphrag` response instead of returning raw BFS order.
- **Why it exists**: highest value-to-effort ratio identified in this analysis; reuses existing `valori-search` reranking infrastructure rather than building new.
- **Code areas touched**: `crates/valori-node/src/server.rs`/`cluster_server.rs` GraphRAG handlers; possibly `valori-search`.
- **Canonical vs. derived changes**: none — reranking is purely a response-shaping step.
- **Determinism implications**: rerank scoring must be deterministic and tie-broken consistently (§9).
- **Testing requirements**: reranked output determinism; regression tests confirming raw hits/subgraph data is unchanged, only ordering/annotation changes.
- **Performance benchmarks**: minor — reranking a bounded (depth-4-capped) subgraph is cheap.
- **Must be proven before moving on**: reranked results are deterministic and demonstrably more useful than raw BFS order (needs a concrete before/after example, not just "it runs").

### G1.4 — Hybrid retrieval with graph constraints (pipeline C)

- **Goal**: the highest-value, highest-risk pipeline from §8 — vector retrieval constrained/filtered by graph structure.
- **Why it exists**: completes direction I (§4), the strongest differentiation claim from §10's research.
- **Explicitly requires a design sub-phase before implementation**: what a "graph constraint" means in Valori's model must be specified first (reachability set? relationship-type predicate? both? — §8 flagged this as undecided). **This is itself a candidate for a G1.4.0 design-only step, mirroring how this document (G1.0) preceded G1 implementation.**
- **Code areas touched**: TBD, pending the design sub-phase.
- **Canonical vs. derived changes**: none expected, but cannot be confirmed until the constraint model is specified.
- **Determinism implications**: TBD, pending design.
- **Testing/benchmark requirements**: TBD, pending design.
- **Must be proven before moving on**: a specified, reviewed constraint model exists before any code is written.

### Deliberately not scheduled in G1

- Edge properties / custom relationship types (§6) — real future work, but no concrete requirement surfaced in this analysis to justify prioritizing it now. Revisit if a specific product need (e.g., weighted relationships) emerges.
- Full entity-extraction-on-default-ingest (§13) — opt-in stays opt-in until separately justified.
- Everything in §7's P2/NOT-A-FIT rows and §13's non-goals.

---

## 15. Risks and Open Product Decisions

- **The O(total_nodes) `resolve_seed_nodes` scan** (§5, §12/G1.2) is a real, unaddressed performance question at scale — not urgent today, but should be measured before G1.2 ships, not discovered in production.
- **Whether entity extraction should ever become default-on** is a genuine open product decision this document does not resolve — it has real value (§2, already working) but real cost (LLM calls on every ingest) and no evidence either way was found for what customers want.
- **Cloud billing/scoping** (§10) is entirely unresolved by this document, by design — it is not visible from this repository and must be decided by whoever owns that surface.
- **Whether to update Valori's public positioning** (its own paper, §10, makes no graph claims) to include the graph capability is a real, open decision outside this document's scope.
- **Pipeline C's constraint model** (§14/G1.4) is undesigned and should not be estimated or scheduled until it is.
- **This document's competitor research has one unverified gap**: the specific quantitative findings of arXiv 2604.09666 ("Do We Still Need GraphRAG?") could not be extracted — its existence and general topic are cited, its conclusions are not, and should not be treated as established by this document.

---

## 16. G1 Entry Criteria

Before any G1 phase begins implementation, the following should be explicitly confirmed by the team (not by this document, which can only propose):

- [ ] §11's 13-point contract is approved as-written, or amended with explicit reasoning for any change.
- [ ] §13's non-goals are agreed, especially the rejection of a general-purpose graph query language and default-on entity extraction.
- [ ] §12's canonical/derived extension is approved, especially the new "derived indexes must produce deterministic logical results regardless of construction order" invariant.
- [ ] G1.1 (query primitives) specifically is approved to start, since it is the only roadmap item with zero open design questions.
- [ ] G1.4's design sub-phase is explicitly scheduled before G1.4 implementation is estimated or started.

---

## Verdict

**G1 NOT READY** as a single, monolithic implementation phase — §14's roadmap deliberately has items (G1.4) with unresolved design questions that must not be implemented against guesses.

**G1.1 (graph query primitives) IS READY** to start immediately upon explicit approval of this contract — it has no open design questions, touches no canonical state, and is the smallest, safest, most directly justified next step from this entire analysis.

Recommended sequencing: approve this document → start G1.1 → benchmark the G1.2 scan-cost question in parallel → schedule G1.4's design sub-phase once G1.1/G1.2/G1.3 have landed and there is real usage data to inform the constraint model, rather than designing it speculatively now.
