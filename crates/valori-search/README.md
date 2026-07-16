# valori-search

Post-retrieval search primitives for the Valori platform.

Three independent, pure modules — no kernel, no engine, no I/O.

## Modules

| Module | What it does |
|--------|--------------|
| `decay` | Time-decay re-ranking — penalises stale records by inflating their L2 distance using a geometric half-life |
| `reranker` | BM25 Okapi hybrid reranker — blends vector similarity with term-frequency scoring for lexical precision |
| `filter` | Metadata predicate matching — exact equality and numeric range operators (`gt`, `gte`, `lt`, `lte`, `eq`) |

## Usage

```toml
[dependencies]
valori-search = { workspace = true }
```

### Decay re-ranking

```rust
use valori_search::{DecayHit, decay_rerank};

let hits = vec![
    DecayHit { id: 1, distance: 1.0, created_at: Some(now - 3600) }, // 1 hour old
    DecayHit { id: 2, distance: 1.2, created_at: Some(now) },        // brand new
];
// half_life = 1800 s (30 min) → record 1 is 2 half-lives old, factor ≈ 0.25
let ranked = decay_rerank(hits, now, 1800, 2);
assert_eq!(ranked[0].id, 2); // fresh record overtakes the older better match
```

### BM25 hybrid reranking

```rust
use valori_search::ValoriReranker;

let mut r = ValoriReranker::new();
r.insert(42, "The optimizer used is AdamW with weight decay");
r.insert(99, "Reinforcement learning uses Adam in single epoch");

// Vector search returns k × POOL_FACTOR candidates; reranker returns top-k.
let candidates = vec![(99, 1.0_f32), (42, 1.5_f32)]; // (record_id, l2_distance)
let reranked = r.rerank("AdamW optimizer", candidates);
assert_eq!(reranked[0].0, 42); // exact term match rises to top
```

### Metadata filter

```rust
use valori_search::matches_metadata_filter;
use serde_json::json;

let meta = json!({"author": "Alice", "year": 2022});
let filter = serde_json::from_value(json!({"author": "Alice", "year": {"gte": 2020}})).unwrap();
assert!(matches_metadata_filter(&meta, &filter));
```

## Design invariants

- **No kernel dependency.** Operates on raw `f32`, `u32`, `u64`, and `serde_json` values.
- **No I/O, no async.** Every function is synchronous and pure.
- **Deterministic output.** Tie-breaking uses record ID ascending.
- **Inverted index for BM25.** `ValoriReranker` maintains `doc_freq` incrementally — IDF lookup is O(1) per query term, not O(|corpus|).

## Scalability notes

| Operation | Complexity |
|-----------|-----------|
| `ValoriReranker::insert` | O(tokens in document) |
| `ValoriReranker::remove` | O(tokens in document) |
| `ValoriReranker::rerank` | O(candidates × query_terms) |
| `decay_rerank` | O(n log n) sort |
| `matches_metadata_filter` | O(filter_keys) |

At `k = 100` candidates and `POOL_FACTOR = 20`, rerank processes at most 2 000 candidates. This is negligible even at millions of records.
