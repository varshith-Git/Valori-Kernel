# Changelog

All notable changes to Valori are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
