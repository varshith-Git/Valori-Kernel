# Changelog

All notable changes to Valori are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Replication factor in the project-creation wizard (Phase 6.1)** — the
  UI's "New Project" dialog now offers "Single Node" or "3-Node Cluster"
  (Raft-replicated, tolerates 1 node down) as a first-class creation
  choice, instead of clustering living only on the separate `/launch`
  power-user page. Cluster projects get a `nodes[]` manifest entry (legacy
  single-port manifests migrate automatically), a dedicated 4010-4999 port
  range that never collides with single-node projects (3010-3999) or the
  Launcher (3000-3009), per-node data files under the same project dir,
  aggregate "2/3 running" status in the UI, and full open/close/delete
  lifecycle across all nodes (open waits for full quorum health; close
  snapshot-stops every node and re-locks files at rest). The two
  previously-divergent dimension option lists are unified into one shared
  module, and `/launch` now imports the same cluster-config helpers
  instead of maintaining its own copies. Verified live end to end,
  including leader election, follower reads, and close→reopen data
  persistence. See `docs/phases/phase-6.1-project-wizard-replication.md`.
- **Shard count in the project-creation wizard (Phase S14)** — the UI's
  first surface for horizontal scaling. Creating a 3-node-cluster project
  now offers a "Shards" control (1/2/4/8); the choice is persisted in the
  project manifest and threaded to `VALORI_SHARD_COUNT` on every spawned
  node (one process per replica still — all shards on a node share its
  HTTP port and gRPC listener). Cluster projects only; standalone
  projects have no shard concept and pin to 1. Verified live end to end:
  a 3-replica/2-shard project produced six independently chain-valid
  per-node-per-shard audit logs (`valori-verify` on each). Requires
  Phase S13 (below) — shard count was not safe to expose while shards
  ≥ 1 silently discarded their audit trail. Known gap, disclosed in the
  wizard itself: Proof/Timeline pages still read shard 0's log only.
- **Shard routing completed across the entire cluster HTTP surface (Phases
  S5-S9)** — every collection-aware endpoint now routes to the shard that
  actually owns its namespace's data, closing out the routing work started
  in S3/S4:
  - **S5** — `cluster_insert_encrypted` routes by namespace;
    `DELETE /v1/crypto/shred/:key_id` fans out to every shard this node
    runs (ciphertext for one key can land on multiple shards) and
    aggregates per-shard status into `{"shredded": bool, "shards": {...}}`.
  - **S6** — linearizable reads are shard-aware:
    `ensure_read_consistency(shard_id, ...)` and
    `GET /v1/cluster/read-index?shard=N`; `cluster_memory_search` gained a
    read-index check it never had before (previously always
    eventually-consistent regardless of the requested `consistency`).
  - **S7** — core CRUD (`/v1/records`, `/v1/search`, `/v1/delete`,
    `/v1/soft-delete`, `/v1/vectors/batch-insert`) gained a `collection`
    field and shard routing, matching the standalone server's existing
    contract.
  - **S8** — graph node/edge CRUD (`/v1/graph/*`), `/v1/graphrag`, and
    namespace-scoped `/v1/community/detect` now route to their collection's
    shard.
  - **S9** — `cluster_ingest` gained automated test coverage via an
    in-process mock embed server; `cluster_tree_hybrid`'s vector-search
    section now routes to the resolved namespace's shard (previously
    resolved the namespace correctly but scanned shard 0 regardless — a bug
    flagged back in S1 and never revisited until now).

  See `docs/phases/phase-S5-crypto-shredding-cross-shard.md` through
  `docs/phases/phase-S9-ingest-coverage-tree-hybrid.md`.

