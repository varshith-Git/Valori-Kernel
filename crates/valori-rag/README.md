# valori-rag

Retrieval-Augmented Generation primitives for the Valori platform.

Three RAG modalities in one crate — all pure computation over `KernelState`, no HTTP coupling:

| Module | What it does |
|--------|-------------|
| `graph` | GraphRAG: vector KNN + BFS subgraph expansion; shared by both routers |
| `tree` | Tree-RAG: hierarchical markdown indexing with BLAKE3 receipt chaining |
| `community` | Community Layer: Label Propagation detection + cosine community search |
| `llm` | Minimal LLM HTTP wrapper for entity extraction (OpenAI / Ollama) |

## Architecture

```
valori-node (owns HTTP routes)
    └── valori-rag (pure RAG logic)
            └── valori-kernel (KernelState, NodeId, FxpScalar)
```

No circular dependencies. `valori-rag` knows nothing about axum routing beyond the two stateless handlers it exports (`tree_verify`, `tree_chain_verify`). Both handlers are zero-argument — they compile into both `server.rs` and `cluster_server.rs` without modification.

## Modules

### `graph` — GraphRAG

```rust
use valori_rag::graph::{resolve_seed_nodes, expand_subgraph, expand_subgraph_budgeted};

// Resolve record_ids → node_ids (O(N) kernel scan)
let seeds = resolve_seed_nodes(&kernel_state, &record_ids);

// BFS subgraph expansion (depth clamped to MAX_DEPTH=4; no node/edge budget)
let (nodes, edges) = expand_subgraph(&kernel_state, &seed_node_ids, 2);

// Phase 5.4: BFS with hard stops on nodes and edges visited
let (nodes, edges) = expand_subgraph_budgeted(
    &kernel_state, &seed_node_ids, 2,
    Some(500),  // max_nodes: halt before visiting >500 nodes
    Some(2000), // max_edges: halt edge emission once >2000 edges emitted
);
```

Invariants:
- Both functions take `&KernelState` — no engine lock needed; cluster path reads from its local snapshot.
- `expand_subgraph` is de-duplicated: a node appears exactly once even if reachable from multiple seeds.
- `MAX_DEPTH = 4` is a hard cap against hostile clients fanning out the whole graph.
- `expand_subgraph` is a zero-budget wrapper over `expand_subgraph_budgeted(state, seeds, depth, None, None)`.
- `expand_subgraph_budgeted` returns early once either budget is hit; edges in `edges_out` always reference a node in `nodes_out` (from-node invariant holds) but destination nodes may be absent when the node budget ran out before they were processed.

### `tree` — Tree-RAG

```rust
use valori_rag::tree::{TreeIndex, Receipt, GENESIS, verify_chain};

// Build from markdown — zero LLM, pure header parsing
let tree = TreeIndex::from_markdown(doc_text, "my-doc");

// Navigate deterministically (term-frequency scoring over ToC)
let result = tree.answer("how many sick days", 2, GENESIS);

// Verify tamper-evidence
assert!(tree.verify_receipt(&result.receipt));

// Chain verify across multiple queries
assert!(verify_chain(&[receipt_a, receipt_b]));
```

Receipt chain mirrors the kernel's BLAKE3 `events.log` — each retrieval seals the previous receipt's hash. Tampering with stored section text is detectable on replay.

### `community` — Community Layer

```rust
use valori_rag::community::{label_propagation, build_community_store, rank_communities};

// Run Label Propagation (O(n + e) per iteration, deterministic min-label tie-break)
let assignments = label_propagation(&kernel_state, None, 20);

// Build store with centroids + BLAKE3 receipt
let store = build_community_store(&kernel_state, assignments);

// Cosine-rank communities against a query vector
let hits = rank_communities(&store, &query_vec, 5);
```

### `llm` — Entity extraction

