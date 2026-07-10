# Valori — LLM project map

---

## Behavioral Guidelines (LLM Coding Standards)

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

### 1. Think Before Coding
**Don't assume. Don't hide confusion. Surface tradeoffs.**
Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

### 2. Simplicity First
**Minimum code that solves the problem. Nothing speculative.**
- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

### 3. Surgical Changes
**Touch only what you must. Clean up only your own mess.**
When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

### 4. Goal-Driven Execution
**Define success criteria. Loop until verified.**
Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.

---

## MANDATORY: after every phase is complete

Do ALL of these before reporting the phase as done. No exceptions.

1. **Create phase doc** — `docs/phases/phase-X.Y-short-name.md` using the 5-section template:
   - **Goal** — what this phase was supposed to achieve (1-2 sentences)
   - **Delivered** — what actually landed, file by file
   - **Findings** — bugs and design gaps found during the work
   - **Validation** — test counts, pass/fail, manual smoke test steps
   - **Follow-ups** — anything deferred, and which future phase owns it

2. **Update the status table** — `docs/phases/README.md` — add a row with the phase number, link to the doc, branch/commit, and ✅ done

3. **Update crate READMEs** — every crate whose files were touched gets its README updated with the new feature, endpoint, config, or invariant

4. **Update main `README.md`** — if the phase added anything user-visible (new endpoint, new env var, new CLI command), add it there

5. **Run tests and record counts** — `cargo test -p valori-kernel -p valori-node` — put the pass count in the phase doc Validation section

6. **Update `CHANGELOG.md`** — add the phase's deliverables under `[Unreleased]` or promote to a version entry

---

Read this first. It replaces cold-start greping for structure, invariants, and commands.

---

## Crate layout

| Crate | One-liner |
|---|---|
| `crates/valori-kernel` | The deterministic core: fixed-point vector store, knowledge graph, BLAKE3 audit chain, snapshot encode/decode. `no_std`. |
| `crates/valori-core` | Zero-dependency `no_std` type foundation (shared IDs, error types, traits). Every other crate depends on this; it depends on nothing except `serde` + `thiserror`. |
| `crates/valori-wire` | Shared serialization types (serde structs) + V2/V3/V4 event-log format (encode/decode/chain). Used by node ↔ Python SDK ↔ CLI. |
| `crates/valori-storage` | Durable storage layer: WAL, append-only event log (V4), object-store backend (S3/file). Persistence primitives only; recovery orchestration lives in `valori-state`. |
| `crates/valori-state` | State lifecycle orchestration: transitions `KernelState` between durable storage and in-memory operation (snapshot restore, WAL replay). |
| `crates/valori-metadata` | Control-plane persistence (redb): project config, collection name mappings, shard topology, snapshot catalog, execution history, planner cache. |
| `crates/valori-planner` | Operation lifecycle + execution planning: turns `Operation` + `PlanningContext` into a DAG of `TaskSpec`s; two-layer cache (in-process + `MetadataDb`). Wired via `run_graph_inline` into `POST /v1/records` (standalone path, Phase A12). |
| `crates/valori-effect` | Effect system: `EffectBus` routes kernel writes, receipt fragments, audit entries, and metrics from task execution. Defines the seven capability traits. Live for `POST /v1/records` (standalone path, Phase A12). |
| `crates/valori-consensus` | Raft state machine + log store (openraft 0.9). Wraps the kernel as an openraft `RaftStateMachine`. Multi-shard: one `ValoriStateMachine` per `ShardId`. |
| `crates/valori-node` | HTTP server (axum) + cluster orchestration. Dispatches to either standalone engine or cluster mode. Dual-path: `server.rs` (standalone) + `cluster_server.rs` (cluster). |
| `crates/valori-cli` | `valori` binary — `setup` wizard, `cluster` subcommand, `timeline` subcommand |
| `crates/valori-ffi` | PyO3 FFI layer for the embedded (in-process) Python SDK |
| `crates/valori-verify` | Standalone verifier binary — replays a `events.log` and checks the BLAKE3 chain; surfaces V4 CRC violations |
| `crates/valori-mcp` | `valori-mcp` binary — Model Context Protocol server (stdio) exposing the node as verifiable agent memory; `memory_recall` returns a BLAKE3 receipt |
| `python/valoricore` | Python SDK: `SyncRemoteClient`, `AsyncRemoteClient`, embedded `local.py` via FFI |

