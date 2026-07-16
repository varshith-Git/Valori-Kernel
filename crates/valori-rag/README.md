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
use valori_rag::graph::{resolve_seed_nodes, expand_subgraph};

// Resolve record_ids → node_ids (O(N) kernel scan)
let seeds = resolve_seed_nodes(&kernel_state, &record_ids);

// BFS subgraph expansion (depth clamped to MAX_DEPTH=4)
let (nodes, edges) = expand_subgraph(&kernel_state, &seed_node_ids, 2);
```

Invariants:
- Both functions take `&KernelState` — no engine lock needed; cluster path reads from its local snapshot.
- `expand_subgraph` is de-duplicated: a node appears exactly once even if reachable from multiple seeds.
- `MAX_DEPTH = 4` is a hard cap against hostile clients fanning out the whole graph.

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
| `expand_subgraph` | O(V + E) BFS | Bounded by `MAX_DEPTH` |
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