- **Namespace→shard routing (Phases S3+S4)** — deterministic
  `shard_for_namespace(namespace_id, shard_count)` (`namespace_id % shard_count`,
  no placement table needed) and a multi-shard-aware `DataPlaneState`.
  `cluster_memory_upsert`, `cluster_memory_consolidate`,
  `cluster_extract_entities`, and `cluster_ingest` (writes) plus
  `cluster_list_nodes` and `cluster_memory_search` (reads) now route to the
  shard that actually owns a namespace's data, instead of always shard 0 —
  every collection-aware write handler is now shard-routed. `cluster_extract_entities`
  also had a latent id-allocation race fixed as part of making its routing
  safe (was pre-reading "next id" from the wrong shard's counter). See
  `docs/phases/phase-S3-shard-routing-infrastructure.md` and
  `docs/phases/phase-S4-remaining-write-handlers.md`.

### Fixed
- **Shards ≥ 1 silently discarded their audit trail (Phase S13)** —
  `bootstrap_cluster()` only ever gave shard 0 a real audit sink; every
  other shard got a hardcoded `NullAuditSink` that discards events without
  writing them to disk. This was an intentional S1-era decision made when
  no HTTP traffic could reach shard ≥ 1 — invalidated once S3-S9 wired real
  namespace→shard HTTP routing to every shard, but never revisited. Writes
  to a non-zero shard were still correctly Raft-committed and applied to
  that shard's `KernelState`, but had no BLAKE3 chain on disk. Every shard
  now gets its own genuine `events-shardN.log` (unchanged filename at
  `shard_count == 1`). A failure to open shard 0's audit log remains fatal
  (unchanged); a failure on shards ≥ 1 falls back to `NullAuditSink` for
  that shard only, logged loudly, rather than aborting the whole node —
  new capability this phase adds, no prior "fatal" guarantee to preserve
  there. See `docs/phases/phase-S13-per-shard-audit-sinks.md`.
- **Cluster mode's `GET /v1/graph/node/:id` and `GET /v1/graph/edges/:id`
  returned different field names than the standalone server (Phase S12)**
  — e.g. `{"id","kind","record"}` vs standalone's
  `{"kind","record_id","namespace_id"}`. Harmless for callers reading raw
  JSON, but the Python SDK's `walk()`/`expand()`/`neighbors()` read
  specific keys (`record_id`, `to_node`) and threw `KeyError` against
  cluster nodes. Predates S1-S11 entirely; found while documenting S11.
  Cluster now emits the same shape as standalone. `GET /v1/graph/subgraph`
  and `/v1/graphrag` were unaffected — they already shared one function
  between both modes.
- **Python SDK graph methods had no `collection` support (Phase S11)** —
  `create_node()`, `get_node()`, `create_edge()`, `get_edges()`,
  `subgraph()`, and `neighbors()` on both `SyncRemoteClient` and
  `AsyncRemoteClient` always targeted the default collection — the server
  side has always supported `collection` on these endpoints (and the
  cluster path routes it correctly as of S8), but the SDK never exposed
  it. All six gained a `collection: str = "default"` parameter,
  backward-compatible with every existing call site.
- **`valoricore-ffi` did not compile (Phase S10)** — `get_timeline()`'s
  exhaustive `KernelEvent` match was missing arms for
  `AutoCreateNamespace`/`DropNamespace` (added in S2). Predates the S1-S9
  sharding work — confirmed present on `main` before any of it. Fixed and
  verified with a real `maturin build --release` (the crate's actual build
  path; a bare `cargo build -p valoricore-ffi` never links successfully by
  design — PyO3's `extension-module` feature omits `libpython`).
- **Python SDK `soft_delete()` permanently deleted records instead of
  soft-deleting them (Phase S7)** — `SyncRemoteClient.soft_delete()` and
  `AsyncRemoteClient.soft_delete()` posted to `/v1/delete` (hard delete)
  instead of `/v1/soft-delete`, on both standalone and cluster targets.
  Fixed both methods to hit the correct endpoint; `crates/valori-node/README.md`'s
  API table had the same mislabeling, corrected, and `/v1/soft-delete`
  (previously undocumented) added as its own row. `delete()`/`soft_delete()`
  also gained an optional `collection` parameter on both clients (and their
  `ClusterClient`/`AsyncClusterClient` wrappers) — previously always scoped
  to the default collection regardless of where the record actually lived.
- **Collections/namespaces for graph data (nodes/edges) and vector-record
  writes were non-functional in cluster mode (Phase S3a)** —
  `ValoriStateMachine::apply()`'s generic dispatch always applied
  `AutoInsertRecord`/`AutoCreateNode`/`AutoCreateEdge` to namespace 0
  regardless of which collection a handler resolved (`cluster_memory_upsert`/
  `cluster_memory_consolidate` resolved a namespace id and then discarded
  it). Only the crypto-shredding path
  (`InsertRecordEncrypted`/`AutoInsertRecordEncrypted`) was genuinely
  namespace-scoped. Fixed by adding `namespace_id` to `ClientRequest`
  (`#[serde(default)]`, backward compatible) and threading it through
  `apply()`'s generic dispatch. Verified live: writes to two different
  collections now correctly land in their own namespaces (and, combined
  with the routing above, their own shards).
- **Cluster-mode collection creation was not Raft-replicated (Phase S2)** —
  `POST /v1/namespaces` mutated a private, per-node, in-memory registry
  directly. Two nodes could silently assign different `NamespaceId`s to the
  same collection name (or the same id to different names), and a follower
  would happily "succeed" against its own out-of-sync copy instead of
  redirecting to the leader. Now goes through Raft like every other write
  (`KernelEvent::AutoCreateNamespace`/`DropNamespace`); every node ends up
  with the identical, durable mapping, and a follower correctly
  307-redirects. See `docs/phases/phase-S2-namespace-replication.md`.
- **Snapshot `CapacityExceeded` at scale** — `encode_state` rewritten from a
  fixed `&mut [u8]` buffer to a growable `&mut Vec<u8>`. Snapshots above ~250K
  records (any dimension) previously failed with `Kernel(CapacityExceeded)`
  because the V6 schema added 10 bytes/record that the buffer-size formula did
  not account for. Verified end-to-end at 1M records (515 MB snapshot in 1.2 s).
  The encoder is now structurally incapable of this error. Stays `no_std`.
- **WAL loss on clean teardown** — added `impl Drop for Engine` and
  `impl Drop for EventCommitter` to flush the batched write buffer on scope
  exit. A clean shutdown could previously lose up to `flush_every` buffered
  events; recovery tests found 0 events after a simulated crash.

### Added
- **Multi-Raft consensus skeleton (Phase S1)** — a cluster process can now run
  multiple independent Raft groups ("shards") sharing one gRPC listener, each
  with its own persistent redb log, state machine, and leader election.
  New `VALORI_SHARD_COUNT` env var (default `1`, byte-identical to prior
  single-Raft-group behavior). Foundation for future namespace-sharded
  horizontal scaling — namespace→shard routing and HTTP-layer wiring are not
  part of this phase. See `docs/phases/phase-S1-multi-raft-skeleton.md`.
- **IVF centroid auto-scaling** (`n_list = max(16, sqrt(N))`, `n_probe = max(1, sqrt(n_list))`) — fixes a 153× QPS regression from 10K to 1M records. Centroids now scale with dataset size so average bucket size stays O(sqrt(N)) and scan cost is O(sqrt(N)) not O(N). Manual override via `VALORI_IVF_N_LIST` / `VALORI_IVF_N_PROBE` disables auto-scaling. Added `IvfIndex::needs_rebuild(count)` hook (returns true when online inserts exceed 2× the build size).
- **`encode_capacity_hint(state)`** — V6-correct pre-allocation estimate so the
  snapshot `Vec` avoids repeated reallocation on the hot path.
- **SIMD L2 distance** (`l2_sq_i32`) — NEON (aarch64) + AVX2 (x86_64) paths with
  scalar fallback; identical integer result on every path (determinism
  preserved), purely a speedup.
- **Benchmark suite** — `benchmarks/local_perf.py` (B1–B7) + `RESULTS_1M.md`,
  with a full performance section and HNSW-above-50K / small-batch warnings in
  the root `README.md`.

## [0.2.3] — 2026-06-29

### Security
- **SEC-2** `SyncRemoteClient` — bearer token was stored in `session.headers`
  (visible in `dict(session.headers)`, Python logging, and tracebacks). Ported
  the `_BearerAuth(requests.auth.AuthBase)` redaction pattern from
  `protocol.py`; token now injected per-request via `__call__`, never stored
  in the headers dict. `_BearerAuth.__repr__` returns `[REDACTED]`.
- **SEC-3** `ProtocolRemoteClient.set_metadata()` / `get_metadata()` — both
  called `session.post/get` without `auth=self._auth`, bypassing authentication
  even when an API key was configured. Fixed; all HTTP calls in
  `ProtocolRemoteClient` are now authenticated.
- **SEC-4** `set_metadata` — `metadata.decode(errors='replace')` silently
  corrupted binary metadata on round-trip (`b'\xff\xfe'` → garbage). Resolved
  by unifying the metadata type to `Dict[str, Any]` with a JSON codec; the
  corrupt decode path is gone entirely.

### Fixed
- **BUG-2** `ProtocolRemoteClient.upsert_text()` crashed with `KeyError` on
  every call — `res["proof_hash"]` hard-access on a field the server does not
  return. Changed to `res.get("proof_hash", "")`.
- **BUG-3** `test_batch_verify.py` called `exit(1)` at module scope when
  `VALORI_URL` was not set, killing the entire pytest process. Replaced with
  `pytest.skip()` inside the test function.
- **BUG-4** `record_count()` always returned 0 — `resp.json().get("record_count", 0)`
  but `/health` returns `{"records": {"live": N}}`. Fixed to
  `resp.json().get("records", {}).get("live", 0)` on both sync and async clients.
- **BUG-5** Duplicate, incompatible exception hierarchies — `protocol.py`
  defined its own `ValoricoreError`, `ValidationError`, `AuthError`,
  `ProtocolError` as separate classes from `exceptions.py`. `except
  valoricore.ValidationError` would not catch a `protocol.ValidationError`.
  Deleted the four duplicates from `protocol.py`; all now imported from
  `exceptions.py`. `ValidationError` now also inherits `ValueError`.
  `AuthError` kept as a backward-compat alias for `AuthenticationError`.
- **#3** `record_count()` — same as BUG-4 above (sync + async).
- **#4** `factory.py` — `Valoricore(remote=…, token=…)` silently dropped the
  token; `SyncRemoteClient` was constructed with no auth. Fixed by forwarding
  `token=token` in both `Valoricore` and `AsyncValoricore`.
- **#5** `ValoriClient` ABC added — shared interface for `LocalClient` and
  `SyncRemoteClient`. `LocalClient` methods widened to accept
  `collection/text/consistency/metadata_filter` kwargs (ignored with annotation)
  so factory-swapped code never raises `TypeError`.
- **#6** Metadata types unified — `insert_batch` now accepts
  `List[Optional[Dict[str, Any]]]` (SDK serialises each dict to a JSON string);
  `get_metadata`/`set_metadata` use `Dict[str, Any]` on all clients with JSON
  encode/decode. `LocalClient` stores as UTF-8 JSON bytes internally.
- **#7** `AsyncRemoteClient` timeout — constructor now accepts
  `timeout: float = 10.0` forwarded to `httpx.AsyncClient`; `AsyncValoricore`
  factory passes it through.
- **#8** BFS O(n²) — all three `walk()` implementations (`LocalClient`,
  `SyncRemoteClient`, `AsyncRemoteClient`) replaced `list.pop(0)` with
  `collections.deque` + `popleft()`.
- **#9** `EXPECTED_DIM = 384` removed from `memory.py`; dead imports cleaned
  from `protocol.py` and `async_memory.py`. `MemoryClient` already used
  `self._dim` for validation; the constant had no effect.
- **#10** Context-manager support — `SyncRemoteClient` gains `close()` /
  `__enter__` / `__exit__`; `AsyncRemoteClient` and both `ClusterClient`
  variants gain `__aenter__` / `__aexit__`.
- **#11** `__init__.py` module docstring — moved to first statement so
  `__doc__` is populated; RST grid table replaced with plain text readable in
  `help()` and `pydoc`.
- **#12** `ClusterClient.close()` — closes all N underlying `requests.Session`
  pools; adds `__enter__` / `__exit__`.
- **#13** `__version__` fallback — `except Exception` narrowed to
  `except PackageNotFoundError`; fallback changed from `"0.0.0"` to `"dev"` to
  distinguish an unregistered editable install from a real release.
- Test suite — 42 offline test failures resolved; `conftest.py` added with
  auto-skip for integration tests, env-var cleanup, and shared fixtures.
  `addopts = "-m 'not integration'"` means `pytest` on a clean checkout runs
  73 tests with 0 failures.

### Added (Phase I7 — Metadata filtering)
- **`metadata_filter` on `POST /search`** — optional JSON predicate that restricts
  results to records whose stored metadata satisfies all specified key-value conditions.
  Supports exact equality for strings/booleans/null and range operators (`gt`, `gte`,
  `lt`, `lte`, `eq`) for numeric fields. Example:
  `{"author": "Alice", "year": {"gte": 2020}}`. Both standalone and cluster paths
  are covered. When a filter is present the server over-fetches `k×10` candidates
  (capped at 5000) before post-filtering to ensure `k` results are returned.
- **Python SDK** — `SyncRemoteClient.search()` and `AsyncRemoteClient.search()` both
  accept `metadata_filter: Optional[Dict[str, Any]] = None`. `ClusterClient` and
  `AsyncClusterClient` inherit via `**kwargs`.

### Added (Phase I6 — Community layer: global sensemaking + entity extraction)
- **`POST /v1/community/detect`** — Label Propagation on the existing GraphNode
  adjacency list (pure Rust, zero LLM). Assigns every node a `community_id`,
  computes an f32 centroid vector per community (average of member FxpVectors),
  and emits a BLAKE3 receipt over the sorted `(node_id, community_id)` map —
  a tamper-evident proof of community structure at that point in time.
  Community store cached in-process; accessible by subsequent search calls.
- **`POST /v1/community/search`** — Cosine-similarity search over community
  centroids. Returns top-k communities ranked best-first with `member_count`
  and a `sample_node_ids` list. Answers "what are the themes across all
  documents?" — the global-sensemaking query that vector RAG cannot handle.
- **`POST /v1/ingest/extract-entities`** — Sends text to the configured LLM
  (reuses `VALORI_EMBED_PROVIDER` credentials — no new env vars). Parses
  `(entity, type, description)` tuples and `(source, target, description,
  strength)` relationships. Embeds entity descriptions and inserts them as
  `Concept` graph nodes with `Relation` edges — bridges a document graph into
  a true entity knowledge graph.
- All three endpoints exist in both **standalone** (`server.rs`) and **cluster**
  (`cluster_server.rs`) paths, following the mandatory dual-path rule.
- `valori-kernel`: added `incoming_edges()` on `KernelState` so Label
  Propagation can traverse both directions of the adjacency list.
- Python SDK: `community_detect()`, `community_search()`, `extract_entities()`
  on both `SyncRemoteClient` and `AsyncRemoteClient`.

### Added (Phase I5 — Tree-RAG: hierarchical retrieval with provable receipts)
- **`POST /v1/tree/build`** — parse a structured/markdown document into a
  navigable table-of-contents tree (sections, parent/child, line ranges).
  Deterministic, zero-LLM, zero-embedding. Returns `{node_count, structure_map, tree}`.
- **`POST /v1/tree/query`** — navigate the tree to the *right section* and answer
  with a breadcrumb + line-range citation and a BLAKE3-chained **retrieval receipt**.
  Distinguishes vocabulary-overlapping sections (e.g. "sick days" → *Sick Leave*,
  not *Annual Leave*) where plain vector search fails. Supports `prev_hash` to
  chain receipts.
- **`POST /v1/tree/verify`** — replay a receipt against the tree; `valid: false`
  proves the stored content was altered after retrieval (tamper detection).
- All three are stateless handlers — identical in standalone and cluster mode.
- Python SDK: `tree_build` / `tree_query` / `tree_verify` on both
  `SyncRemoteClient` and `AsyncRemoteClient`.

### Added (Phase I5 gap-fill — server-side tree cache + hybrid retrieval)
- **Server-side tree cache** — `Engine` (standalone) and `DataPlaneState` (cluster) now
  hold a `HashMap<String, TreeIndex>` keyed by `BLAKE3(text)`. `/v1/tree/build` stores the
  parsed tree and returns `cache_key` in the response. Subsequent `/v1/tree/query` and
  `/v1/tree/hybrid` calls accept `cache_key` instead of re-transmitting the full tree.
- **`POST /v1/tree/hybrid`** — single-call hybrid retrieval fusing tree-RAG section scores
  (term-frequency, normalized to [0,1]) with vector-search similarity scores (if
  `VALORI_EMBED_PROVIDER` is set). Configurable `tree_weight` (default 0.6). Returns merged,
  re-ranked hits with per-hit `source` tag (`"tree"` or `"vector"`), BLAKE3 receipt for the
  tree path, and a human-readable `reasoning` string. Available on both standalone and cluster.
- **`/v1/tree/build` and `/v1/tree/query`** are now stateful (take engine state for cache
  read/write); `/v1/tree/verify` remains stateless (no cache dependency).
- Python SDK: `tree_hybrid()` added to both `SyncRemoteClient` and `AsyncRemoteClient`.

### Added (Phase I4.1 — replicated metadata sidecar)
- **`KernelEvent::SetMeta { key, value }`** — new kernel event storing a
  replicated `meta` map on `KernelState`. Cluster ingest now writes the chunk/
  document metadata sidecar via `raft.client_write(SetMeta)` so **all** peers
  share it (previously node-local on the ingesting node only).
- **`/v1/memory/meta/set` + `/v1/memory/meta/get`** added to the cluster router,
  reading/writing through the kernel (`sm.with_state`) instead of a node-local map.

### Added (Phase I1/I2/I3 — Built-in ingest pipeline)
- **`POST /v1/ingest/document`** — server-side document chunking with five strategies:
  `auto` (sniffs text), `tree` (section headers), `conversation` (Q&A boundaries),
  `sentence` (sentence-window with ±2 context), `fixed` (overlapping windows).
  Returns `{strategy_used, chunk_count, chunks: [{index, title, text}]}`.
  Works in both standalone and cluster mode (stateless handler).
- **`POST /v1/ingest`** — full one-call pipeline: chunk + embed + insert + graph nodes +
  metadata sidecar. Requires `VALORI_EMBED_PROVIDER` (ollama / openai / custom).
  Supports `VALORI_EMBED_MODEL`, `VALORI_EMBED_URL`, `VALORI_EMBED_API_KEY`.
  Returns `{document_node_id, chunk_count, record_ids, strategy_used}`.
- **`/health`** now includes `embed_enabled: bool` and `embed_provider: string?` so
  clients can probe node capability before deciding on a pipeline.
- **`crates/valori-node/src/embedder.rs`** — HTTP embed client with Ollama fallback
  (`/api/embed` → `/api/embeddings`) and OpenAI-compatible batching.
- **Python SDK** — `SyncRemoteClient.chunk_document()`, `ingest()`,
  `AsyncRemoteClient.chunk_document()`, `ingest()`.
- **UI** — DocumentUploadTab probes node on mount; shows "Server-side pipeline active ⚡"
  banner and routes upload through `/v1/ingest` when embed is configured;
  falls back transparently to client-side pipeline otherwise.
- **Phase I4 — cluster ingest**: `POST /v1/ingest` now works in 3/5-node cluster mode.
  Vectors and graph nodes/edges go through `raft.client_write()` and are replicated to
  all peers. `DataPlaneState` gains `embed_config` and node-local `metadata` sidecar.
  `build_cluster_router` auto-reads `VALORI_EMBED_*` env vars. Cluster `/health`
  now exposes `embed_enabled` + `embed_provider`.

### Added (Phase C5 — Valori Reranker)
- **Valori Reranker** (`crates/valori-node/src/valori_reranker.rs`) — server-side hybrid
  retrieval that runs inside the node with no external dependency. Records inserted with a
  `text` field are tokenised and indexed. At search time, `query_text` triggers a two-stage
  pipeline: the kernel returns `k × POOL_FACTOR` candidates by vector similarity, the
  reranker blends vector and term-frequency scores (50 / 50), and the top-k are returned.
  Achieves **90 % accuracy** on hard lexical queries vs 60 % for LLM-based navigation, at
  **0.4 s** latency.
- `/records` and `/v1/vectors/batch_insert` accept `text` / `texts` fields for reranker
  indexing. `/search` accepts `rerank: bool` (default `true`) and `query_text: string`.
- `SyncRemoteClient` and `AsyncRemoteClient` updated: `insert(text=)`,
  `insert_batch(texts=)`, `search(rerank=True, query_text=)`, and new `health()` method.
- Cluster path: `ValoriStateMachine` stores raw texts in `text_corpus`; `cluster_server`
  builds a transient reranker per query from the corpus via `with_text_corpus()`.
- `KernelState::iter_records_in_ns(namespace_id)` — public iterator over records in a
  namespace, used by `drop_collection` to clean up the reranker on collection drop.

### Added (Phase 6 — Persistent, isolated projects in the UI)
- **Each UI project is now its own persistent, isolated workspace.** A project maps to one
  `valori-node` process with its own data dir, port, and WAL/snapshot under
  `~/.valori/projects/<name>/` (manifest at `~/.valori/ui-projects.json`, kept separate from
  the CLI wizard's `projects.json`). Home is now a project picker that lists every project
  from disk — even when all nodes are stopped — and one click resumes a session.
- **Auto-start on open / snapshot-on-close.** Opening a project boots its node and points the
  UI at it; closing writes a final snapshot, stops the node, and re-locks the files at rest.
- **Files are deletable only through the UI.** Data files carry the macOS immutable flag
  (`chflags uchg`; Linux falls back to read-only perms) while a project is at rest — Finder
  and `rm` refuse to remove them. The UI delete path clears the flag first.
- **Node graceful-shutdown snapshot.** Standalone `valori-node` now serves with a
  `SIGTERM`/`Ctrl-C` handler that writes a final snapshot to `VALORI_SNAPSHOT_PATH` before
  exiting — a durable backstop on top of the always-on WAL.
- New UI API routes `GET/POST /api/projects`, `DELETE /api/projects/[name]`, and
  `POST /api/projects/[name]/{open,close}`. The Launcher's defaults moved off `/tmp` to
  `~/.valori/cluster`.

### Changed (Python SDK — full endpoint coverage)
- **The Python SDK now wraps every product endpoint (40/40).** Newly added to `SyncRemoteClient` and `AsyncRemoteClient`:
  - **Agent-memory primitives** — `memory_upsert()` (`/v1/memory/upsert_vector`: store vector + document→chunk graph, returns `memory_id`) and `memory_search()` (`/v1/memory/search_vector`: hits carry `memory_id`, `metadata`, and decay fields). Previously only the lower-level `insert`/`search` (which return `{id, score}` with no `memory_id`/metadata) were exposed.
  - **Proof / provenance** — `event_log_proof()` (`/v1/proof/event-log`: the receipt primitive — event-log hash, state hash, committed height). Also on `ClusterClient`/`AsyncClusterClient`.
  - **Graph / introspection** — `list_nodes()` (`/graph/nodes`), `get_version()` (`/version`).
  - **Snapshot / object-store offload** — `save_snapshot()`, `restore_snapshot()`, `list_remote_snapshots()`, `upload_snapshot_to_store()`, `restore_from_store()`, `list_remote_wal()`, `archive_wal_segment()`.
- **Deprecated** `list_contradictions()` / `resolve_contradiction()` — legacy C3 methods that called the Next.js UI layer (`ui_url`), not the node, and returned whatever that layer held (historically `[]`). They now emit `DeprecationWarning` pointing to the node-native, audited `contradict()` / `consolidate()`. Scheduled for removal.

### Added (Phase C4.3 — Contradiction detection: self-maintaining memory, pillar 3)
- **`POST /v1/memory/contradict`** — given two record ids, computes cosine similarity between their Q16.16 vectors and, if it meets `threshold` (default 0.85), commits an `AutoCreateEdge(record_a → record_b, Contradicts)` to the BLAKE3 audit chain. Request `{ record_a, record_b, threshold?, collection? }`; response `{ record_a, record_b, similarity, contradicts, edge_id?, state_hash }` (`edge_id` only when `contradicts`). On both standalone and cluster data planes.
- **`EdgeKind::Contradicts = 8`** — new kernel edge kind (no_std-safe); the verdict is a first-class hashed event, not mutable metadata.
- **Python SDK** — `contradict(record_a, record_b, threshold=, collection=)` on all four clients; cluster variants route to the leader.
- **v1 boundary (documented):** "contradiction" is currently a structural proxy — cosine similarity ≥ threshold, which detects near-duplicates, *not* semantic NLI. The hashed `Contradicts` event path is signal-agnostic: a real entailment model can replace the cosine gate at the node layer with zero kernel change. See `docs/phases/phase-C4.3-contradiction.md`.

### Added (Phase C4.2 — Memory consolidation: self-maintaining memory, pillar 2)
- **`POST /v1/memory/consolidate`** — replace a memory in one auditable operation: commits `SoftDeleteRecord(old)` → `AutoInsertRecord(new)` → `AutoCreateEdge(new → old, Supersedes)` to the audit chain. Request `{ old_record_id, new_vector, collection?, metadata? }`; response `{ old_record_id, new_record_id, supersedes_edge_id, state_hash }`. On both standalone and cluster data planes.
- **`EdgeKind::Supersedes = 7`** — new kernel edge kind (no_std-safe) linking a replacement to the memory it retired, so a reader can trace why a record was soft-deleted.
- **Python SDK** — `consolidate(old_record_id, new_vector, collection=, metadata=)` on all four clients; cluster variants route to the leader.
- **Atomicity:** standalone is atomic (single engine write lock across all three events). Cluster commits the events as a sequence of Raft entries — each chain-valid and replicated, but not a single transaction; a mid-sequence leader crash can leave a partial result (follow-up: multi-event `ClientRequest`). See `docs/phases/phase-C4.2-consolidation.md`.

### Added (Phase C4.1b — Cluster decay + state-machine creation timestamps)
- **Cluster `/search` now honours `decay_half_life_secs`.** In C4.1 the cluster endpoint accepted the field but ignored it; now the consensus state machine tracks per-record creation timestamps (`StateMachineInner.created_at`, stamped at `AutoInsertRecord` apply time) and the cluster search path runs the same over-fetch → `decay::rerank` → top-k pipeline as standalone. One SDK call now behaves identically against both node types.
- **`ValoriStateMachine::record_created_at` / `with_state_and_timestamps`** — read accessors exposing creation time to the search path under one lock.
- **Determinism preserved** — `created_at` is a derived, non-hashed, non-replicated side map (same design as standalone `Engine.created_at`), so the BLAKE3 state hash is unchanged. Known boundary: a node that restarts or installs a snapshot loses timestamps and ranks pre-event records neutrally until re-stamped — durable WAL timestamps are deferred to **C4.1c**. See `docs/phases/phase-C4.1b-cluster-decay.md`.
- **Internal:** new `raft_write_data` helper returns the committed `ClientResponse` so cluster multi-step writes (consolidate/contradict) read allocated record/node/edge IDs from the apply response instead of pre-reading them — closing a TOCTOU race against concurrent writers.

### Added (Phase C4.1 — Kernel-native time decay: self-maintaining memory, pillar 1)
- **`decay_half_life_secs`** on `POST /search` and `POST /v1/memory/search_vector` — recency-aware re-ranking. When set (> 0), older records decay: a record one half-life old has its L2 distance doubled, so a fresh near-match can overtake a stale better one. Each hit gains `decay_factor` (∈ (0,1]) and `age_secs`; `score` stays the true, undecayed distance. Absent/`0` → byte-identical to the prior response.
- **`VALORI_DECAY_HALF_LIFE_SECS`** — optional server-default half-life; a per-request value wins (incl. an explicit `0` to disable).
- **Determinism preserved** — decay is a read-time re-rank: it never mutates kernel state, emits no event, and does not change the BLAKE3 state hash (regression-tested). Creation time lives in a derived, non-hashed `Engine.created_at` map stamped on live inserts only.
- **Python SDK** — `search(..., decay_half_life_secs=…)` on all four clients (`Sync`/`Async` `RemoteClient`, `ClusterClient` via `**kwargs`).
- **MCP** — `memory_recall` accepts `decay_half_life_secs` for recency-aware agent recall; the receipt still verifies over the decayed result set.
- **Supersedes the UI-only Phase C3** "self-maintaining memory," which shipped no decay and lived outside the audit chain. See `docs/phases/phase-C4.1-decay.md`.
- Known boundaries (v1): cluster decay is accepted-but-neutral (creation time isn't tracked in the consensus state machine yet — C4.1b); `created_at` is in-memory, so recovered records rank neutrally until re-stamped (durable WAL timestamps — C4.1b).

### Added (Phase 3.15 — Native GraphRAG: one-call retrieval)
- **`POST /v1/graphrag`** — retrieve the K nearest vectors **and** the connected knowledge subgraph around them in a single call, from one consistent kernel snapshot. Request `{ query_vector, k, depth, collection? }`; response `{ hits, seed_nodes, subgraph: { nodes, edges } }`. Added to both standalone and cluster data planes (cluster also honours `consistency`).
- **`memory_graph_recall` MCP tool** — GraphRAG with a receipt that binds **both** the hits and the returned subgraph (`receipt.subgraph = { node_ids, edge_ids }`, sorted). valori-mcp now exposes 7 tools.
- **Shared `graph_rag` module** (`expand_subgraph`, `resolve_seed_nodes`) — one BFS implementation reused by `/v1/graphrag`, `/graph/subgraph`, and the cluster equivalents, so the traversal stays identical across paths.
- **Python SDK** — `graphrag(query_vector, k, depth, collection, consistency)` on `SyncRemoteClient`, `AsyncRemoteClient`, `ClusterClient`, and `AsyncClusterClient` (cluster variants route to a read replica).
- Plain `memory_recall` receipts are unchanged on the wire (the new optional `subgraph` field is omitted when absent).

### Added (Phase 3.14 — MCP server: verifiable agent memory)
- **New crate `valori-mcp`** — a Model Context Protocol server (stdio, protocol `2024-11-05`) exposing a Valori node as verifiable, deterministic long-term memory for agents. New binary `valori-mcp`.
- **Six MCP tools** — `memory_write`, `memory_recall`, `memory_why`, `memory_timeline`, `memory_forget`, `memory_fork` — each a thin composition over existing node endpoints.
- **Retrieval receipts** — `memory_recall` returns a `receipt`: `receipt_digest = BLAKE3(canonical_json(body))` binding the exact result set to the committed `state_hash`, `event_log_hash`, and `committed_height` at recall time. Independently recomputable offline by any client, in any language.
- **`VALORI_URL` / `VALORI_AUTH_TOKEN`** (and `--url` / `--auth-token`) configure the node the MCP server talks to.
- **`examples/mcp_agent_memory.py`** — runnable end-to-end demo that boots a node, drives the MCP handshake, and re-derives the receipt digest in Python to prove cross-language verification. **`examples/claude_desktop_config.json`** — copy-paste client config.

### Added (Phase 3.13 — HNSW parameter exposure)
- **`VALORI_HNSW_M`** — sets max edges per node per layer; `m_max0` and `lambda` are derived automatically (`m_max0 = 2*M`, `lambda = 1/ln(M)`).
- **`VALORI_HNSW_EF_CONSTRUCTION`** — sets beam width during index build (default 100). Higher = better recall at the cost of insert throughput.
- **`VALORI_HNSW_EF_SEARCH`** — sets beam width floor during queries (default 50). Higher = better recall at the cost of query latency.
- **`GET /v1/index/config`** — new endpoint returning active index type and current HNSW parameters. Returns `{"index_type":"hnsw","hnsw":{"m":…,"m_max0":…,"ef_construction":…,"ef_search":…}}` for HNSW or `{"index_type":"brute_force","hnsw":null}` for brute-force.
- **Python SDK** — `SyncRemoteClient.get_index_config()` and `AsyncRemoteClient.get_index_config()` wrap the new endpoint.
- `HnswIndex::new_with_config(config: HnswConfig)` constructor; `HnswConfig` gains `ef_search` field.
- `Engine` stores `hnsw_config: HnswConfig` so `rebuild_index()` preserves operator-supplied parameters across crash recovery.

### Added (Phase 3.10 — Signed releases + SBOM)
- **cosign keyless signing** — every release binary and Docker image is signed
  using GitHub Actions OIDC → Sigstore transparency log. No private key to
  manage. Verify with `cosign verify-blob --certificate ... --signature ...`.
- **SPDX 2.3 SBOM** — `valori-sbom.spdx.json` generated via `cargo-sbom` on
  every release tag and attached to the GitHub Release with its own cosign
  signature.
- **Multi-platform binaries** — `linux/amd64`, `linux/arm64`, `darwin/amd64`,
  `darwin/arm64` in every GitHub Release alongside SHA-256 checksums.
- **SOC 2 evidence collection** — `scripts/soc2/collect_evidence.py` hits
  `/v1/proof/*`, `/v1/keys`, `/v1/cluster/status`, `/v1/storage/snapshots`
  and writes an evidence bundle with control-family mappings (CC6.6, CC7.2, A1.1, CC9).
- **Weekly evidence workflow** — `.github/workflows/soc2-evidence.yml` collects
  and uploads a 90-day-retained artifact bundle every Sunday at 02:00 UTC.

### Added (Phase 3.9 — Terraform modules)
- **`terraform/aws/`** — EKS cluster, VPC (3 AZs), S3 Object Lock bucket (KMS
  encrypted), IAM IRSA role for pod-level S3 access, ALB controller role,
  CloudWatch alarms for `state_hash_match` and replication lag.
- **`terraform/azure/`** — AKS cluster, Azure Blob Storage (ZRS, versioning,
  lifecycle policy), Key Vault (purge-protected, Premium SKU for Phase 5 CMK),
  Log Analytics workspace (90-day retention), Monitor alerts.
- **`docs/DEPLOY_AWS.md`** — Quick-start, variables, Helm deploy, cost estimate (~$575/mo).
- **`docs/DEPLOY_AZURE.md`** — Quick-start, SOC 2 KQL queries, CMK upgrade path, cost estimate (~$636/mo).

### Added (Phase 3.8 — Write-throughput regression gates)
- **`benchmarks/write_regression.py`** — Measures p50/p99 single-insert latency
  and batch throughput; compares against `benchmarks/baseline/write_regression_baseline.json`.
  Exit 1 if p99 grows > 15% or throughput drops > 10%.
- **`.github/workflows/write-regression.yml`** — Runs on every PR touching `crates/`.
  Builds release binary, starts node, runs benchmark, posts a warning comment on
  regression. Does not block merge (`continue-on-error: true`).
- **`benchmarks/baseline/write_regression_baseline.json`** — Seed baseline
  (p99 = 8 ms, throughput = 3 000 rps). Update via `--save-baseline` after
  deliberate perf improvements.

### Added (Phase 3.12 — Batch insert per-item idempotency)
- **Per-item `request_ids`** in `POST /v1/vectors/batch_insert` — each slot in
  the batch may carry an optional 32-hex idempotency key. A duplicate key is
  detected server-side (O(1) in-memory `FxHashMap`) and the previously assigned
  record ID is returned instead of creating a new record.
- **Mixed batches supported** — deduped and new items may be interleaved at
  arbitrary positions; the response `ids` array preserves original order.
- **Capacity guard accounts for deduped items** — a fully-deduped batch never
  trips the capacity limit.
- **Python SDK** — `insert_batch()` on all four client classes gains
  `request_ids: Optional[List[Optional[str]]] = None`.
- **4 new integration tests** in `tests/api_batch_idempotency.rs`.

### Changed (Phase 3.11 — Concurrent reads via RwLock engine)
- `SharedEngine` type changed from `Arc<Mutex<Engine>>` to `Arc<RwLock<Engine>>`;
  18+ read-only HTTP handlers now acquire a shared read lock, allowing concurrent
  search, proof, health, and timeline requests without serializing behind a global
  write lock. Write handlers (insert, delete, restore, crypto-shred, etc.) retain
  the exclusive write lock.
- `main.rs` auto-snapshot task uses `.read().await` (snapshot is read-only).
- Replication hash-checker and start-offset reads use `.read().await`.

### Added (Phase 3.6 — Crypto-shredding / GDPR erasure)
- **AES-256-GCM per-record encryption** — `POST /v1/records/encrypted` encrypts
  a binary payload before storing; the vector slot is zeroed (not searchable).
  Returns `{"id": int, "key_id": str}`. Group multiple records under one
  `key_id` to shred them atomically.
- **Cryptographic key destruction** — `DELETE /v1/crypto/shred/:key_id` destroys
  the DEK; all records encrypted under that key become permanently unrecoverable
  (GDPR Article 17 "right to erasure" via key destruction, not log truncation).
- **Key existence check** — `GET /v1/crypto/status/:key_id` returns
  `{"exists": bool}`.
- **`VALORI_SHRED_LOG_PATH`** — optional env var; shredded key_ids are appended
  to this file so they remain unrecoverable across restarts.
- **Python SDK** — `insert_encrypted()`, `shred_key()`, `shred_key_status()`
  added to both `SyncRemoteClient` and `AsyncRemoteClient`.
- **Kernel invariants** — `FLAG_ENCRYPTED` (0x02) and `FLAG_SHREDDED` (0x04)
  now fully implemented; `is_searchable()` added to `Record`; shredded records
  are excluded from search, iteration, and index rebuild.
- **Audit chain preserved** — encrypted/shredded record slots remain in the
  BLAKE3 hash chain; the flags byte proves shredding happened without exposing
  plaintext.
- **5 new integration tests** in `tests/api_crypto_shred.rs`.

### Added (Phase 3.7 — `valori import` — provable migrations)
- **`valori import qdrant`** — imports from a Qdrant collection via the scroll
  API. Detects source dimension automatically and aborts with a clear error if
  it mismatches the Valori node's `VALORI_DIM`. Cursor-based pagination;
  per-record idempotency keys ensure exactly-once delivery even on retry.
  Supports `--resume` via a `.valori-import-qdrant-<collection>.json` sidecar
  (tracks `last_offset` + import count across interruptions). Progress bar via
  `indicatif`; state hash printed on completion.
- **`valori import jsonl`** — imports from a JSONL file
  (`{"vector": [...], "metadata": "...", "tag": 0}` per line). Accepts aliases
  `embedding`/`values` for the vector field and `text`/`content`/`payload` for
  metadata. Skips malformed or wrong-dimension lines with a warning; does not
  abort the whole import.
- **Dim validation before any data write** — both subcommands call
  `GET /health` and compare the node's declared `dim` to the source before
  touching any data.
- **Auto-create target collection** — if the target collection doesn't exist,
  it is created before the first insert (idempotent; `400 Already Exists` is
  swallowed).
- **No new dependencies** — uses `ureq` + `indicatif` + `chrono` already in
  `valori-cli`'s dep tree.

### Added (Phase 3.5 — Per-tenant API Keys + RBAC)
- **`POST /v1/keys`** — create a scoped API key (`read_only`, `read_write`, or
  `admin`). Returns the plain-text token once; thereafter only the BLAKE3 hash
  is stored. Accepts optional `collection` lock and `description`.
- **`GET /v1/keys`** — list all keys (masked — `prefix` + metadata, no raw token).
  Requires `admin` scope.
- **`DELETE /v1/keys/:id`** — revoke a key. Audit-safe: key is removed from the
  store immediately; the `events.log` is not affected.
- **`VALORI_KEYS_PATH`** — new env var (JSON file); key store survives restarts
  when set. Absent = in-memory only.
- **`VALORI_AUTH_TOKEN` legacy fallback** — existing static tokens continue to
  work; the new key store is checked first, then the static token as a fallback
  (treated as admin scope).
- **`build_router_with_keys()`** / **`build_cluster_router_with_keys()`** — new
  router builders used by `main.rs`; existing `build_router()` unchanged
  (in-memory key store, no breaking change for tests).
- **Scope enforcement at middleware layer** — routes auto-classified as
  read-only, read-write, or admin by path + method without per-handler changes.
- **8 new integration tests** in `crates/valori-node/tests/api_keys.rs`.

### Added (Phase 3.3 — Cluster-aware Python SDK)
- **`ClusterClient`** — new sync multi-node client. Takes a list of node URLs;
  routes writes to the discovered leader, round-robins local reads across all
  replicas, and upgrades to linearizable reads on request. Leader is discovered
  from the first 307 redirect and cached; failover resets the cache and
  self-heals on the next call.
- **`AsyncClusterClient`** — async mirror backed by `AsyncRemoteClient`.
  `cluster_health()` fans out with `asyncio.gather`. `close()` shuts down all
  underlying httpx clients.
- **`SyncRemoteClient.insert()`** — now auto-generates a UUID4 idempotency key
  and sends it as `"request_id": [u8; 16]` in the JSON body on every call.
  The key is identical across all retry attempts, enabling server-side dedup
  when a write was applied before a connection reset. Pass `idempotency_key=`
  to supply your own token.
- **`SyncRemoteClient.delete()` / `soft_delete()`** — same idempotency key
  handling.
- **`SyncRemoteClient.leader_url()`** — expose the cached leader base URL.
- **`SyncRemoteClient.get_cluster_role()`** / **`AsyncRemoteClient.get_cluster_role()`**
  — `GET /v1/cluster/role` → `"leader"` | `"follower"`.
- **`AsyncRemoteClient.timeline()`** — replaced `aiohttp` with the existing
  `httpx.AsyncClient` (`self.client`); eliminates the mixed-client inconsistency.
- `ClusterClient` and `AsyncClusterClient` exported from `valoricore` package.

### Added (Phase 3.4 — As-of / Point-in-Time Reads)
- **`POST /search`** — new optional fields `as_of` (ISO 8601 UTC string) and
  `as_of_log_index` (u64). When either is set the server replays committed
  events up to the target, searches the resulting state, and returns
  `as_of_log_index`, `as_of_timestamp_iso`, and `as_of_state_hash` (BLAKE3
  hex) alongside the hit list. Requires `VALORI_EVENT_LOG_PATH`.
- **`GET /v1/timeline`** — upgraded from a raw string list to structured JSON
  (`TimelineResponse`). Accepts `from=<ISO8601>` and `to=<ISO8601>` query
  params for timestamp range filtering. Each entry includes `log_index`,
  `timestamp_unix`, `timestamp_iso`, `event_type`, and per-event IDs.
- **`EventJournal`** — now stamps each committed event with a wall-clock
  unix-second timestamp. New methods: `committed_with_timestamps()`,
  `find_log_index_at_or_before()`, `event_timestamp()`.
- **Python SDK** — `SyncRemoteClient.search()` and `AsyncRemoteClient.search()`
  gain `as_of` and `as_of_log_index` params. New `timeline()` method on both.
- **6 new integration tests** in `crates/valori-node/tests/api_as_of.rs`.

### Added (Phase 2.10d — Partition Harness)
- **`crates/valori-consensus/tests/partition_scenarios.rs`** — three new
  integration tests for the in-process partition harness:
  - `asymmetric_partition_lagging_node_catches_up` — one-directional link block
    (leader → follower); 2/3 quorum commits; lagging node catches up and all
    three BLAKE3 hashes converge.
  - `blake3_chain_consistent_across_partition_and_heal` — full compliance proof:
    isolated-leader's hash is frozen during a symmetric partition, and after heal
    all 3 replicas share the same BLAKE3 state hash over all 6 records.
  - `isolated_node_hash_frozen_then_converges` — confirms an isolated follower
    cannot fork the audit chain; hash is frozen during isolation and adopts the
    majority chain after heal.
- All 3 new tests pass (0.73 s); full `valori-consensus` suite clean.

### Added (C3 — Self-Maintaining Memory)
- **Global entity registry** (`ui/src/app/api/ingest/route.ts`) — before creating a
  Concept node, checks `entity:<collection>:<normalized_label>` in the metadata sidecar.
  Existing nodes are reused across documents and ingest sessions so the same real-world
  entity converges to a single graph node.
- **Content dedup** — per-chunk SHA-256 computed before embedding. Exact duplicates
  (`content:<collection>:<sha>` already registered) skip the vector insert entirely.
  `dedup_skipped` count returned in ingest response; `dedup: true` flag per chunk.
  `content_sha256` stored in sidecar for external verification.
- **Contradiction detection** — after each ingest, `detectContradictions()` runs
  async (fire-and-forget). Similarity > 0.92 with a different source document queues
  a `contradiction:<id>` entry with `status: "pending"`.
- **`GET /api/contradictions`** — lists pending/dismissed/superseded contradictions
  for a collection with chunk text preview.
- **`POST /api/contradictions`** — resolve: `dismiss` (both valid) or `supersede_b`
  (marks `record_b` sidecar as `superseded: true`).
- **Supersession filter in `/api/why`** — chunks with `metadata.superseded === true`
  are excluded from vector search results. Kernel record is immutable (audit trail
  preserved); only retrieval is suppressed.

### Added (C2 — Audited Entity Graph + Provenance Receipt)
- **`GET /graph/subgraph?root=<id>&depth=<d>`** — bounded BFS (depth capped at 4)
  returning all reachable nodes and edges. Added to both `server.rs` (standalone)
  and `cluster_server.rs` (cluster, respects readiness gate).
- **Entity extraction at ingest** (`ui/src/app/api/ingest/route.ts`) — when
  contextual enrichment is enabled, extracts up to 8 named entities per chunk via
  the configured LLM. Creates `NodeKind::Concept` nodes + `EdgeKind::Mentions`
  edges (chunk → concept), deduplicated within the ingest session via a
  `entityNodeMap`. Entity labels are stored in the metadata sidecar.
- **Provenance subgraph in receipt** (`ui/src/app/api/why/route.ts`) — after
  graph expansion, calls `/graph/subgraph?depth=1` for each top-5 chunk node and
  collects traversed nodes + edges. Entity labels fetched for Concept nodes.
- **Receipt schema** (`ui/src/lib/receipts.ts`) — `ReceiptGraphNode` and
  `ReceiptGraphEdge` interfaces added. `ServerReceiptPart` and `AnswerReceipt`
  gain `provenance_nodes` and `provenance_edges` arrays.
- **Bug fix**: `Document→Chunk` edge kind corrected from `0` (Relation) to `6`
  (ParentOf) in the ingest route.

### Added (C1 — Contextual Retrieval + Audited Enrichment)
- **Audited context sentences** — `BatchInsertRequest` now accepts
  `metadata: Option<Vec<Option<String>>>`. Per-vector UTF-8 metadata blobs are
  committed into `KernelEvent::InsertRecord.metadata` / `AutoInsertRecord.metadata`,
  included in the BLAKE3 audit chain, and replicated through Raft. The cluster ingest
  path (`cluster_server.rs`) previously always passed `metadata: None` — fixed.
- **Contextual enrichment at ingest** (`ui/src/app/api/ingest/route.ts`) — when
  enabled, generates a one-sentence LLM context per chunk before embedding and
  commits it as `{"doc","n","total","ctx"}` JSON in the audited metadata field.
  Concurrency limit: 6 parallel LLM calls via `Promise.allSettled`. Failure is
  graceful (ingest continues without enrichment, `enriched: false` in receipt).
- **Tier-2 reranker** (`ui/src/app/api/why/route.ts`) — optional cross-encoder
  reranker (Cohere or custom endpoint) applied after vector search. Failure is
  silent. `rerank_score: number | null` per chunk + `reranked: boolean` flag are
  written into the proof receipt so non-determinism is documented, not hidden.
- **Receipt schema** (`ui/src/lib/receipts.ts`) — `ReceiptChunkRef` gains
  `rerank_score: number | null` and `enriched: boolean`. Both additive, no version
  bump needed within `"1.0"`.
- **Settings → Tier-2 Reranker** (`ui/src/app/settings/page.tsx`) — Disabled /
  Cohere / Custom endpoint toggle persisted in `localStorage["valori:reranker_config"]`.
- **DocumentUploadTab** (`ui/src/components/ingestion/DocumentUploadTab.tsx`) — adds
  per-upload contextual enrichment toggle that passes LLM params to the ingest route.
- **AskTab** (`ui/src/components/collections/AskTab.tsx`) — loads reranker config
  from localStorage and passes it to `/api/why` on each question.

### Added (C0 — Eval Harness)
- **`scripts/eval/eval.py`** — Python eval harness with three subcommands: `probe`
  (health check, no embedding needed), `seed-eval` (seeds 10 records, embeds,
  searches, measures recall@k + provenance integrity; CI gate exits 1 if
  recall@1 < 0.8 or citation_existence < 1.0), `verify` (verifies
  `content_sha256` in saved receipt JSON files against a live node).
- **`scripts/eval/qa_sets/bootstrap.jsonl`** — 10 bootstrap QA entries labeled
  `[bootstrap]`. Not for external claims; replaced with real corpus when available.
- **`ui/src/lib/receipts.ts`** — receipt schema frozen at `version: "1.0"`.
  Breaking changes must bump `RECEIPT_VERSION`.
- **`docs/phases/phase-C0-cortex-plan.md`** — full converged Cortex plan (5
  contradiction cycles, 34 items, 4-point moat statement).

### Fixed (B13 — Startup Readiness Gate)
- **Partial-state-on-restart bug fixed** (`valori-node`) — cluster nodes no longer
  serve `Local`-consistency reads during the openraft log-replay catch-up window that
  follows a restart. Reads now return HTTP 503 (`Retry-After: 1`) until the node has
  replayed all entries committed before shutdown.
- **`ReadinessGate`** added to `cluster_server.rs` — atomic latch initialized from
  `startup_committed_index` (read from the redb `KEY_COMMITTED` entry before Raft
  opens). Latch opens permanently once `applied_index >= startup_committed_index`;
  fresh/in-memory nodes get `target=0` and are immediately ready.
- **Explicit snapshot cadence** (`cluster.rs`) — `SnapshotPolicy::LogsSinceLast(n)`
  now explicitly configured (default 5000, overridable via
  `VALORI_SNAPSHOT_EVERY_EVENTS`) instead of relying on openraft's implicit default,
  bounding the maximum catch-up window after restart.

### Added (B13 — env vars)
- `VALORI_SNAPSHOT_EVERY_EVENTS` — trigger a Raft snapshot every N applied entries
  (default 5000). Lower values reduce restart catch-up latency at the cost of more
  frequent snapshot I/O.
- `VALORI_RAFT_SNAPSHOT_KEEP` — log entries to retain after snapshot for followers
  that are slightly behind (default 1000).

### Added (Phase 3.2 — Rolling Upgrades)
- **`schema_version` field on `ClientRequest`** (`valori-consensus`) — the
  leader stamps `CURRENT_SCHEMA_VERSION` (currently `0`) on every proposal. Old
  nodes decode the field as `0` via `#[serde(default)]`.
- **`CURRENT_SCHEMA_VERSION: u8 = 0`** constant (`valori-consensus::types`) —
  single source of truth for the cluster wire version. Bump when a new
  `KernelEvent` variant or breaking field change requires newer followers.
- **Schema version gate in `ValoriStateMachine::apply()`** — followers reject
  entries with `schema_version > CURRENT_SCHEMA_VERSION` with `StorageError`
  (halts replication on that node; cluster continues through remaining quorum).
  State and audit log are untouched on rejection.
- **`valori cluster upgrade --url … --target-version …`** CLI command — interactive
  guided rolling upgrade: discovers topology, upgrades non-leaders first then
  leader, polls `/health` after each restart, waits for re-election before
  declaring the leader step complete.
- **`docs/COMPATIBILITY.md`** — schema version history, rolling-window rules,
  coexistence matrix, and the procedure for bumping `CURRENT_SCHEMA_VERSION`.

### Fixed (Phase 3.2)
- `corrupted_snapshot_payload_is_refused_and_state_kept` snapshot corruption
  test was flipping byte `bytes.len() / 2` which, for V6 snapshots (8318 bytes),
  lands in the namespace sentinel region not covered by `hash_state_blake3`.
  Fixed to corrupt `bytes.last_mut()` (last byte of the `state_hash` tail),
  which always triggers the hash mismatch check regardless of format version.

---

## [0.2.1] — 2026-06-19

### Added
- **Multi-tenant collections** — up to 1 024 named namespaces per node.
  `POST /v1/namespaces`, `GET /v1/namespaces`, `DELETE /v1/namespaces/:name`.
  All data endpoints accept an optional `"collection"` field. Records are
  isolated at the kernel level via intrusive per-namespace linked lists enforced
  at three independent points (event-commit, WAL replay, `build_index`).
- **`AutoCreateNode` / `AutoCreateEdge` kernel events** — graph mutations with
  IDs assigned at apply time for deterministic cluster-mode graph operations.
- **Persistent Raft state machine** — when `VALORI_RAFT_LOG_PATH` is set, the
  state machine shares the redb file and persists `last_applied`, membership,
  and the latest snapshot, preventing duplicate audit-log writes on restart.
- **Replay suppression** — `replay_until` suppresses already-written audit
  entries when openraft replays committed log entries after a restart.
- **`GET /v1/cluster/role`** — current node's Raft role for load-balancer routing.
- **`state_hash_match` Prometheus gauge** — cluster-wide hash-convergence metric.
- **Snapshot V6 format** — per-record `namespace_id` + linked-list pointers,
  2 × 1 024 × 4 = 8 KB namespace heads arrays, and a backward-compatible NSRG
  section (namespace registry as JSON, detected by `"NSRG"` magic tag).
- **Python SDK collection API** — `create_collection`, `list_collections`,
  `drop_collection` on both `SyncRemoteClient` and `AsyncRemoteClient`;
  `collection` parameter on all data methods; `consistency` parameter on search.
- **Threat model** (`docs/THREAT_MODEL.md`).
- **Capacity planning** (`docs/CAPACITY.md`).
- **DR & multi-region runbook** (`docs/DR.md`).
- **Multi-arch hash benchmark** (`benchmarks/multi_arch_hash.py`).
- **Q16.16 precision benchmark** (`benchmarks/q16_precision.py`).
- **Helm snapshot CronJob** (`deploy/helm/valori/templates/snapshot-cronjob.yaml`).
- **CI test-count workflow** (`.github/workflows/test-count.yml`).

### Fixed
- `LeaderClient::get_proof()` wire-format mismatch — server returns
  `{"final_state_hash":"<hex>"}` but client expected `[u8; 32]`. Added
  `LeaderProof { final_state_hash: String }` and updated hex comparison in replication.
- Snapshot buffer too small for V6 in `format.rs` and `snapshot_roundtrip.rs`
  (4 KB → 16 KB).
- `spawn_state_hash_watcher` held `Arc<Database>` indefinitely, blocking redb
  file re-open on restart. Now returns `JoinHandle`, stored in `ClusterHandle`,
  aborted and awaited before shutdown.
- arXiv paper title corrected from *"Deterministic Memory: A Substrate for
  Verifiable AI Agents"* to *"Valori: A Deterministic Memory Substrate for
  AI Systems"* in README and BibTeX.
- Hardcoded test count badge (271) replaced with CI-driven workflow badge.
- Python SDK version badge corrected from v0.1.11 to v0.2.1.
- Apply-vs-audit ordering invariant now explicitly documented with crash-window
  analysis in `valori-consensus/README.md`.
- Comparison table "No" cells now cite competitor documentation.

### `valori_raft_state_hash_match` Prometheus gauge — a background task on
  each cluster node periodically calls `/v1/proof/state` on every peer and
  publishes `1` when all reachable nodes agree on the BLAKE3 state hash, `0`
  when any peer diverges. Mismatches are also logged at `ERROR` level and
  counted by `valori_raft_divergence_detections_total`. Configurable via
  `VALORI_STATE_HASH_CHECK_SECS` (default 30 s; `0` disables).
- **`GET /v1/cluster/role`** endpoint — returns `{"role":"leader"|"follower",
  "node_id":N,"current_leader":N}` on any node. Designed for load-balancer
  health-check routing: steer writes at the pod that answers `"leader"` to
  avoid 307 redirect round trips on every write.
- **Proptest event-sequence fuzz** (`crates/valori-consensus/tests/proptest_event_fuzz.rs`)
  — 32 randomly generated insert/soft-delete/delete sequences applied through
  a 3-node in-process cluster, asserting all nodes converge to the same BLAKE3
  state hash after each sequence. Shrinks failing cases automatically.
- **Helm chart** (`deploy/helm/valori/`) — production StatefulSet with
  PersistentVolumeClaims for `events.log` and `raft.redb`, headless service
  for stable pod DNS, client service, and configurable liveness/readiness
  probes pointing at `/v1/cluster/health` and `/health`. Topology spread
  anti-affinity keeps pods on separate availability zones by default.

- **Automatic `events.log` rotation** on both write paths — the standalone
  `EventCommitter` and the cluster `EventLogAuditSink` seal the live segment to
  `events.log.NNNNNN` once it passes `VALORI_EVENT_LOG_ROTATION_BYTES` (default
  256 MiB; `0` disables), opening a fresh segment that splices from the sealed
  one's chain head.
- **Multi-segment recovery** — recovery now discovers and replays every local
  segment (sealed archives + live file) in sequence order and verifies each
  splice point.

- **Linearizable reads via the read-index protocol** (now the default read
  consistency). The leader serves through openraft's `ensure_linearizable()`;
  a follower fetches the leader's read index from the new
  `GET /v1/cluster/read-index` endpoint, then waits for its own apply to catch
  up before scanning local state. Clients can opt into a faster,
  eventually-consistent read with `consistency: "local"` (Python SDK:
  `search(..., consistency="local")`).

### Fixed
- Rotated logs previously recovered **only the live segment**, silently dropping
  all pre-rotation history; recovery is now multi-segment and lossless.
- Archive segments are named by monotonic segment sequence instead of a
  wall-clock timestamp, so two rotations within the same second no longer
  collide and clobber an earlier archive.

## [0.2.0] — 2026-06-13

The multi-node release. Valori graduates from a single standalone node to a
Raft-replicated cluster with verifiable, crash-symmetric state on every replica.

### Added
- **Raft consensus layer** (`valori-consensus`) over openraft 0.9: replicated
  log store (in-memory + persistent `redb`), `KernelState` state machine with
  the audit-log write at apply time, and a tonic/gRPC peer transport.
- **Cluster mode** for `valori-node`: boot-time dispatch on
  `VALORI_CLUSTER_MEMBERS`, leader-redirect (`307 + Location`) for writes,
  local reads on any replica, and a `/v1/cluster/*` management plane
  (status, health, add-node, remove-node).
- **Mutual TLS** on the Raft channel (`VALORI_TLS_*`), enforced at the
  handshake against a shared cluster CA.
- **Persistent Raft log** via embedded `redb` (`VALORI_RAFT_LOG_PATH`) — the
  log and vote survive process restarts.
- **Raft metrics** exported on `/metrics` (term, leader, log/apply lag,
  snapshot/purge indexes).
- **State-machine ID allocation** (`KernelEvent::AutoInsertRecord`): record IDs
  are assigned deterministically at apply time, removing the per-node insert
  mutex and retry loop.
- **Cluster data-plane endpoints**: `/v1/delete`, `/v1/soft-delete`,
  `/v1/vectors/batch_insert`, `/v1/proof/state`.
- **Interactive setup wizard** (`valori setup`): pick architecture and node
  count, start an in-process cluster, and drive inserts/search/membership from
  a live menu. Projects persist to `~/.valori/projects.json`.
- **`valori cluster` CLI**: operate a running cluster (status, health,
  add-node, remove-node) against any node's HTTP API.
- **Docker deployment**: distroless multi-stage `Dockerfile` with a built-in
  `--health-check` TCP probe, and a 3-node `docker-compose.yml`.
- **Partition harness**: in-memory switchable-transport test suite covering
  leader isolation, re-election, partition heal/convergence, and the
  minority-cannot-commit invariant.

### Changed
- Cluster search now uses the kernel's maintained index via `search_l2`
  instead of an ad-hoc record-pool scan.
- Workspace versioning unified at `0.2.0` via `[workspace.package]`; all crates
  inherit version, edition, and license.

### Fixed
- `Dockerfile` now copies all workspace member manifests so workspace
  resolution succeeds; healthcheck no longer references a non-existent flag.

### Repository
- Removed scratch and stale top-level files; relocated manual/e2e/benchmark
  scripts under `scripts/`.
- Tightened `.gitignore` for runtime database directories and caches.

[Unreleased]: https://github.com/valori-db/valori-kernel/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/valori-db/valori-kernel/releases/tag/v0.2.0