---

## Key files — go here first

### valori-kernel

| File | Contains |
|---|---|
| `src/event.rs` | `KernelEvent` enum — every mutation that the kernel understands |
| `src/types/id.rs` | `RecordId`, `NodeId`, `NamespaceId`, `DEFAULT_NS`, `MAX_NAMESPACES = 1024` |
| `src/state/kernel.rs` | `KernelState` — `apply_event_ns()`, `apply()`, `build_index()`, `search_l2_ns()` |
| `src/state/command.rs` | `Command` enum — higher-level commands dispatched into kernel events |
| `src/storage/record.rs` | `Record` struct — `values: Vec<FxpScalar>`, `namespace_id`, `next_in_ns`, `prev_in_ns` |
| `src/storage/pool.rs` | `RecordPool` — slab allocator + intrusive per-namespace linked lists |
| `src/graph/node.rs` | `GraphNode` struct + adjacency list |
| `src/snapshot/encode.rs` | V6 snapshot encoder — writes namespace heads arrays + NSRG section |
| `src/snapshot/decode.rs` | V6 snapshot decoder — backward-compatible with V5 |
| `src/crypto/` | BLAKE3 chaining helpers |
| `src/fxp/` | Q16.16 fixed-point scalar and vector ops (`FxpScalar`) |

### valori-consensus

| File | Contains |
|---|---|
| `src/types.rs` | `ClientRequest`, `ClientResponse`, `ValoriNode` — the Raft application types |
| `src/state_machine.rs` | `ValoriStateMachine` — applies `ClientRequest` to `KernelState`, writes audit entry |
| `src/log_store_redb.rs` | Persistent Raft log on redb (`VALORI_RAFT_LOG_PATH`) |
| `src/network.rs` | tonic/gRPC peer transport |

### valori-node

| File | Contains |
|---|---|
| `src/main.rs` | Entry point — dispatches standalone vs cluster mode |
| `src/config.rs` | All `VALORI_*` env var parsing |
| `src/server.rs` | Standalone HTTP routes (axum): `/records`, `/search`, `/v1/namespaces`, `/v1/graphrag`, etc. |
| `src/graph_rag.rs` | Shared GraphRAG traversal — `expand_subgraph` + `resolve_seed_nodes`; used by `/v1/graphrag` and `/graph/subgraph` on both standalone and cluster paths |
| `src/decay.rs` | Phase C4.1 read-time decay re-rank (`rerank`, `decay_factor`). Pure; never mutates state. Used by `/search` (standalone + cluster, C4.1b) + `/v1/memory/search_vector` |
| `src/valori_reranker.rs` | Phase C5 Valori Reranker — `ValoriReranker` struct, term-frequency corpus, `POOL_FACTOR=20`, hybrid vector+term blend. std-only; never move to kernel. |
| `src/tree_rag.rs` | Phase I5 Tree-RAG — `TreeIndex` (markdown→tree), deterministic ToC navigator, breadcrumb citations, BLAKE3 `Receipt` chain + `verify_receipt` + `verify_chain`. Types for all four endpoints: `BuildRequest/Response`, `QueryRequest`, `HybridRequest/Response`, `VerifyRequest/Response`. `tree_verify` (stateless, no cache dep) lives here; `tree_build`/`tree_query`/`tree_hybrid` are stateful handlers in `server.rs`/`cluster_server.rs`. std-only; never move to kernel. |
| `src/community.rs` | Phase I6 Community Layer — `CommunityStore`, `label_propagation()` (O(n+e), min-label tie-break), `build_community_store()` (BLAKE3 receipt over sorted assignments), `rank_communities()` (cosine over centroids), `extract_entities_via_llm()`. Handlers `/v1/community/{detect,search}` + `/v1/ingest/extract-entities` in both routers. std-only; never move to kernel. |
| `src/engine.rs` | Standalone engine wrapper around `KernelState`; holds `created_at` (derived, non-hashed) for decay |
| `src/cluster.rs` | Cluster startup, `ClusterHandle`, watcher tasks |
| `src/cluster_server.rs` | Cluster HTTP routes — same surface as `server.rs` but backed by Raft |
| `src/cluster_api.rs` | `/v1/cluster/*` management plane (status, health, add-node, remove-node, role) |
| `src/replication.rs` | Leader hash-convergence watcher (`state_hash_match` gauge) |
| `src/wal_writer.rs` | `events.log` append — standalone path |
| `src/wal_reader.rs` | `events.log` reader — multi-segment recovery |
| `src/object_store/mod.rs` | `ObjectStoreBackend` — S3/file snapshot offload and WAL archival (Phase 3.1) |

