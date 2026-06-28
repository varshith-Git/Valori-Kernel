# Phase I5 — Tree-RAG: gap-fill (cache + hybrid retrieval)

## Goal

Harden the existing Tree-RAG prototype (stateless handlers, no cache) into a production-grade retrieval system: server-side tree cache to avoid re-transmitting the full tree on every query, and a hybrid `/v1/tree/hybrid` endpoint that fuses tree-navigation scores with vector similarity scores in a single call.

## Delivered

| File | What changed |
|---|---|
| `crates/valori-node/src/tree_rag.rs` | `BuildResponse` now includes `cache_key: String`; `QueryRequest.tree` is now `Option<TreeIndex>` with a new `cache_key: Option<String>` field; added `HybridHit`, `HybridRequest`, `HybridResponse` types; added `TreeIndex::rank_nodes_normalized()` (normalises raw TF scores to [0, 1]); `tree_verify` kept stateless; `tree_build`/`tree_query` removed from this file (moved to server layers as stateful handlers); exported `hash_text()` and `GENESIS` constant |
| `crates/valori-node/src/engine.rs` | `Engine` struct gains `tree_cache: HashMap<String, TreeIndex>`; new methods `cache_tree(&str, TreeIndex) -> String` and `get_cached_tree(&str) -> Option<&TreeIndex>` |
| `crates/valori-node/src/server.rs` | Routes updated: `/v1/tree/build`, `/v1/tree/query`, `/v1/tree/hybrid` are now stateful handlers taking `State<SharedEngine>`; `/v1/tree/verify` remains stateless; three handler functions (`tree_build`, `tree_query`, `tree_hybrid`) appended at end of file |
| `crates/valori-node/src/cluster_server.rs` | `DataPlaneState` gains `tree_cache: Arc<tokio::sync::RwLock<HashMap<String, TreeIndex>>>`; initialised to empty `HashMap` in `build_cluster_router_with_keys`; routes updated to match standalone; three cluster-path handlers (`cluster_tree_build`, `cluster_tree_query`, `cluster_tree_hybrid`) appended at end of file |
| `python/valoricore/remote.py` | `SyncRemoteClient.tree_hybrid()` and `AsyncRemoteClient.tree_hybrid()` added |

## Findings

- **Tree cache is node-local** — not Raft-replicated. Trees are deterministic from source text, so any peer that receives a `/v1/tree/build` call re-derives an identical tree locally and caches it independently. Cross-node cache misses are harmless: the fallback is either the full `tree` body in the request or an HTTP 404 prompting the caller to rebuild.
- **Cluster vector search** in `cluster_tree_hybrid` uses `sm.with_state(|kernel| ...)` (the established cluster read pattern) rather than a direct engine lock, preserving the no-direct-engine-lock invariant for cluster handlers.
- **Namespace resolution** in the cluster hybrid handler uses the std `Mutex` (not `tokio::sync::Mutex`) that `namespaces` already uses — `.lock().unwrap()` is correct here, not `.lock().await`.
- **Hybrid scoring**: tree scores normalized by `score / max_score` (max raw score ≥ 1e-9 to avoid div/0); vector distances converted to similarity by `1 - dist / max_dist`; combined = `tree_weight * tree_score + (1 - tree_weight) * vec_score`. Both dimensions on [0, 1].

## Validation

```
cargo test -p valori-kernel -p valori-node
```

**259 tests — 0 failed.**

Manual smoke (stateless verification path unchanged):
```bash
curl -X POST http://localhost:3000/v1/tree/build \
  -H 'Content-Type: application/json' \
  -d '{"text":"# Intro\nHello\n## Details\nWorld","doc_name":"test"}' \
  | python3 -m json.tool   # → cache_key, node_count, structure_map, tree
```

## Follow-ups

| Item | Phase |
|---|---|
| Periodic LRU eviction for the tree cache (unbounded growth if many large docs are built) | I6 or maintenance |
| Cross-node cache broadcast via Raft event (optional; single-node cache-miss fallback is already robust) | I6+ |
| `tree_hybrid` UI tab in the collection page | UI backlog |
| Tree cache size / entry count exposed in `/metrics` | Observability backlog |