```rust
use valori_rag::{LlmConfig, extract_entities_via_llm};

let cfg = LlmConfig {
    provider: "openai".to_string(),
    model: "gpt-4o-mini".to_string(),
    url: "https://api.openai.com".to_string(),
    api_key: Some("sk-...".to_string()),
};

let output = extract_entities_via_llm(
    "Alice works at Acme Corp.",
    &[],  // defaults to PERSON, ORGANIZATION, CONCEPT, LOCATION, EVENT
    &cfg,
    None,
    &http_client,
).await?;
```

`LlmConfig` mirrors the 4 fields of `valori-node`'s `EmbedConfig` that entity extraction needs. The node constructs `LlmConfig` from its `EmbedConfig` at the call site — no circular dependency.

## Design invariants

- **No `valori-node` dependency.** `valori-rag` must never depend on `valori-node` — that would be circular.
- **Stateless handlers compile into both routers.** `tree_verify` and `tree_chain_verify` are `axum::Json` handlers with no `State<>` parameter.
- **Pure computation only.** No file I/O, no spawning, no global state. All functions take explicit references.
- **BLAKE3 everywhere.** Graph receipts, tree receipts, and community receipts all use BLAKE3 so the same verifier binary can check all three.

## Scalability notes

| Operation | Complexity | Notes |
|-----------|-----------|-------|
| `resolve_seed_nodes` | O(N nodes) | One kernel scan; no index |
| `nodes_referencing_record` | O(N nodes) | One kernel scan; ascending `NodeId` order. Used by `Engine::delete_record`/cluster's `RecordOps::delete` (G1.3.1) to cascade-delete every graph node referencing a hard-deleted record — measured 791ns@1K, 82.2µs@100K, 1.61ms@1M nodes, well under the cost of the Raft round trip it gates. |
| `expand_subgraph` | O(V + E) BFS | Bounded by `MAX_DEPTH`; calls `expand_subgraph_budgeted` with no budget |
| `expand_subgraph_budgeted` | O(min(V,max_nodes) + min(E,max_edges)) BFS | Phase 5.4: hard stops on nodes and edges; early-exit when either budget is reached |
| `graph_distances_from_seeds` | O(V + E) multi-source BFS | Bounded by `MAX_DEPTH`; Phase G1.4.1's graph-aware reranking signal. Measured 1.2µs@1K/9.6µs@10K/168µs@100K (1 seed, depth 2), scaling with reachable-set size the same way `query_graph`/`expand_subgraph` already do (G1.2), not with total graph size. |
| `label_propagation` | O((N + E) × iters) | Typically < 10 iterations |
| `build_community_store` | O(N × dim) | Centroid average per community |
| `rank_communities` | O(C × dim) | Cosine over C centroids |
| `tree_answer` | O(nodes × query_terms) | Term-frequency over ToC |

## Usage

```toml
[dependencies]
valori-rag = { workspace = true }
```

For integration tests that need both kernel state and RAG:

```toml
[dev-dependencies]
valori-kernel = { workspace = true, features = ["std"] }
valori-rag = { workspace = true }
```

## The `utoipa` feature (Phase API-3.1)

Optional and **off by default** — nothing in the runtime path needs it, and
enabling it adds a dependency the shipped binary does not carry.

```toml
utoipa = ["dep:utoipa"]
```

`valori-node`'s own `utoipa` feature turns it on transitively. It adds
`#[derive(ToSchema)]` to the `tree::*` and `community::*` request/response types, which `valori-node` serialises verbatim from
`/v1/tree/*`, `/v1/community/*`, `/v1/ingest/extract-entities`.

The point is that there is **one** type. The public OpenAPI contract references
the same struct the handler returns, so a field added or renamed here shows up
in the contract automatically instead of drifting away from a hand-copied mirror
in `valori-node/src/api.rs`. `scripts/verify-api-route-contract.py` and the
byte-equality test in `crates/valori-node/tests/openapi_generated.rs` enforce it.