---

## Architecture in one paragraph

Mutations enter as HTTP JSON → serialized to `ClientRequest { event: KernelEvent, namespace_id, request_id }` → either applied directly to `KernelState` (standalone) or committed through openraft and applied in `ValoriStateMachine::apply()` (cluster). Every successful apply appends a BLAKE3-chained entry to `events.log` and updates the running state hash. Namespaces (collections) are 16-bit integer IDs (`NamespaceId`); per-namespace record lists are intrusive linked lists stored inside each `Record`. The state hash is the BLAKE3 Merkle root over all applied events; it is reproduced from scratch after snapshot restore. Arithmetic is Q16.16 fixed-point (`FxpScalar`) everywhere — no `f32`/`f64` in the hot path.

---

## Invariants — never break these

1. **Apply before audit**: `DEDUP CHECK → KERNEL APPLY → AUDIT WRITE`. Never write an audit entry for a rejected or duplicate event.
2. **Namespace isolation at 3 points**: `apply_committed_event_ns()` (event path), WAL replay path, and `build_index()` after snapshot restore. If you add a fourth path, add a guard there too.
3. **Q16.16 only in vector ops**: All `insert_record` and `search` paths must use `FxpScalar`. Never pass raw `f32` through the kernel.
4. **Snapshot buffer ≥ 16 KB in tests**: V6 snapshots are ~8.3 KB minimum. Use `vec![0u8; 1 << 14]`.
5. **`watcher_tasks` must be aborted before redb re-open**: `spawn_state_hash_watcher` returns a `JoinHandle` stored in `ClusterHandle`. Abort and await it before any shutdown or test restart, or redb will deadlock on the file lock.
6. **`request_id` dedup**: `ClientRequest` carries a `request_id: Uuid`. The state machine checks and records it before applying. Do not skip this on any new cluster command.
7. **`valori-kernel` MUST remain `no_std`** — this is a non-negotiable architectural invariant. The kernel is the portability moat: it must compile for embedded and WASM targets with no OS dependency.
   - `crates/valori-kernel/src/lib.rs` has `#![cfg_attr(not(feature = "std"), no_std)]` — never remove this.
   - Never add `use std::` to any file inside `crates/valori-kernel/src/`. Use `core::` or `alloc::` instead.
   - Anything that genuinely requires std (file I/O, threads, `HashMap`, crypto-shredding) must be gated behind `#[cfg(feature = "std")]`.
   - New dependencies in `crates/valori-kernel/Cargo.toml` must use `default-features = false`. If they need std, add them to the `std` feature list, not as always-on deps.
   - After any change to `valori-kernel`, verify with: `cargo build -p valori-kernel --target wasm32-unknown-unknown`
   - `valori-node`, `valori-consensus`, and `valori-cli` opt in via `features = ["std"]` — that is intentional and correct.

---

## MANDATORY: single-node AND multi-node — always think both

Every feature must be evaluated against **both** execution paths before you write a single line of code. Missing one path is a bug, not a follow-up.

### The two paths

| Path | Entry point | State access | How writes land |
|---|---|---|---|
| **Standalone** | `server.rs` → handler with `State<SharedEngine>` | Direct `engine.write().await` | Mutates `KernelState` in-process; WAL writer appends to `events.log` |
| **Cluster** | `cluster_server.rs` → handler with `State<DataPlaneState>` | No direct engine — writes go via `raft.client_write(KernelEvent)` | Raft log → `ValoriStateMachine::apply()` on all nodes → `KernelState` mutated identically on every peer |

### Decision rules — apply before implementing any endpoint

1. **Stateless handlers** (pure text/JSON transformation, no reads or writes to `KernelState`) — add to **both** `server.rs` and `cluster_server.rs`. Example: `/v1/ingest/document` (chunking only).

2. **Read-only handlers** (search, health, proof, metrics) — add to **both**. Cluster reads from local `KernelState` snapshot; standalone reads directly from engine.

3. **Write handlers** (insert, graph mutations, collection ops) — add to **both**, but the cluster path must go through `raft.client_write()`, not a direct engine lock. Never take a write lock on the engine in a cluster handler.

4. **Compound handlers** (chunk + embed + insert, e.g. `/v1/ingest`) — the embed step is stateless (HTTP call), but the insert step is a write. Standalone: embed then `engine.write()`. Cluster: embed then `raft.client_write()` per chunk. Both paths required.

5. **Config-dependent handlers** (e.g. embed config, decay config) — the config lives on `NodeConfig` → `Engine` in standalone, and must be separately accessible in cluster mode (store on `DataPlaneState` or read from env at handler time).

### Checklist — for every new endpoint

- [ ] Added to `server.rs` (standalone)?
- [ ] Added to `cluster_server.rs` (cluster)?
- [ ] If it writes state — does the cluster path use `raft.client_write()` (not a direct engine lock)?
- [ ] If it reads config — is the config accessible from both `SharedEngine` and `DataPlaneState`?
- [ ] If it's stateless — confirmed no `State<>` parameter so it compiles in both routers?
- [ ] Python SDK updated (`SyncRemoteClient` + `AsyncRemoteClient`)?
- [ ] Node README API table updated?
- [ ] AGENTS.md env var table updated if new env vars added?

### Common mistake to avoid

Adding a new endpoint only to `server.rs` and calling it "done". The cluster path silently 404s or returns "method not allowed" — no compile error, no test failure, only a runtime surprise when someone runs `docker compose up`.

### Shared-handler pattern (Phases R1/R2) — use it for new endpoints

`crates/valori-node/src/routes/` holds handler bodies written ONCE and served
by both routers: a per-domain `*Ops` trait carries only the state-touching
primitives (standalone impl = engine locks in `server.rs`; cluster impl =
`raft_write_data()` / state-machine reads in `cluster_server.rs`), and the
shared generic function owns validation + response shaping. Migrated domains:
`collections`, `graph`, `records` (delete/soft-delete), `meta`, `version`.
Not yet migrated (each needs a design pass): insert, search, memory
upsert/consolidate/contradict. Intentionally path-specific (do NOT unify):
index config/rebuild, proof/timeline mechanics.

`tests/route_parity.rs` enforces parity mechanically: it diffs the `/v1`
route declarations of both server files (paths AND methods). Adding a route
to one router only fails the test — either add it to both, or add it to the
`STANDALONE_ONLY` / `CLUSTER_ONLY` allowlist with a reason.

---

## Where to add things

| Task | File(s) to touch |
|---|---|
| New kernel mutation | Add variant to `KernelEvent` in `event.rs`, handle in `KernelState::apply_event_ns()` in `state/kernel.rs` |
| New HTTP endpoint (standalone) | `server.rs` → route + handler; add to `build_router()` |
| New HTTP endpoint (cluster) | `server.rs` **and** `cluster_server.rs`; both must be kept in sync |
| New collection/namespace operation | `server.rs` and `cluster_server.rs`; namespace logic lives in `RecordPool` / `pool.rs` |
| New consensus command | `types.rs` (`ClientRequest` variant), `state_machine.rs` (apply branch) |
| New env var | `config.rs` (`NodeConfig::from_env()`) |
| New Python SDK method | `python/valoricore/remote.py` (both `SyncRemoteClient` and `AsyncRemoteClient`) |
| New snapshot field | `snapshot/encode.rs` and `snapshot/decode.rs`; bump snapshot version constant; update tests in `tests/format.rs` |

---

## Commands

```bash
# Build
cargo build --workspace

# Test (the two meaningful crates)
cargo test -p valori-kernel
cargo test -p valori-node

# Test a specific test
cargo test -p valori-node test_collections_isolation -- --nocapture

# Run a 3-node local cluster
docker compose up -d
curl http://localhost:3001/health

# Python SDK (from repo root)
pip install -e python/
python3 examples/cluster_quickstart.py

# Benchmarks (node must be running on :3000)
python3 benchmarks/run_benchmark.py
python3 benchmarks/multi_arch_hash.py
python3 benchmarks/q16_precision.py --dim 384
```

---

## Snapshot format versions

| Version | What changed |
|---|---|
| V5 | BruteForce + HNSW index payload |
| V6 (current) | Adds per-record `namespace_id` + `next_in_ns` + `prev_in_ns`; 2 × 1024 × 4 B namespace heads; NSRG JSON section at end |

Backward-compat: V5 snapshots restore into an empty namespace registry (all records land in `DEFAULT_NS`).

---

## Environment variables

**Standalone node**

| Var | Default | Purpose |
|---|---|---|
| `VALORI_DIM` | 128 | Vector dimension (immutable after first insert) |
| `VALORI_MAX_RECORDS` | 1 000 000 | Record slab capacity |
| `VALORI_MAX_NODES` / `VALORI_MAX_EDGES` | 100k / 500k | Graph slab capacity |
| `VALORI_BIND` | 0.0.0.0:3000 | HTTP listen address |
| `VALORI_EVENT_LOG_PATH` | — | Audit log path (omit = in-memory only) |
| `VALORI_SNAPSHOT_PATH` | — | Snapshot file path |
| `VALORI_SNAPSHOT_INTERVAL` | — | Periodic autosave interval in seconds (standalone only; needs `VALORI_SNAPSHOT_PATH`). UI-launched nodes set 60. Omit = snapshot only on graceful shutdown |
| `VALORI_AUTH_TOKEN` | — | Bearer token (omit = no auth) |
| `VALORI_INDEX` | brute | `brute`, `hnsw`, `ivf`, `bq`, or `auto` (`auto` = brute-force < 10k, BQ 10k–2M, HNSW > 2M; `mstg` is an alias) |
| `VALORI_SHARD_COUNT` | 1 | Standalone logical shards. Namespaces route via `ns_id % shard_count`. 1 = no sharding. |
| `VALORI_IVF_N_LIST` | auto | IVF centroid count. Absent = auto-scale: `max(16, sqrt(N))` computed at each `build()`. Setting this disables auto-scale. |
| `VALORI_IVF_N_PROBE` | auto | IVF probe count. Absent = auto-scale: `max(1, sqrt(n_list))`. Setting this disables auto-scale. |
| `VALORI_DECAY_HALF_LIFE_SECS` | — | Phase C4.1 default decay half-life for search ranking; per-request `decay_half_life_secs` overrides. Omit/0 = no decay |
| `VALORI_EMBED_PROVIDER` | — | Phase I2: `ollama` / `openai` / `custom`; absent = embedding disabled; enables `POST /v1/ingest` |
| `VALORI_EMBED_MODEL` | provider default | Embed model name (e.g. `nomic-embed-text`, `text-embedding-3-small`) |
| `VALORI_EMBED_URL` | provider default | Base URL (Ollama: `http://localhost:11434`; OpenAI: `https://api.openai.com`) |
| `VALORI_EMBED_API_KEY` | — | API key for OpenAI / custom providers |

**Cluster additions**

| Var | Purpose |
|---|---|
| `VALORI_NODE_ID` | Integer ID for this node (1-based) |
| `VALORI_CLUSTER_MEMBERS` | `1=host:3100/host:3000,2=...` — full topology |
| `VALORI_CLUSTER_INIT` | `1` on the bootstrap node only |
| `VALORI_RAFT_BIND` | gRPC listen address (default 0.0.0.0:3100) |
| `VALORI_RAFT_LOG_PATH` | Persistent redb file for Raft log + vote + SM |
| `VALORI_STATE_HASH_CHECK_SECS` | Hash-convergence poll interval (default 30, 0 = off) |
| `VALORI_TLS_CA` / `VALORI_TLS_CERT` / `VALORI_TLS_KEY` | mTLS on Raft channel |
| `VALORI_SHARD_COUNT` | Independent Raft groups per process, one shared gRPC listener, symmetric placement (every node runs every shard). Default 1 = single-Raft-group behavior, byte-identical to pre-S1. Namespace→shard HTTP routing is live (S3-S9: `shard_for_namespace(ns, count) = ns % count`, wired into every collection-aware handler) and every shard has its own BLAKE3-chained audit log (S13: `events-shardN.log` via `shard_path()`). Exposed in the UI project wizard as "Shards" (S14, cluster projects only). Known gap: `/v1/proof/event-log` + `/v1/timeline` still read shard 0's log only. See `docs/phases/phase-S1-multi-raft-skeleton.md` through `phase-S14-*.md` |

**Object store (Phase 3.1)**

| Var | Default | Purpose |
|---|---|---|
| `VALORI_OBJECT_STORE_URL` | — | `s3://bucket/prefix` or `file:///path`; absent = disabled |
| `VALORI_OBJECT_STORE_KEEP` | 7 | Snapshots to retain in object store after pruning |
| `VALORI_OBJECT_STORE_REGION` | `us-east-1` | S3 region (also reads `AWS_DEFAULT_REGION`) |
| `VALORI_OBJECT_STORE_ENDPOINT` | — | Custom endpoint for MinIO / Localstack / R2 |

---

## Python SDK quick reference

```python
from valoricore.remote import SyncRemoteClient

c = SyncRemoteClient("http://localhost:3000")

# Collections
c.create_collection("tenant-acme")
c.list_collections()           # → ["default", "tenant-acme"]
c.drop_collection("tenant-acme")

# Node health
c.health()  # → "ok"

# Data (collection= defaults to "default")
c.insert([0.1, 0.2, 0.3], collection="tenant-acme")
c.insert([0.1, 0.2, 0.3], text="Section 3.1 Training — AdamW optimizer")  # Phase C5: index for Valori Reranker
c.batch_insert([[...], [...]], collection="tenant-acme")
c.insert_batch([[...], [...]], texts=["chunk one body text", "chunk two body text"])  # Phase C5: bulk index

# Search
c.search([0.1, 0.2, 0.3], k=5, collection="tenant-acme",
         consistency="linearizable")  # or "local"

# Valori Reranker (Phase C5) — hybrid vector + term-frequency scoring, server-side.
# rerank=True is the default; pass query_text to activate term scoring.
c.search([0.1, 0.2, 0.3], k=5, query_text="what optimizer is used?")
# → re-ranks vector candidates by term frequency; best for lexical queries

# Recency-aware search (Phase C4.1) — older records decay in ranking.
# Hits gain decay_factor + age_secs; score stays the true distance.
c.search([0.1, 0.2, 0.3], k=5, decay_half_life_secs=86400)  # 1-day half-life

# Metadata filtering (Phase I7) — restrict results by JSON predicate.
# Exact match: all keys must be present and equal in the record's stored metadata.
c.search([0.1, 0.2, 0.3], k=5, metadata_filter={"author": "Alice"})
# Range operators (gt, gte, lt, lte, eq) for numeric fields:
c.search([0.1, 0.2, 0.3], k=5, metadata_filter={"author": "Alice", "year": {"gte": 2020}})

# GraphRAG — K nearest vectors + connected subgraph in one call
c.graphrag([0.1, 0.2, 0.3], k=5, depth=2)
# → {"hits": [...], "seed_nodes": [...], "subgraph": {"nodes": [...], "edges": [...]}}

# Agent-memory primitives — return memory_id + graph nodes + decay fields
c.memory_upsert([0.1, 0.2, 0.3], metadata={"role": "note"})  # → {"memory_id", "record_id", "document_node_id", "chunk_node_id"}
c.memory_search([0.1, 0.2, 0.3], k=5, decay_half_life_secs=86400)
# → [{"memory_id", "record_id", "score", "metadata", "decay_factor"?, "age_secs"?}]

# Self-maintaining memory (Phase C4.2 / C4.3) — commits edges to the audit chain
c.consolidate(old_record_id=7, new_vector=[0.2, 0.3, 0.4])  # soft-delete old + insert new + Supersedes edge
# → {"old_record_id", "new_record_id", "supersedes_edge_id", "state_hash"}
c.contradict(record_a=3, record_b=9, threshold=0.9)         # Contradicts edge if cosine ≥ threshold
# → {"record_a", "record_b", "similarity", "contradicts", "edge_id"?, "state_hash"}

# Proof / provenance
c.event_log_proof()   # → {"event_log_hash", "final_state_hash", "committed_height", ...} — the receipt primitive
c.get_proof()         # → {"final_state_hash": "<hex>"}

# Object-store offload (Phase 3.1) — needs VALORI_OBJECT_STORE_URL on the node
c.upload_snapshot_to_store(); c.list_remote_snapshots(); c.restore_from_store(key="...")

# Cluster
c.get_cluster_status()

# Built-in ingest pipeline (Phase I1/I2) — server handles chunk+embed+insert
c.chunk_document(text, strategy="auto")  # chunking only — no embed
# → {"strategy_used":"tree","chunk_count":31,"chunks":[{"index","title","text"},...]}

c.ingest(text, source="paper.pdf", strategy="auto", collection="default")
# → {"ok":True,"document_node_id":42,"chunk_count":31,"record_ids":[...],"strategy_used":"tree"}
# Requires VALORI_EMBED_PROVIDER on the node. Returns 422 if not configured.

# Document update (Phase I8) — diff by BLAKE3 content hash, re-embed only changed chunks
c.ingest_update(42, new_text, source="paper-v2.pdf", collection="default")
# → {"ok":True,"document_node_id":42,"new_chunk_count":35,"kept_count":28,
#    "removed_count":3,"added_count":7,"record_ids":[...]}
```

> SDK wraps all 40 product endpoints. `list_contradictions()` /
> `resolve_contradiction()` are deprecated (legacy C3 UI-layer; use
> `contradict()` / `consolidate()`).
>
> **Phase C5 additions:** `insert(text=)`, `insert_batch(texts=)`,
> `search(rerank=True, query_text=)`, `health()` — available on both
> `SyncRemoteClient` and `AsyncRemoteClient`.
>
> **Phase I1/I2 additions:** `chunk_document(text, strategy=)`,
> `ingest(text, source=, strategy=, collection=)` — available on both
> `SyncRemoteClient` and `AsyncRemoteClient`.
>
> **Phase I8 addition:** `ingest_update(document_node_id, text, source=, strategy=, collection=)` —
> diff-based document update; re-embeds only changed chunks. Available on both
> `SyncRemoteClient` and `AsyncRemoteClient`.
>
> **Phase I5 additions:** `tree_build(text, doc_name=)`, `tree_query(tree_or_none, query, cache_key=, k=, prev_hash=)`,
> `tree_verify(tree, receipt)`, `tree_hybrid(query, text=, tree=, cache_key=, namespace=, k=, tree_weight=, prev_hash=, doc_name=)` — available on both
> `SyncRemoteClient` and `AsyncRemoteClient`. `tree_build` returns `cache_key`; pass it to subsequent calls instead of the full tree.
>
> **Phase I6 additions:** `community_detect(namespace=, max_iter=)`,
> `community_search(vector, k=, namespace=, depth=, drill_in=)`,
> `community_overview()`,
> `extract_entities(text, namespace=, entity_types=, model=)` — available on both
> `SyncRemoteClient` and `AsyncRemoteClient`. `community_detect` must be called before `community_search` or `community_overview`. `extract_entities` requires `VALORI_EMBED_PROVIDER`.

---

## UI — light mode is mandatory

Every UI change must work in **both** dark and light mode. The app ships with a live theme toggle and real users use light mode. Violating this is a bug, not a cosmetic issue.

### How theming works in this codebase

- `globals.css` defines `.dark` and `.light` CSS classes on `<html>` (set by `ThemeProvider`).
- Semantic tokens (`--background`, `--foreground`, `--border`, `--card`, `--muted-foreground`, etc.) automatically switch — use these, never hardcode colours.
- Valori accent tokens: `--v-accent`, `--v-accent-muted`, `--v-accent-ring` — also defined for both themes.
- `--v-heatmap-empty` is explicitly set for dark (`oklch(0.28 0.02 270)`) and light (`oklch(0.88 0.01 270)`) in `globals.css`.
- Tailwind zinc hardcodes (`bg-zinc-950`, `text-zinc-400`, `border-zinc-800`, etc.) are remapped in `.light` via a palette flip — **but this only works for zinc**. Never use raw oklch/hex/rgb colour literals that are only readable on dark backgrounds.

### Checklist — apply to every UI change

1. **No hardcoded dark colours** — `oklch(0.28 ...)`, `rgba(0,0,0,...)`, `#1a1a1a` etc. are invisible or ugly in light mode. Use semantic tokens.
2. **CSS keyframe animations** — check `box-shadow` colours in `@keyframes`. The `chainGlow` animation uses `oklch(0.62 0.20 270 / ...)` (indigo) which is fine in both themes. Avoid hardcoded dark RGBA shadows.
3. **Inline `style=` colour values** — always use CSS variables (`var(--v-accent)`), never literal colour strings.
4. **New CSS variables** — when adding a new variable, add it in **both** `.dark` and `.light` blocks in `globals.css`. Omitting the light value leaves the variable undefined in light mode (it won't inherit the dark value).
5. **`text-zinc-*` and `bg-zinc-*`** — the palette-flip in `.light` handles these automatically. Prefer these over semantic tokens when you need to express "slightly lighter/darker than the surface".
6. **After writing any UI code, mentally trace through light mode**: would any colour be invisible (dark-on-dark or light-on-light)? Would any border disappear? Would any shadow be invisible?
7. **Heatmap / chart colours** — use `oklch`/CSS-var colours, never `rgba(99,102,241,...)` with a fixed opacity that only works on dark backgrounds.

### Quick reference — token meanings

| Token | Dark | Light |
|---|---|---|
| `--background` | near-black | near-white |
| `--card` | slightly lighter than bg | slightly darker than bg |
| `--border` | `oklch(1 0 0 / 10%)` (barely visible white) | `oklch(0.87 0 0)` (light grey) |
| `--muted-foreground` | medium grey | medium-dark grey |
| `--v-accent` | indigo-500 bright | indigo-500 slightly darker |

---

## Docs index (short)

| Path | What it covers |
|---|---|
| `docs/README.md` | Full documentation map |
| `docs/THREAT_MODEL.md` | Security model, keyed BLAKE3 MAC analysis |
| `docs/CAPACITY.md` | Vectors/GB, RAM, WAL growth, S3 cost |
| `docs/DR.md` | Snapshot-to-S3, restore, cross-region runbook |
| `docs/CLUSTER.md` | Cluster operations and setup wizard |
| `CHANGELOG.md` | Version history |
| `CONTRIBUTING.md` | Contribution guidelines |
