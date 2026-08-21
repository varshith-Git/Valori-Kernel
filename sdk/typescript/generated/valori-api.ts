//
// GENERATED FILE — DO NOT EDIT.
// Source of truth: api/openapi/valori-v1.yaml
// Regenerate with: sdk/typescript/scripts/generate.sh
//
/* eslint-disable */
/* tslint:disable */
/*
 * ---------------------------------------------------------------
 * ## THIS FILE WAS GENERATED VIA SWAGGER-TYPESCRIPT-API        ##
 * ##                                                           ##
 * ## AUTHOR: acacode                                           ##
 * ## SOURCE: https://github.com/acacode/swagger-typescript-api ##
 * ---------------------------------------------------------------
 */

/**
 * The canonical error body, as a schema-bearing DTO.
 *
 * [`valori_engine::EngineError`] produces this shape at runtime but lives in
 * a crate that does not depend on `utoipa`, so the schema is declared here —
 * the translation layer §36 asks for, rather than a re-export of an internal
 * type.
 */
export interface ApiError {
  /** Stable machine-readable code. Branch on this. */
  code: ErrorCode;
  /** Human-readable message. Do not parse. */
  error: string;
}

export interface ArchiveWalRequest {
  /** Absolute path on this node's local filesystem to the sealed segment. */
  path: string;
}

export interface ArchiveWalResponse {
  key: string;
  /**
   * @format int64
   * @min 0
   */
  size_bytes: number;
}

export interface BatchInsertRequest {
  batch: number[][];
  collection?: string | null;
  /**
   * Optional per-vector metadata blobs (UTF-8 JSON strings).
   * If present, must be the same length as `batch`.
   * Each entry is committed inside the `InsertRecord` event and is
   * therefore included in the BLAKE3 audit chain.
   */
  metadata?: (string | null)[] | null;
  /**
   * Per-item idempotency keys (32-hex strings = 16-byte UUIDs).
   * If present, must be the same length as `batch`. A null entry means
   * "no dedup key for this item". A repeated key causes that item to be
   * skipped and the previously assigned ID is returned instead.
   */
  request_ids?: (string | null)[] | null;
  /**
   * Optional per-vector text strings for BM25 hybrid reranking.
   * If present, must be the same length as `batch`. A null entry means
   * no text is stored for that vector. Text is tokenised and indexed
   * so that future /search calls with `rerank=true` can re-score results.
   */
  texts?: (string | null)[] | null;
}

export interface BatchInsertResponse {
  ids: number[];
}

/**
 * The index kinds `POST /v1/namespaces/{name}/index` will actually build.
 *
 * Phase API-3.3: narrower than the project-wide `IndexKindInput`, and
 * deliberately so. The build task in both routers matches on exactly three
 * strings — `"hnsw"`, `"ivf"`, `"bq"` — and its `_` arm returns
 * `"unknown index type '<x>'"`. `brute` and `auto` are project-level
 * selections, not buildable per-collection ANN structures, so they are not
 * members here; sending one is an error, and the schema now says so instead
 * of advertising an open `string`.
 *
 * `null` is the fourth valid value and means *drop the index*. It is carried
 * by the `Option`, not by a variant.
 */
export enum BuildableIndexKind {
  Hnsw = "hnsw",
  Ivf = "ivf",
  Bq = "bq",
}

/** `GET /v1/cluster/health` response. */
export interface ClusterHealthResponse {
  /** Present only on the `no-leader` path. */
  detail?: string | null;
  leader?: null | U64;
  /** `ok` when a leader is visible, `no-leader` otherwise. */
  status: string;
}

/** The `cluster` sub-object of `GET /health`. Cluster mode only. */
export interface ClusterHealthStats {
  /**
   * Vector dimension the cluster locked on first insert, if any.
   * @format int32
   * @min 0
   */
  dim?: number | null;
  /**
   * Node id of the leader this node currently sees, if any.
   * @format int64
   * @min 0
   */
  leader?: number | null;
  /** Raft role of this node (`Leader`, `Follower`, `Candidate`, `Learner`). */
  role: string;
  /** `ok` when this node sees an elected leader, `no-leader` otherwise. */
  status: string;
  /**
   * @format int64
   * @min 0
   */
  term: number;
}

/** `GET /v1/cluster/proof` response. */
export interface ClusterProofResponse {
  /** 64 lowercase hex characters (32 bytes). */
  final_state_hash: string;
  /**
   * Raft index this hash was taken at. Two peers only need to agree when
   * compared at the same index.
   * @format int64
   * @min 0
   */
  last_applied_index?: number | null;
  /**
   * @format int64
   * @min 0
   */
  node_id: number;
  /**
   * @format int64
   * @min 0
   */
  term: number;
}

/** `GET /v1/cluster/role` response. */
export interface ClusterRoleResponse {
  /** The leader this node currently believes in, if any. */
  current_leader?: null | U64;
  node_id: U64;
  /** `leader` or `follower`. Both are healthy. */
  role: string;
}

export interface CollectionInfo {
  /**
   * Present only for collections created with an explicit vector config.
   * @format int32
   * @min 0
   */
  dimension?: number | null;
  /**
   * @format int32
   * @min 0
   */
  id: number;
  index?: null | IndexKind;
  /** @min 0 */
  max_records?: number | null;
  metric?: null | Metric;
  name: string;
  /** @min 0 */
  record_count?: number | null;
}

export interface CommunityDetectRequest {
  /**
   * @format int32
   * @min 0
   */
  max_iter?: number | null;
  namespace?: string | null;
}

export interface CommunityDetectResponse {
  communities: CommunitySummary[];
  /** @min 0 */
  community_count: number;
  /** @min 0 */
  node_count: number;
  /** BLAKE3 hex receipt over sorted assignments. */
  receipt: string;
}

export interface CommunityHit {
  /**
   * @format int32
   * @min 0
   */
  community_id: number;
  /** @min 0 */
  member_count: number;
  sample_node_ids: number[];
  /** @format float */
  score: number;
}

/** One community in `GET /v1/community/overview`, largest first. */
export interface CommunityOverviewEntry {
  /** Mean vector of the community's members. */
  centroid: number[];
  /**
   * @format int32
   * @min 0
   */
  community_id: number;
  /** @min 0 */
  member_count: number;
  /** Up to 10 member node ids, for a cheap preview. */
  sample_node_ids: number[];
}

/** `GET /v1/community/overview` — requires `POST /v1/community/detect` first. */
export interface CommunityOverviewResponse {
  communities: CommunityOverviewEntry[];
  /** @min 0 */
  community_count: number;
  /** @min 0 */
  node_count: number;
  /** BLAKE3 receipt over the sorted community assignment. */
  receipt: string;
}

export interface CommunitySearchRequest {
  /**
   * @format int32
   * @min 0
   */
  depth?: number;
  drill_in?: boolean;
  /** @min 0 */
  k?: number;
  namespace?: string | null;
  vector: number[];
}

export interface CommunitySearchResponse {
  communities: CommunityHit[];
  /** @min 0 */
  total_communities_searched: number;
}

export interface CommunitySummary {
  /**
   * @format int32
   * @min 0
   */
  centroid_record_id?: number | null;
  /**
   * @format int32
   * @min 0
   */
  community_id: number;
  /** @min 0 */
  member_count: number;
}

export interface CreateCollectionRequest {
  /**
   * Vector dimension for this collection. **Required for every name** —
   * `"default"` included, with no exception (Phase 3.3). Valori does not
   * infer a collection's dimension from its first insert or from any
   * project/env-level default; `POST` without it is rejected with 400.
   * Immutable after creation; a later request with a different value for
   * the same collection is rejected, not silently applied.
   *
   * The Rust field stays `Option` so that a missing value reaches
   * `parse_collection_config` and is answered with that explanatory 400
   * rather than a generic deserialization failure. `required = true`
   * records the contract-level truth the handler enforces, so a generated
   * SDK makes the argument mandatory instead of silently omittable.
   * @format int32
   * @min 0
   */
  dimension: number | null;
  /**
   * Index algorithm: `"brute"`, `"hnsw"`, `"ivf"`, `"bq"`, or `"auto"`.
   * Optional — omitting it means `index = NONE` (exact
   * namespace-specific search, no dedicated ANN structure), a
   * first-class supported state, not a missing feature. Requires
   * `dimension` to also be set — a collection cannot have an explicit
   * index without an explicit dimension, since the index is constructed
   * for that dimension at creation time.
   */
  index?: null | IndexKindInput;
  /**
   * Distance metric. **Required for every name** — `"default"` included,
   * with no exception (Phase 3.3). Sent explicitly over the wire, never
   * assumed. Only `"squared_l2"` is supported today; present as a field
   * (not hard-coded) so the wire contract does not need to change when a
   * second metric is added.
   *
   * See `dimension` for why this is `Option` in Rust but `required` in the
   * contract.
   */
  metric: null | MetricInput;
  name: string;
}

export interface CreateCollectionResponse {
  created: boolean;
  /**
   * @format int32
   * @min 0
   */
  id: number;
  name: string;
}

export interface CreateEdgeRequest {
  collection?: string | null;
  /**
   * @format int32
   * @min 0
   */
  from: number;
  /**
   * @format int32
   * @min 0
   */
  kind: number;
  /**
   * @format int32
   * @min 0
   */
  to: number;
}

export interface CreateEdgeResponse {
  /**
   * @format int32
   * @min 0
   */
  edge_id: number;
  /**
   * Raft log index of the committed write — cluster path only.
   * @format int64
   * @min 0
   */
  log_index?: number | null;
}

export interface CreateNodeRequest {
  collection?: string | null;
  /**
   * @format int32
   * @min 0
   */
  kind: number;
  /**
   * @format int32
   * @min 0
   */
  record_id?: number | null;
}

export interface CreateNodeResponse {
  /**
   * Raft log index of the committed write — cluster path only.
   * @format int64
   * @min 0
   */
  log_index?: number | null;
  /**
   * @format int32
   * @min 0
   */
  node_id: number;
}

export interface CryptoStatusResponse {
  exists: boolean;
  key_id: string;
}

export interface DeleteNodeResponse {
  /**
   * Raft log index of the committed write — cluster path only.
   * @format int64
   * @min 0
   */
  log_index?: number | null;
  success: boolean;
}

export interface DeleteRecordRequest {
  collection?: string | null;
  /**
   * @format int32
   * @min 0
   */
  id: number;
}

export interface DeleteRecordResponse {
  /**
   * Raft log index of the committed write — cluster path only.
   * @format int64
   * @min 0
   */
  log_index?: number | null;
  success: boolean;
}

export interface EdgeData {
  /**
   * @format int32
   * @min 0
   */
  edge_id: number;
  /**
   * @format int32
   * @min 0
   */
  kind: number;
  /**
   * @format int32
   * @min 0
   */
  to_node: number;
}

/**
 * The `engine` sub-object of `GET /health`. Standalone mode only; absent in
 * cluster mode, where the node has no single in-process engine to describe.
 */
export interface EngineHealthStats {
  /** @min 0 */
  collections: number;
  /** Slab occupancy for one kernel pool (records, graph nodes, graph edges). */
  edges: PoolStatsSchema;
  embed_enabled: boolean;
  embed_provider?: string | null;
  /**
   * @format int64
   * @min 0
   */
  event_log_height?: number | null;
  event_log_path?: string | null;
  /** Slab occupancy for one kernel pool (records, graph nodes, graph edges). */
  nodes: PoolStatsSchema;
  /** `event_log`, `wal`, `snapshot`, or `none`. */
  persistence: string;
  /** Slab occupancy for one kernel pool (records, graph nodes, graph edges). */
  records: PoolStatsSchema;
  /** @min 0 */
  shard_count: number;
  snapshot_path?: string | null;
  status: string;
  version: string;
}

/**
 * Mirror of [`valori_engine::ErrorCode`] for schema generation.
 *
 * `tests/api_contract.rs` diffs the runtime enum against the committed YAML;
 * this type exists so the generated document carries the same closed set.
 */
export enum ErrorCode {
  ValidationError = "validation_error",
  Unauthorized = "unauthorized",
  Forbidden = "forbidden",
  NotFound = "not_found",
  CollectionNotFound = "collection_not_found",
  RecordNotFound = "record_not_found",
  DimensionMismatch = "dimension_mismatch",
  InvalidMetric = "invalid_metric",
  InvalidIndex = "invalid_index",
  IndexBuildFailed = "index_build_failed",
  Conflict = "conflict",
  CapacityExceeded = "capacity_exceeded",
  NotLeader = "not_leader",
  Unavailable = "unavailable",
  NotImplemented = "not_implemented",
  InternalError = "internal_error",
}

export interface EventProofResponse {
  /**
   * @format int64
   * @min 0
   */
  committed_height: number;
  /**
   * @format int64
   * @min 0
   */
  event_count: number;
  event_log_hash: string;
  final_state_hash: string;
  /**
   * @format int32
   * @min 0
   */
  kernel_version: number;
  snapshot_hash?: string | null;
}

/**
 * A completed ingest execution, keyed by `operation_id` — the real payload
 * for `GET /v1/operations/:id/execution`.
 */
export interface ExecutionRecord {
  /** @min 0 */
  chunks_produced: number;
  collection: string;
  document_source: string;
  error?: string | null;
  operation_id: string;
  /**
   * Present when the operation's receipt was emitted before this record
   * was built — always true for the standalone `/v1/ingest` path.
   */
  receipt_id?: string | null;
  /** @min 0 */
  records_written: number;
  stages: StageView[];
  state_hash_after?: string | null;
  state_hash_before?: string | null;
  success: boolean;
  /**
   * @format int64
   * @min 0
   */
  total_duration_ms: number;
}

export interface ExtractEntitiesRequest {
  entity_types?: string[];
  model?: string | null;
  namespace?: string | null;
  text: string;
}

export interface ExtractEntitiesResponse {
  entities: InsertedEntity[];
  /** @min 0 */
  entity_count: number;
  /** @min 0 */
  relationship_count: number;
  relationships: InsertedRelationship[];
  /** @min 0 */
  skipped_relationships: number;
}

export interface GetEdgesResponse {
  edges: EdgeData[];
}

export interface GetNodeResponse {
  /**
   * @format int32
   * @min 0
   */
  kind: number;
  /**
   * @format int32
   * @min 0
   */
  namespace_id: number;
  /**
   * @format int32
   * @min 0
   */
  record_id?: number | null;
}

export interface GraphQueryHitDto {
  /**
   * @format int32
   * @min 0
   */
  depth: number;
  /**
   * @format int32
   * @min 0
   */
  kind: number;
  /**
   * @format int32
   * @min 0
   */
  node_id: number;
  /**
   * @format int32
   * @min 0
   */
  record_id?: number | null;
}

export interface GraphQueryResponse {
  /** @min 0 */
  count: number;
  hits: GraphQueryHitDto[];
}

/**
 * One blended vector + graph hit from `POST /v1/graphrag`.
 *
 * Phase API-3.3: `GraphRagResponse.hits` was `Vec<Object>`, so GraphRAG — a
 * headline retrieval feature — returned `object[]` to every generated SDK.
 * The producer is `capabilities.rs`, which builds a fixed ten-key object.
 *
 * Several scores are nullable by design: a hit reached purely through graph
 * expansion has no vector distance, so `score` and `vector_score` are `null`
 * on it. `final_score` and `graph_score` are always present.
 */
export interface GraphRagHit {
  /**
   * Combined score in `[0, 1]`. Always present; rank on this.
   * @format float
   */
  final_score: number;
  /**
   * Hop count from the nearest seed node, when reachable.
   * @format int32
   * @min 0
   */
  graph_distance?: number | null;
  /**
   * Normalised graph relevance in `[0, 1]`.
   * @format float
   */
  graph_score: number;
  /** Stable memory identity, `rec:<record_id>`. */
  memory_id: string;
  /** Caller-supplied metadata stored alongside the record, if any. */
  metadata?: Partial<Record<string, any>> | null;
  /**
   * Graph node for this record, when it has one.
   * @format int32
   * @min 0
   */
  node_id?: number | null;
  /**
   * The underlying record.
   * @format int32
   * @min 0
   */
  record_id: number;
  /**
   * Vector distance. `null` for a graph-only hit. Retained for backward
   * compatibility; `vector_score` is the explicit spelling of the same value.
   * @format float
   */
  score?: number | null;
  /** How this hit entered the result set — e.g. `vector`, `graph`. */
  source: string;
  /**
   * Vector distance. `null` for a graph-only hit.
   * @format float
   */
  vector_score?: number | null;
}

export interface GraphRagRequest {
  collection?: string | null;
  /**
   * @format int32
   * @min 0
   */
  depth?: number;
  /**
   * Maximum returned hits. Absent = defaults to `retrieval_k` (Phase 5.4).
   * @min 0
   */
  final_k?: number | null;
  /**
   * Phase 5.4: β in `final_score = (1-β)×vector_rel + β×graph_rel`. Range [0,1].
   * @format float
   */
  graph_weight?: number;
  /**
   * Legacy alias for `retrieval_k`. When `retrieval_k` is absent, `k` is used.
   * @min 0
   */
  k?: number | null;
  /**
   * Phase 5.4: halt edge emission once this count is reached per BFS round.
   * @min 0
   */
  max_edges?: number | null;
  /**
   * Budget on graph-only candidates (applied before `final_k`). Absent = 100.
   * @min 0
   */
  max_graph_candidates?: number | null;
  /**
   * Phase 5.4: halt BFS before visiting a node that would exceed this count.
   * @min 0
   */
  max_nodes?: number | null;
  query_vector: number[];
  /**
   * How many vector candidates to use as seeds for graph expansion.
   * @min 0
   */
  retrieval_k?: number | null;
}

/**
 * `POST /v1/graphrag` — K nearest vectors plus the connected subgraph around
 * them, read from one consistent kernel snapshot.
 */
export interface GraphRagResponse {
  /** Blended vector + graph hits, best first. */
  hits: GraphRagHit[];
  /** Graph node ids the vector hits seeded the expansion from. */
  seed_nodes: number[];
  /** `GET /v1/graph/subgraph` — a BFS expansion around one root node. */
  subgraph: SubgraphResponse;
}

export interface GraphRerankRequest {
  /**
   * `"outgoing"` (default) | `"incoming"` | `"both"` — case-insensitive,
   * same convention as `GET /v1/graph/query`.
   */
  direction?: string | null;
  /**
   * Max hop count from the seed set. Clamped to `query_graph`'s own
   * `MAX_DEPTH` (4), never rejected.
   * @format int32
   * @min 0
   */
  max_depth?: number;
  /**
   * Number of top vector hits to resolve as graph seeds. Clamped
   * `[1, 10]` server-side (never rejected).
   * @min 0
   */
  seed_count?: number;
  /**
   * Multiplier weight per hop of graph distance. Clamped `[0.0, 1.0]`
   * server-side. `adjusted = score * (1 + weight * distance)`.
   * @format float
   */
  weight?: number;
}

export interface HealthResponse {
  cluster?: null | ClusterHealthStats;
  /** @min 0 */
  collections?: number | null;
  /**
   * The vector dimension the cluster has locked to, or the configured
   * dimension when nothing has been inserted yet. Cluster mode only.
   * @format int32
   * @min 0
   */
  dim?: number | null;
  edges?: null | PoolStatsSchema;
  embed_enabled?: boolean | null;
  embed_provider?: string | null;
  engine?: null | EngineHealthStats;
  /**
   * @format int64
   * @min 0
   */
  event_log_height?: number | null;
  /**
   * Node id of the leader this node currently sees. Cluster mode only.
   * @format int64
   * @min 0
   */
  leader?: number | null;
  /**
   * @format int64
   * @min 0
   */
  leader_id?: number | null;
  /** @min 0 */
  members?: number | null;
  mode: string;
  /**
   * @format int64
   * @min 0
   */
  node_id?: number | null;
  nodes?: null | PoolStatsSchema;
  persistence?: string | null;
  raft_state?: string | null;
  records?: null | PoolStatsSchema;
  role?: string | null;
  /** @min 0 */
  shard_count: number;
  state_hash?: string | null;
  status: string;
  /**
   * @format int64
   * @min 0
   */
  term?: number | null;
  version: string;
}

export interface HnswConfigView {
  /** @min 0 */
  m_max0: number;
  /** @min 0 */
  ef_construction: number;
  /** @min 0 */
  ef_search: number;
  /** @min 0 */
  m: number;
}

/**
 * The tuning knobs `POST /v1/namespaces/{name}/index` actually reads.
 *
 * Phase API-3.3: [`IndexBuildRequest::parameters`] is a `serde_json::Value`,
 * which utoipa rendered as a schema with no `type` at all — `unknown` in
 * TypeScript, `Any` in Python, and nothing whatsoever for a user to discover
 * the knob names from. It was the only genuinely untyped field in the public
 * surface.
 *
 * The runtime is not actually open-ended: both routers read exactly five
 * keys, all unsigned integers — `m`, `ef_construction`, `ef_search` for HNSW
 * (`server.rs` / `cluster_server.rs`, the `"hnsw"` arm) and `n_list`,
 * `n_probe` for IVF (the `"ivf"` arm). This type names them.
 *
 * `additionalProperties` stays open because the documented behaviour is that
 * unknown keys are *ignored*, not rejected — so a client sending one is not
 * making an error, and the schema must not claim otherwise.
 */
export interface IndexBuildParameters {
  /**
   * HNSW: candidate-list size during construction.
   * @format int64
   * @min 0
   */
  ef_construction?: number | null;
  /**
   * HNSW: candidate-list size during search.
   * @format int64
   * @min 0
   */
  ef_search?: number | null;
  /**
   * HNSW: neighbours per node. `m_max0` is derived as `2 * m`.
   * @format int64
   * @min 0
   */
  m?: number | null;
  /**
   * IVF: centroid count. Omit to auto-scale to `max(16, sqrt(N))`.
   * @format int64
   * @min 0
   */
  n_list?: number | null;
  /**
   * IVF: probe count. Omit to auto-scale to `max(1, sqrt(n_list))`.
   * @format int64
   * @min 0
   */
  n_probe?: number | null;
}

/** The request body for `POST /v1/namespaces/{name}/index`. */
export interface IndexBuildRequest {
  /**
   * Optional parameter overrides. Only the parameters the implementation
   * actually reads are used; unknown keys are ignored.
   *
   * Stays a `serde_json::Value` at runtime — the handlers index into it by
   * key and tolerate anything — but is *described* as
   * [`IndexBuildParameters`] so a generated SDK can offer the real knobs.
   */
  parameters?: IndexBuildParameters;
  /** `"hnsw"`, `"ivf"`, `"bq"`, or `null` (drop the index). */
  type?: null | BuildableIndexKind;
}

export interface IndexConfigResponse {
  hnsw?: null | HnswConfigView;
  index_type: string;
}

/**
 * The `index` values a response can contain — canonical only, matching
 * [`valori_domain::IndexKind`]'s `as_str`.
 */
export enum IndexKind {
  Brute = "brute",
  Hnsw = "hnsw",
  Ivf = "ivf",
  Bq = "bq",
  Auto = "auto",
}

/**
 * The `index` values a request may send — canonical plus the aliases
 * [`valori_domain::IndexKind`]'s `FromStr` accepts.
 *
 * Omitting `index` entirely is a distinct, first-class state (`index = NONE`,
 * exact namespace-scoped search with no dedicated ANN structure) and is not
 * represented here — absence is the representation.
 */
export enum IndexKindInput {
  Brute = "brute",
  Bruteforce = "bruteforce",
  Hnsw = "hnsw",
  Ivf = "ivf",
  Bq = "bq",
  Auto = "auto",
  Mstg = "mstg",
}

/**
 * `POST /v1/index/rebuild` request body. Project-wide rebuild; per-collection
 * lifecycle lives at `/v1/namespaces/{name}/index`.
 */
export interface IndexRebuildRequest {
  /**
   * Echoed back as `effective`. Present for forward compatibility; the
   * standalone rebuild always rebuilds every per-collection index.
   */
  index?: null | IndexKindInput;
}

/** `POST /v1/index/rebuild` response. */
export interface IndexRebuildResponse {
  /** The `index` value from the request, or `"rebuilt"` when it was absent. */
  effective: string;
  ok: boolean;
  /**
   * Live record count after the rebuild.
   * @min 0
   */
  records: number;
}

/**
 * Response to `POST /v1/namespaces/{name}/index` and
 * `GET /v1/namespaces/{name}/index`.
 *
 * # Cluster vs standalone distinction
 *
 * In cluster mode, `desired_type` is always populated from the Raft-
 * replicated desired spec (what the cluster wants), while `active_type`
 * and `status` reflect this **node's local** build state. They may
 * differ temporarily as builds propagate across replicas.
 *
 * Example during a transition:
 * ```json
 * { "desired_type": "ivf", "active_type": "hnsw", "status": "building",
 *   "building_generation": 2, "active_generation": 1 }
 * ```
 *
 * In standalone mode, `desired_type` is always equal to `active_type` once
 * a build completes (there's only one node).
 */
export interface IndexStatusResponse {
  /**
   * The active generation number, if any.
   * @format int32
   * @min 0
   */
  active_generation?: number | null;
  /** The currently serving index type ("hnsw", "ivf", "bq", "none"). */
  active_type: string;
  /**
   * The base LSN of the building generation.
   * @format int64
   * @min 0
   */
  base_lsn?: number | null;
  /**
   * Unix seconds when the current build started.
   * @format int64
   * @min 0
   */
  build_started_at?: number | null;
  /**
   * If a build is in progress, its generation number.
   * @format int32
   * @min 0
   */
  building_generation?: number | null;
  collection: string;
  /**
   * The type the user requested (may differ from active while building).
   * In cluster mode, this comes from the Raft-replicated desired spec and
   * is authoritative for the whole cluster, not just the responding node.
   */
  desired_type?: string | null;
  /** Human-readable failure reason, if the last build failed. */
  error?: string | null;
  /** Current lifecycle status of the active or building generation. */
  status: string;
}

/**
 * The `202` body of `POST /v1/ingest` when `async: true`.
 *
 * The async branch has always returned this object; the contract used to
 * declare the `202` with no content at all, so a generated client saw
 * `never` and had no typed way to reach `job_id` — the one field the whole
 * async flow depends on, since it is what `GET /v1/ingest/status/{job_id}`
 * takes.
 */
export interface IngestAcceptedResponse {
  collection: string;
  /** Poll `GET /v1/ingest/status/{job_id}` with this id. */
  job_id: string;
  ok: boolean;
  /** Always `processing` on this response. */
  status: string;
}

/** One chunk produced by a chunking strategy. */
export interface IngestChunk {
  /**
   * 0-based position in the chunk sequence.
   * @min 0
   */
  index: number;
  /** Full chunk text ready to embed. */
  text: string;
  /** Section title (tree strategy) or empty string. */
  title: string;
}

export interface IngestDocumentRequest {
  /**
   * Fixed-strategy overlap in chars (default 200).
   * @min 0
   */
  chunk_overlap?: number | null;
  /**
   * Fixed-strategy chunk size in chars (default 1000).
   * @min 0
   */
  chunk_size?: number | null;
  /** Collection to ingest into (default = "default"). */
  collection?: string | null;
  /** Source label stored in metadata (e.g. filename). Optional. */
  source?: string | null;
  /** Chunking strategy: `auto` | `tree` | `conversation` | `sentence` | `fixed`. */
  strategy?: string | null;
  /** Raw text content of the document. */
  text: string;
}

export interface IngestDocumentResponse {
  /**
   * Total number of chunks produced.
   * @min 0
   */
  chunk_count: number;
  /**
   * The chunks. Caller embeds each `text`, inserts the vector, records
   * `record_id` → chunk for provenance.
   */
  chunks: IngestChunk[];
  /** Collection the document was targeted at. */
  collection: string;
  /** Strategy that was actually used (useful when `strategy="auto"`). */
  strategy_used: string;
}

/**
 * The lifecycle states an asynchronous ingest job actually reports.
 *
 * Phase API-3.3: these are the three literals both routers write — see the
 * `jobs.insert(..)` calls in [`ingest`] (standalone) and `cluster_ingest`
 * (cluster). There is no separate `pending`: a job is `processing` from the
 * moment `POST /v1/ingest?async=true` answers `202`.
 */
export enum IngestJobState {
  Processing = "processing",
  Completed = "completed",
  Failed = "failed",
}

/**
 * The body of `GET /v1/ingest/status/{job_id}`.
 *
 * # Why this type exists
 *
 * Phase API-3.3: this response was annotated `body = Object`, rendering as a
 * bare `type: object` with no properties — `object` in TypeScript,
 * `Dict[str, Any]` in Python. An SDK user polling an async ingest had no
 * typed way to learn whether the job finished, and no discoverable name for
 * the field carrying the answer. That defeats the purpose of the `202`
 * contract that points here.
 *
 * Every field is optional except `status` and `job_id`, because which ones
 * are present genuinely depends on the stage the job has reached — the
 * terminal-success fields do not exist while it is `processing`, and `error`
 * exists only on `failed`. `status` is the discriminant to branch on.
 */
export interface IngestJobStatusResponse {
  /**
   * Chunks the document was split into.
   * @min 0
   */
  chunk_count?: number | null;
  /** Target collection. Absent on `failed` jobs that failed before resolving one. */
  collection?: string | null;
  /**
   * `completed` only — the graph node representing the ingested document.
   * @format int32
   * @min 0
   */
  document_node_id?: number | null;
  /** `failed` only — the human-readable reason. */
  error?: string | null;
  /** Echo of the polled job id. */
  job_id: string;
  /** `completed` only — correlates with `GET /v1/operations/{id}`. */
  operation_id?: string | null;
  /** `completed` only — the records written, one per chunk. */
  record_ids?: number[] | null;
  /** Which stage the job has reached. Branch on this. */
  status: IngestJobState;
  /** Chunking strategy the server selected. */
  strategy_used?: string | null;
}

export interface IngestRequest {
  async?: boolean | null;
  /** @min 0 */
  chunk_overlap?: number | null;
  /** @min 0 */
  chunk_size?: number | null;
  collection?: string | null;
  source?: string | null;
  strategy?: string | null;
  text: string;
}

export interface IngestResponse {
  /** @min 0 */
  chunk_count: number;
  collection: string;
  /**
   * @format int32
   * @min 0
   */
  document_node_id: number;
  ok: boolean;
  /**
   * Fetch `GET /v1/operations/:id/execution` with this id for the full
   * per-stage execution breakdown (Execution Explorer).
   */
  operation_id: string;
  record_ids: number[];
  strategy_used: string;
}

export interface IngestUpdateRequest {
  /** @min 0 */
  chunk_overlap?: number | null;
  /** @min 0 */
  chunk_size?: number | null;
  collection?: string | null;
  /**
   * @format int32
   * @min 0
   */
  document_node_id: number;
  source?: string | null;
  strategy?: string | null;
  text: string;
}

export interface IngestUpdateResponse {
  /** @min 0 */
  added_count: number;
  collection: string;
  /**
   * @format int32
   * @min 0
   */
  document_node_id: number;
  /** @min 0 */
  kept_count: number;
  /** @min 0 */
  new_chunk_count: number;
  ok: boolean;
  record_ids: number[];
  /** @min 0 */
  removed_count: number;
  strategy_used: string;
}

export interface InsertEncryptedRequest {
  collection?: string | null;
  /** Optional pre-chosen key_id (hex). If absent, a fresh key_id is generated. */
  key_id?: string | null;
  /** Base64-encoded plaintext payload (will be encrypted by the vault). */
  payload: string;
  /**
   * @format int64
   * @min 0
   */
  tag?: number | null;
}

export interface InsertEncryptedResponse {
  /**
   * @format int32
   * @min 0
   */
  id: number;
  key_id: string;
}

export interface InsertReceiptJson {
  new_root: string;
  old_root: string;
  proof: string;
  /**
   * @format int32
   * @min 0
   */
  record_id: number;
  /**
   * @format int64
   * @min 0
   */
  sequence: number;
  state_hash: string;
  /**
   * @format int64
   * @min 0
   */
  timestamp: number;
}

/**
 * **The** public request body for `POST /v1/records` — one model, both routers.
 *
 * Phase API-2 merged the two divergent bodies that existed before:
 * standalone accepted `{values, collection, text}` and silently discarded
 * everything else; cluster accepted `{values, collection, metadata, tag,
 * request_id}` and silently discarded `text`. Every field below is now
 * honoured on **both** paths.
 */
export interface InsertRecordRequest {
  collection?: string | null;
  /**
   * Opaque per-record metadata bytes, committed inside the `InsertRecord`
   * event and therefore covered by the BLAKE3 audit chain.
   */
  metadata?: number[] | null;
  /**
   * Client idempotency token. Replaying the same token inside the dedup
   * window returns the record the first request created and performs no
   * second write.
   */
  request_id?: null | RequestId;
  /**
   * Opaque user tag stored alongside the record.
   * @format int64
   * @min 0
   */
  tag?: number;
  /**
   * Optional raw text for BM25 hybrid reranking. When provided, stored
   * in the reranker index alongside the vector so future searches can
   * use term-frequency scoring to reorder results.
   */
  text?: string | null;
  values: number[];
}

/**
 * **The** public response body for `POST /v1/records` — one model, both routers.
 *
 * `log_index` is Raft-only and omitted in standalone; `deduplicated` is
 * present on both paths and is `true` exactly when the request carried a
 * `request_id` that had already been applied, in which case `id` is the
 * record the original request created and no new write happened.
 */
export interface InsertRecordResponse {
  /**
   * `true` when this request was recognised as a replay of a previous
   * `request_id` and no new record was created.
   */
  deduplicated: boolean;
  /**
   * @format int32
   * @min 0
   */
  id: number;
  /**
   * Raft log index of the committed write — cluster path only.
   * @format int64
   * @min 0
   */
  log_index?: number | null;
  receipt: InsertReceiptJson;
}

export interface InsertedEntity {
  description: string;
  name: string;
  /**
   * @format int32
   * @min 0
   */
  node_id: number;
  /**
   * @format int32
   * @min 0
   */
  record_id?: number | null;
  type: string;
}

export interface InsertedRelationship {
  description: string;
  /**
   * @format int32
   * @min 0
   */
  edge_id: number;
  source_name: string;
  target_name: string;
}

export interface ListCollectionsResponse {
  collections: CollectionInfo[];
}

export interface ListNodesResponse {
  /** @min 0 */
  count: number;
  nodes: NodeInfo[];
}

export interface ListRemoteSnapshotsResponse {
  /** @min 0 */
  count: number;
  snapshots: SnapshotEntry[];
}

export interface ListRemoteWalResponse {
  /** @min 0 */
  count: number;
  segments: WalEntry[];
}

export interface ManifestResponse {
  manifest?: null | SnapshotManifest;
}

export interface MemberView {
  api_addr: string;
  id: U64;
  raft_addr: string;
  voter: boolean;
}

/**
 * Replace an existing memory record with a new vector, committing a
 * SoftDeleteRecord + AutoInsertRecord + AutoCreateEdge(Supersedes) to the
 * BLAKE3 audit chain in one logical operation.
 */
export interface MemoryConsolidateRequest {
  collection?: string | null;
  metadata?: object | null;
  /** New vector that replaces the old memory. */
  new_vector: number[];
  /**
   * Record id of the memory being replaced.
   * @format int32
   * @min 0
   */
  old_record_id: number;
}

export interface MemoryConsolidateResponse {
  /**
   * @format int64
   * @min 0
   */
  log_index?: number | null;
  /**
   * The new record id.
   * @format int32
   * @min 0
   */
  new_record_id: number;
  /**
   * The old record id (now soft-deleted).
   * @format int32
   * @min 0
   */
  old_record_id: number;
  /** BLAKE3 state hash after all three events are applied. */
  state_hash: string;
  /**
   * The Supersedes edge id linking new → old.
   * @format int32
   * @min 0
   */
  supersedes_edge_id: number;
}

/**
 * Check whether two records contradict each other (by cosine similarity
 * threshold) and, if so, commit a Contradicts edge to the audit chain.
 */
export interface MemoryContradictRequest {
  collection?: string | null;
  /**
   * @format int32
   * @min 0
   */
  record_a: number;
  /**
   * @format int32
   * @min 0
   */
  record_b: number;
  /**
   * Cosine similarity threshold above which the records are deemed to
   * contradict. Default 0.85 — tuned for claim-level NLI in Q16.16 space.
   * @format float
   */
  threshold?: number | null;
}

export interface MemoryContradictResponse {
  contradicts: boolean;
  /**
   * Edge id of the Contradicts edge, present only when contradicts=true.
   * @format int32
   * @min 0
   */
  edge_id?: number | null;
  /**
   * @format int64
   * @min 0
   */
  log_index?: number | null;
  /**
   * @format int32
   * @min 0
   */
  record_a: number;
  /**
   * @format int32
   * @min 0
   */
  record_b: number;
  /** @format float */
  similarity: number;
  state_hash: string;
}

export interface MemorySearchHit {
  /**
   * Phase C4.1 — record age in seconds; present only when decay is active.
   * @format int64
   * @min 0
   */
  age_secs?: number | null;
  /**
   * Phase C4.1 — applied decay factor in (0, 1]; present only when decay is active.
   * @format float
   */
  decay_factor?: number | null;
  memory_id: string;
  metadata?: object | null;
  /**
   * @format int32
   * @min 0
   */
  record_id: number;
  /** @format float */
  score: number;
}

export interface MemorySearchResponse {
  results: MemorySearchHit[];
}

export interface MemorySearchVectorRequest {
  collection?: string | null;
  /**
   * Phase S6 (cluster mode only; ignored standalone): `"local"` skips
   * the read-index round trip (eventually consistent, faster). Absent
   * or any other value defaults to linearizable, matching `/v1/search`.
   */
  consistency?: string | null;
  /**
   * Phase C4.1 — recency half-life (seconds). When set (> 0), the agent-memory
   * recall path re-ranks older memories down. See `SearchRequest`.
   * @format int64
   * @min 0
   */
  decay_half_life_secs?: number | null;
  /** @min 0 */
  k: number;
  /**
   * Phase I7 — restrict results to records whose stored metadata satisfies
   * every key/value predicate. Same semantics as `SearchRequest::metadata_filter`.
   */
  metadata_filter?: object | null;
  /**
   * Phase C5 — raw query text for BM25 hybrid re-ranking. Required when
   * `rerank=true`; ignored otherwise.
   */
  query_text?: string | null;
  query_vector: number[];
  /**
   * Phase C5 — when `true` (default) and `query_text` is provided, re-ranks
   * candidates by hybrid BM25 + vector score before returning the top-k.
   */
  rerank?: boolean;
}

export interface MemoryUpsertResponse {
  /**
   * @format int32
   * @min 0
   */
  chunk_node_id: number;
  /**
   * @format int32
   * @min 0
   */
  document_node_id: number;
  /**
   * @format int64
   * @min 0
   */
  log_index?: number | null;
  memory_id: string;
  /**
   * @format int32
   * @min 0
   */
  record_id: number;
}

export interface MemoryUpsertVectorRequest {
  /**
   * @format int32
   * @min 0
   */
  attach_to_document_node?: number | null;
  collection?: string | null;
  metadata?: object | null;
  tags?: string[] | null;
  vector: number[];
}

export interface MetadataGetResponse {
  metadata?: object | null;
  target_id: string;
}

export interface MetadataSetRequest {
  /**
   * Arbitrary caller-supplied JSON object. Valori stores it verbatim and
   * never interprets it, so this is genuinely open-ended — but
   * `additionalProperties` makes that explicit, so a generator emits
   * `Record<string, unknown>` / `Dict[str, Any]` rather than a bare
   * property-less `object` that says nothing at all.
   */
  metadata: Partial<Record<string, any>>;
  target_id: string;
}

export interface MetadataSetResponse {
  success: boolean;
}

/**
 * The `metric` values a response can contain — canonical only, matching
 * [`valori_domain::Metric`]'s `as_str`.
 */
export enum Metric {
  SquaredL2 = "squared_l2",
}

/**
 * The `metric` values a request may send — canonical plus the aliases
 * [`valori_domain::Metric`]'s `FromStr` accepts.
 */
export enum MetricInput {
  SquaredL2 = "squared_l2",
  L2 = "l2",
  L2Sq = "l2sq",
}

/** A single result from a multi-collection search, annotated with its source. */
export interface MultiSearchHit {
  /**
   * Age of the record in seconds. Present only when decay is active.
   * @format int64
   * @min 0
   */
  age_secs?: number | null;
  /** The collection this record lives in. */
  collection: string;
  /**
   * Phase C4.1 — applied decay factor in (0, 1]. Present only when decay is active.
   * @format float
   */
  decay_factor?: number | null;
  /**
   * Record ID within the collection.
   * @format int32
   * @min 0
   */
  id: number;
  /**
   * Squared L2 distance to the query (smaller = closer).
   * @format float
   */
  score: number;
}

/** Request body for `POST /v1/search/multi`. */
export interface MultiSearchRequest {
  /** One or more collection names. All must share the same `dim` and `metric`. */
  collections: string[];
  /**
   * Phase C4.1 — decay half-life in seconds. Applied per-collection before merge.
   * @format int64
   * @min 0
   */
  decay_half_life_secs?: number | null;
  /**
   * Number of global top-k results to return.
   * @min 0
   */
  k: number;
  /** Metadata predicate applied per-collection after vector search. */
  metadata_filter?: object | null;
  /** Query vector. Must match the shared dimension of all listed collections. */
  query: number[];
}

/** Response for `POST /v1/search/multi`. */
export interface MultiSearchResponse {
  /** Names of all collections included in this query. */
  collections_searched: string[];
  /**
   * Runtime failures from individual collections, if any.
   * Present only when at least one collection's search failed after
   * dimension/metric compatibility was confirmed.
   */
  partial_failures?: PartialSearchFailure[] | null;
  /** Global top-k hits sorted by score ascending (smaller = closer). */
  results: MultiSearchHit[];
}

export interface NodeInfo {
  /**
   * @format int32
   * @min 0
   */
  kind: number;
  /**
   * @format int32
   * @min 0
   */
  namespace_id: number;
  /**
   * @format int32
   * @min 0
   */
  node_id: number;
  /**
   * @format int32
   * @min 0
   */
  record_id?: number | null;
}

export interface OperationDetailResponse {
  collection: string;
  /** Canonical v1 operation identity. Always a string (§13). */
  id: string;
  /** Resource cost of the operation. */
  metrics: OperationMetrics;
  /** Identity and addressing for the operation. */
  overview: OperationOverview;
  /**
   * The proof of the state transition.
   *
   * When a receipt was assembled for this operation this is a full
   * [`crate::openapi::ReceiptDto`]. When one was not — a receipt store is
   * in-process and does not survive a restart — the node synthesises a
   * reduced stand-in carrying `receipt_id`, `status`, `operation_hash`,
   * `state_hash_before` and `state_hash_after`. Because the two shapes
   * genuinely differ, this is documented as an open object rather than
   * claiming a single schema that only sometimes holds.
   */
  proof: Partial<Record<string, any>>;
  /** What the operation changed. */
  results: OperationResults;
  status: string;
  /**
   * @format int64
   * @min 0
   */
  timestamp_unix: number;
  timing: string;
  type: string;
}

/**
 * The `details` block of [`OperationSummary`].
 *
 * `shard_id` is populated on the cluster path only — standalone has no shard
 * dimension, so it is absent there rather than defaulted to a fictitious `0`.
 */
export interface OperationDetails {
  /**
   * Set when the event touched a graph edge.
   * @format int32
   * @min 0
   */
  edge_id?: number | null;
  /**
   * Position in the committed event log.
   * @format int64
   * @min 0
   */
  log_index?: number | null;
  /**
   * Set when the event touched a graph node.
   * @format int32
   * @min 0
   */
  node_id?: number | null;
  /**
   * Set when the event touched a record.
   * @format int32
   * @min 0
   */
  record_id?: number | null;
  /**
   * Cluster mode only — the shard whose log this event came from.
   * @format int32
   * @min 0
   */
  shard_id?: number | null;
}

/** The `metrics` block of [`OperationDetailResponse`]. */
export interface OperationMetrics {
  /**
   * @format int64
   * @min 0
   */
  cpu_cycles: number;
  /** @format double */
  duration_ms: number;
  /**
   * @format int64
   * @min 0
   */
  memory_bytes: number;
  /** Qualitative assessment, e.g. `optimal`. */
  status: string;
}

/** The `overview` block of [`OperationDetailResponse`]. */
export interface OperationOverview {
  collection: string;
  /**
   * Set when the operation touched a graph edge.
   * @format int32
   * @min 0
   */
  edge_id?: number | null;
  id: string;
  /**
   * Position in the committed event log.
   * @format int64
   * @min 0
   */
  log_index?: number | null;
  /**
   * Set when the operation touched a graph node.
   * @format int32
   * @min 0
   */
  node_id?: number | null;
  /**
   * Set when the operation touched a record.
   * @format int32
   * @min 0
   */
  record_id?: number | null;
  status: string;
  timing: string;
  type: string;
}

/** The `results` block of [`OperationDetailResponse`]. */
export interface OperationResults {
  /**
   * @format int32
   * @min 0
   */
  edges_affected: number;
  /** Human-readable summary. Do not parse. */
  message: string;
  /**
   * @format int32
   * @min 0
   */
  nodes_affected: number;
  /**
   * @format int32
   * @min 0
   */
  records_affected: number;
  /** Commit outcome, e.g. `committed`. */
  status: string;
}

export interface OperationSummary {
  collection: string;
  /** Addressing for the underlying committed event. */
  details: OperationDetails;
  /** Canonical v1 operation identity. Always a string (§13). */
  id: string;
  status: string;
  /**
   * @format int64
   * @min 0
   */
  timestamp_unix: number;
  timing: string;
  type: string;
}

export interface OperationsListResponse {
  operations: OperationSummary[];
  /** @min 0 */
  total: number;
}

/** Per-package health entry. */
export interface PackageHealth {
  id: string;
  /** @min 0 */
  ref_count: number;
  /**
   * @format int64
   * @min 0
   */
  size_bytes: number;
  /** Health status for one installed package. */
  status: PackageHealthStatus;
}

/** Health status for one installed package. */
export enum PackageHealthStatus {
  Verified = "verified",
  Installed = "installed",
  Missing = "missing",
  Corrupted = "corrupted",
}

/** A per-collection runtime failure reported in `MultiSearchResponse`. */
export interface PartialSearchFailure {
  collection: string;
  error: string;
}

/** Slab occupancy for one kernel pool (records, graph nodes, graph edges). */
export interface PoolStatsSchema {
  /**
   * Configured slab capacity.
   * @min 0
   */
  capacity: number;
  /**
   * `live / capacity` as a percentage, rounded to one decimal.
   * @format double
   */
  fill_pct: number;
  /**
   * Live (non-tombstoned) entries.
   * @min 0
   */
  live: number;
  /**
   * Slab slots consumed, including tombstones.
   * @min 0
   */
  slots_used: number;
}

/**
 * The unified proof of one completed Operation, as it crosses the wire.
 *
 * Mirrors [`valori_effect::Receipt`], which lives in a crate with no `utoipa`
 * dependency — the same translation-layer arrangement as [`ApiError`].
 *
 * # Why this type exists
 *
 * Phase API-3.3: `GET /v1/proof/receipt` and `GET /v1/proof/receipt/{id}`
 * were annotated `body = Object`, which renders as a bare `type: object` with
 * no properties. Generators produce `object` in TypeScript and
 * `Dict[str, Any]` in Python — so the receipt, which is the entire point of
 * a verifiable memory system, arrived in every SDK as an opaque blob with no
 * discoverable field.
 *
 * The handlers return `serde_json::to_value(&Receipt)`, and `Receipt` is a
 * fully concrete struct. Nothing about it was ever unknowable; it simply was
 * not written down.
 *
 * `tests/api_contract.rs::receipt_dto_matches_the_runtime_receipt` serialises
 * a real `Receipt` and diffs its key set against this type, so the two cannot
 * drift.
 */
export interface Receipt {
  /** Whether the producing node was running in cluster mode. */
  cluster_mode: boolean;
  /**
   * Committed log height at production time.
   * @format int64
   * @min 0
   */
  committed_height: number;
  /** Whether embedding was enabled on the node that produced this. */
  embed_enabled: boolean;
  /** Per-task fragments in topological order. */
  fragments: ReceiptFragment[];
  /** `BLAKE3(op_hash ‖ fp.hash ‖ ctx_hash ‖ topo_order)` for the task graph. */
  graph_hash: string;
  /**
   * Kernel ABI the operation ran against.
   * @format int32
   * @min 0
   */
  kernel_abi_version: number;
  /** `BLAKE3(kind ‖ inputs ‖ policy)` for the operation. */
  operation_hash: string;
  /** Parent receipt hashes in the Merkle DAG. Empty for a root receipt. */
  parent_receipts: number[][];
  /** `BLAKE3(version ‖ routing_config_hash ‖ feature_flags_hash ‖ schema_version)`. */
  planner_fingerprint_hash: string;
  /**
   * Unix seconds. Deliberately excluded from `receipt_hash`.
   * @format int64
   * @min 0
   */
  produced_at: number;
  /**
   * Content-addressed BLAKE3 of the receipt, as 32 raw bytes.
   *
   * `ReceiptHash` is a `[u8; 32]` newtype, so it crosses the wire as an
   * array of 32 integers — not the hex string `to_hex()` produces.
   */
  receipt_hash: number[];
  /** Unique id for this receipt. */
  receipt_id: string;
  /**
   * Shard count on the producing node.
   * @format int32
   * @min 0
   */
  shard_count: number;
  /**
   * Shard that produced this receipt.
   * @format int32
   * @min 0
   */
  shard_id: number;
  /** BLAKE3 hex of kernel state after. Equal to `before` for read-only operations. */
  state_hash_after: string;
  /** BLAKE3 hex of kernel state before the operation. */
  state_hash_before: string;
}

/**
 * One task's contribution to a [`ReceiptDto`], mirroring
 * [`valori_effect::ReceiptFragment`].
 */
export interface ReceiptFragment {
  /** BLAKE3 hex of the fragment itself, used for chaining. */
  fragment_hash: string;
  /** True if this task produced kernel writes. */
  mutated: boolean;
  /** BLAKE3 hex of the kernel state after this task. Equal to `before` for reads. */
  state_hash_after: string;
  /** BLAKE3 hex of the kernel state before this task. */
  state_hash_before: string;
  /**
   * Position of this task in the executed graph's topological order.
   * @format int32
   * @min 0
   */
  task_index: number;
}

/** `GET /v1/records/{id}` — one stored record, decoded back to `f32`. */
export interface RecordResponse {
  /**
   * @format int32
   * @min 0
   */
  id: number;
  /** Whatever JSON was committed alongside the record, if any. */
  metadata?: object | null;
  /**
   * @format int64
   * @min 0
   */
  tag: number;
  /**
   * The stored Q16.16 vector, converted back to `f32` for the wire.
   * Round-tripping is lossy at the Q16.16 quantum, by design.
   */
  vector: number[];
}

/** 16-byte client idempotency token. A replay returns the record the first request created, with `deduplicated: true`. */
export type RequestId = string | number[];

export interface RestoreFromStoreRequest {
  /**
   * Object key returned by a previous upload or list call. Omit to
   * restore whatever `manifest.json` currently names as current — see
   * `GET /v1/storage/manifest`; this is now the recommended entry point
   * for disaster recovery instead of listing `snapshots/` and picking
   * the newest filename by hand.
   */
  key?: string | null;
}

export interface RestoreFromStoreResponse {
  key: string;
  /** @min 0 */
  size_bytes: number;
  state_hash: string;
}

export interface SearchHit {
  /**
   * Age of the record in seconds at query time. Present only when decay is
   * active and the record's creation time is known.
   * @format int64
   * @min 0
   */
  age_secs?: number | null;
  /**
   * Phase C4.1 — applied decay factor in (0, 1]. Present only when decay is
   * active. `score` stays the true (undecayed) L2 distance for honesty;
   * ranking reflects `score / decay_factor`.
   * @format float
   */
  decay_factor?: number | null;
  /**
   * G1.4.1 — hop distance to the nearest `graph_rerank` seed. Present
   * only when `graph_rerank` was requested. `None` within that means
   * the candidate has no graph node, or is unreachable within
   * `max_depth` — never causes a candidate to be dropped.
   * @format int32
   * @min 0
   */
  graph_distance?: number | null;
  /**
   * @format int32
   * @min 0
   */
  id: number;
  /** @format float */
  score: number;
}

export interface SearchRequest {
  /**
   * ISO 8601 UTC timestamp — search the vector state as it existed at this moment.
   * Requires the event log to be enabled (`VALORI_EVENT_LOG_PATH`).
   */
  as_of?: string | null;
  /**
   * Log index — search the vector state after exactly this many committed events.
   * Mutually exclusive with `as_of`; `as_of_log_index` takes precedence if both given.
   * @format int64
   * @min 0
   */
  as_of_log_index?: number | null;
  collection?: string | null;
  /**
   * Phase C4.1 — recency half-life in seconds. When set (> 0), results are
   * re-ranked so older records decay: a record one half-life old has its L2
   * distance doubled. `0`/absent uses the server default (or pure distance).
   * Ignored for `as_of` / point-in-time queries.
   * @format int64
   * @min 0
   */
  decay_half_life_secs?: number | null;
  /**
   * G1.4.1 — optional graph-aware reranking. Presence enables it;
   * absence is a complete no-op (identical to pre-G1.4.1 behavior).
   * Composes with either `rerank` (BM25) or `decay_half_life_secs` — it
   * runs as a final pass over whatever score the rest of the pipeline
   * already produced, using each hit's existing `score` as the base.
   */
  graph_rerank?: null | GraphRerankRequest;
  /** @min 0 */
  k: number;
  /**
   * Optional JSON object whose key-value pairs must ALL be present (and equal)
   * in a record's metadata for the record to be returned.
   * Numeric values support optional range operators: `{"gte": 2020, "lte": 2024}`.
   * Example: `{"author": "Alice", "year": {"gte": 2020}}`
   */
  metadata_filter?: object | null;
  query: number[];
  /**
   * The raw query string used for BM25 scoring. Required when `rerank=true`.
   * Ignored when `rerank=false`.
   */
  query_text?: string | null;
  /**
   * BM25 hybrid reranking. When `true` (default), the server fetches
   * `k × POOL_FACTOR` candidates by vector similarity and re-ranks them by
   * a 50/50 blend of normalised vector score + BM25 term-frequency score
   * before returning the top-k. Requires `query_text` to be set.
   * Set to `false` to get pure vector ranking (legacy behaviour).
   */
  rerank?: boolean;
}

export interface SearchResponse {
  /**
   * Present only for as-of searches: the log index of the replayed state.
   * @format int64
   * @min 0
   */
  as_of_log_index?: number | null;
  /** BLAKE3 hex hash of the kernel state at `as_of_log_index`. */
  as_of_state_hash?: string | null;
  /** ISO 8601 string of `as_of_timestamp_unix`. */
  as_of_timestamp_iso?: string | null;
  /**
   * Unix-second wall-clock timestamp of the `as_of_log_index` event.
   * @format int64
   * @min 0
   */
  as_of_timestamp_unix?: number | null;
  results: SearchHit[];
}

/** One shard's collection assignment in `GET /v1/shard/routing`. */
export interface ShardRoutingEntry {
  collections: string[];
  /** @min 0 */
  shard: number;
}

/**
 * `GET /v1/shard/routing` — which collection lives on which logical shard.
 * Routing is `namespace_id % shard_count`.
 */
export interface ShardRoutingResponse {
  /** `standalone` or `cluster`. */
  mode: string;
  /** @min 0 */
  shard_count: number;
  shards: ShardRoutingEntry[];
}

/**
 * Raw snapshot bytes, as the OpenAPI binary idiom.
 *
 * Phase API-3.3: `/v1/snapshot/download` and `/v1/snapshot/upload` were
 * annotated `body = Vec<u8>`, which utoipa renders literally — `type: array,
 * items: {type: integer, format: int32}`. Generators believe it: the
 * throwaway Python client typed the download as `list[int]`, so restoring a
 * snapshot meant round-tripping every byte of a multi-megabyte file through
 * a Python integer list.
 *
 * `type: string, format: binary` is the OpenAPI idiom for an opaque byte
 * stream, and generators map it to `bytes` / `Blob` / `File`. The wire format
 * is unchanged — this describes the same octet-stream correctly.
 * @format binary
 */
export type SnapshotBytes = File;

export interface SnapshotEntry {
  /**
   * Unix epoch seconds extracted from the key name — used for sorting.
   * @format int64
   * @min 0
   */
  epoch_secs: number;
  /** Full object key (e.g. `"prefix/snapshots/00000001750000000_abc12345.snap"`). */
  key: string;
  /**
   * Snapshot size in bytes.
   * @format int64
   * @min 0
   */
  size_bytes: number;
  /** Hex BLAKE3 state hash recorded alongside the snapshot. */
  state_hash: string;
}

/**
 * `manifest.json` — the entry point for disaster recovery. Written
 * alongside every snapshot upload (see [`ObjectStoreBackend::
 * upload_snapshot_and_update_manifest`]), it names the ONE snapshot that
 * is current (out of however many timestamped `.snap` objects exist under
 * `snapshots/` — old ones aren't deleted until `prune_snapshots` runs) plus
 * the WAL segments archived since, so a restore tool has a single object
 * to fetch instead of listing-and-sorting `snapshots/`/`wal/` and hoping
 * the newest filename really is the right one.
 */
export interface SnapshotManifest {
  /**
   * `None` only if a manifest was written before any snapshot ever
   * succeeded — shouldn't happen in practice since
   * `upload_snapshot_and_update_manifest` always has a just-uploaded
   * snapshot to point at, but kept optional rather than a fabricated
   * placeholder entry.
   */
  current_snapshot?: null | SnapshotEntry;
  /**
   * `CARGO_PKG_VERSION` of whatever wrote this manifest (valori-node,
   * normally) — lets a restore tool detect "this snapshot was written by
   * an older/newer node than the one about to restore it."
   */
  node_version: string;
  /**
   * @format int32
   * @min 0
   */
  schema_version: number;
  /**
   * Unix epoch seconds when this manifest was last written.
   * @format int64
   * @min 0
   */
  updated_at: number;
  wal_segments: WalEntry[];
}

export interface SnapshotRestoreRequest {
  path: string;
}

export interface SnapshotRestoreResponse {
  success: boolean;
}

export interface SnapshotSaveRequest {
  path?: string | null;
}

export interface SnapshotSaveResponse {
  path: string;
  success: boolean;
}

/** Format-specific counters emitted by each stage. E4.1. */
export type StageMetrics =
  | {
      /** @min 0 */
      bytes_read: number;
      mime: string;
      stage: "reader";
    }
  | {
      /**
       * Number of checks that were evaluated.
       * @min 0
       */
      checks_run: number;
      stage: "validator";
      warnings: string[];
    }
  | {
      /** @min 0 */
      avg_chunk_bytes: number;
      /** @min 0 */
      chunks_created: number;
      /** @min 0 */
      max_chunk_bytes: number;
      stage: "chunker";
    }
  | {
      /**
       * Number of embed-batch calls made (1 per batch).
       * @min 0
       */
      batch_count: number;
      /** @min 0 */
      dimensions: number;
      /**
       * Wall-clock latency of all embed calls combined, milliseconds.
       * @format int64
       * @min 0
       */
      latency_ms: number;
      /** Model name (e.g. `"nomic-embed-text"`). */
      model: string;
      /**
       * Provider kind (e.g. `"ollama"`, `"openai"`), parsed from the
       * first embedding's `model_id` (`"{provider}/{model}"`).
       */
      provider: string;
      stage: "embedder";
    }
  | {
      /**
       * Parent→chunk edges created. Today always equal to
       * `graph_nodes_created` (`KernelWriter` creates exactly one edge per
       * chunk node), tracked separately since that's an implementation
       * detail of one `Writer`, not a pipeline invariant.
       * @min 0
       */
      graph_edges_created: number;
      /**
       * Chunk graph nodes created (one per written chunk that got a node —
       * `KernelWriter` always does; other writers may not).
       * @min 0
       */
      graph_nodes_created: number;
      /** @min 0 */
      records_written: number;
      stage: "writer";
    };

export enum StageName {
  Reader = "reader",
  Validator = "validator",
  Chunker = "chunker",
  Embedder = "embedder",
  Writer = "writer",
}

/**
 * One stage, with its human-facing label alongside the full metrics —
 * enough to render either a DAG step or a timeline row from the same data.
 */
export interface StageView {
  /**
   * @format int64
   * @min 0
   */
  duration_ms: number;
  error?: string | null;
  /**
   * User-facing description ("Read document", "Generate embeddings", …) —
   * never an internal crate/struct name.
   */
  label: string;
  /** Format-specific counters emitted by each stage. E4.1. */
  metrics: StageMetrics;
  stage: StageName;
  /**
   * @format int64
   * @min 0
   */
  started_at_ms: number;
  success: boolean;
  warnings: string[];
}

/**
 * `GET /v1/proof/state` — the running BLAKE3 Merkle root over all applied
 * events. Identical wire shape in standalone and cluster mode.
 */
export interface StateProofResponse {
  /** 64 lowercase hex characters (32 bytes). */
  final_state_hash: string;
}

export interface StatusView {
  current_leader?: null | U64;
  is_leader: boolean;
  /**
   * @format int64
   * @min 0
   */
  last_applied_index?: number | null;
  /**
   * @format int64
   * @min 0
   */
  last_log_index?: number | null;
  members: MemberView[];
  node_id: U64;
  /**
   * @format int64
   * @min 0
   */
  term: number;
}

export interface StorageSnapshotUploadResponse {
  key: string;
  /** @min 0 */
  pruned: number;
  /** @min 0 */
  size_bytes: number;
  state_hash: string;
}

/** A compact table-of-contents entry (title + summary, no body). */
export interface StructureNode {
  node_id: string;
  /**
   * Child sections. `no_recursion` stops utoipa's schema builder from
   * descending into this type forever — the generated document emits a
   * `$ref` back to `StructureNode` instead of an infinitely nested inline
   * schema.
   */
  nodes?: StructureNode[];
  summary: string;
  title: string;
}

/**
 * One edge in an expanded subgraph, as emitted by
 * `valori_rag::graph::expand_subgraph`.
 */
export interface SubgraphEdge {
  /**
   * Source node id.
   * @format int32
   * @min 0
   */
  from: number;
  /**
   * Graph edge id.
   * @format int32
   * @min 0
   */
  id: number;
  /**
   * `EdgeKind` discriminant.
   * @format int32
   * @min 0
   */
  kind: number;
  /**
   * Target node id.
   * @format int32
   * @min 0
   */
  to: number;
}

/**
 * One node in an expanded subgraph.
 *
 * Phase API-3.3: `SubgraphResponse.nodes` was `Vec<Object>` — an array of
 * property-less objects, which is `object[]` in TypeScript. The producer,
 * `valori_rag::graph::expand_subgraph`, emits a fixed three-key object; this
 * records it. Note the keys are `id`/`record`, not the `node_id`/`record_id`
 * that [`NodeInfo`] uses — the two shapes are genuinely different and must
 * not be conflated.
 */
export interface SubgraphNode {
  /**
   * Graph node id.
   * @format int32
   * @min 0
   */
  id: number;
  /**
   * `NodeKind` discriminant.
   * @format int32
   * @min 0
   */
  kind: number;
  /**
   * The record this node represents, when it represents one.
   * @format int32
   * @min 0
   */
  record?: number | null;
}

/** `GET /v1/graph/subgraph` — a BFS expansion around one root node. */
export interface SubgraphResponse {
  edges: SubgraphEdge[];
  nodes: SubgraphNode[];
}

/**
 * Aggregate health report for the entire model package store.
 *
 * Returned by `GET /v1/models/health`.
 */
export interface SystemHealth {
  /** @min 0 */
  corrupted: number;
  /**
   * @format int64
   * @min 0
   */
  disk_used_bytes: number;
  /** @min 0 */
  missing: number;
  packages: PackageHealth[];
  /**
   * Bytes used by packages with zero active references.
   * @format int64
   * @min 0
   */
  reclaimable_bytes: number;
  /** @min 0 */
  total_installed: number;
  /** @min 0 */
  verified: number;
}

/** A single entry in the timeline — one committed kernel event with its metadata. */
export interface TimelineEntry {
  /**
   * Edge ID if this is a graph-edge event.
   * @format int32
   * @min 0
   */
  edge_id?: number | null;
  /** Human-readable event kind. */
  event_type: string;
  /**
   * Sequential index within this entry's shard log (0-based).
   * Used as a tie-breaker when two shards share the same `timestamp_unix`.
   * @format int64
   * @min 0
   */
  log_index: number;
  /**
   * Node ID if this is a graph-node event.
   * @format int32
   * @min 0
   */
  node_id?: number | null;
  /**
   * Record ID if this is a record-level event.
   * @format int32
   * @min 0
   */
  record_id?: number | null;
  /**
   * Shard that committed this event. Always 0 in standalone mode.
   * @format int32
   * @min 0
   */
  shard_id: number;
  /** ISO 8601 UTC string for `timestamp_unix`. */
  timestamp_iso: string;
  /**
   * Unix-second wall-clock timestamp when this event was committed.
   * @format int64
   * @min 0
   */
  timestamp_unix: number;
}

export interface TimelineResponse {
  events: TimelineEntry[];
  /**
   * Inclusive lower bound filter applied (unix seconds), if any.
   * @format int64
   * @min 0
   */
  from_unix?: number | null;
  /**
   * Inclusive upper bound filter applied (unix seconds), if any.
   * @format int64
   * @min 0
   */
  to_unix?: number | null;
  /** @min 0 */
  total: number;
}

export interface TreeAnswerResult {
  answer: string;
  citations: TreeCitation[];
  evidence_text: string;
  fetched_ranges: number[][];
  query: string;
  reasoning: string;
  /** One tamper-evident record of a single retrieval, chained with BLAKE3. */
  receipt: TreeReceipt;
  visited_node_ids: string[];
}

export interface TreeBuildRequest {
  doc_name?: string | null;
  text: string;
}

export interface TreeBuildResponse {
  cache_key: string;
  doc_name: string;
  /** @min 0 */
  node_count: number;
  structure_map: StructureNode[];
  /** A hierarchical, line-addressable index of one document. */
  tree: TreeIndex;
}

export interface TreeChainVerifyRequest {
  receipts: TreeReceipt[];
}

export interface TreeChainVerifyResponse {
  /** @min 0 */
  broken_at?: number | null;
  valid: boolean;
}

/** A citation back to the exact section + line range an answer came from. */
export interface TreeCitation {
  breadcrumb: string;
  lines: number[];
  node_id: string;
  title: string;
}

export interface TreeHybridHit {
  breadcrumb?: string | null;
  /** @format float */
  distance?: number | null;
  lines?: number[] | null;
  node_id?: string | null;
  /**
   * @format int32
   * @min 0
   */
  record_id?: number | null;
  /** @format double */
  score: number;
  source: string;
  text?: string | null;
  title?: string | null;
}

export interface TreeHybridRequest {
  cache_key?: string | null;
  doc_name?: string | null;
  /** @min 0 */
  k?: number;
  namespace?: string | null;
  prev_hash?: string | null;
  query: string;
  text?: string | null;
  tree?: null | TreeIndex;
  /** @format double */
  tree_weight?: number;
}

export interface TreeHybridResponse {
  hits: TreeHybridHit[];
  query: string;
  reasoning: string;
  tree_answer?: null | TreeAnswerResult;
  /** @min 0 */
  tree_hit_count: number;
  /** @min 0 */
  vector_hit_count: number;
}

/** A hierarchical, line-addressable index of one document. */
export interface TreeIndex {
  doc_name: string;
  nodes: Partial<Record<string, TreeNode>>;
  roots: string[];
}

/** One section of a document — a node in the table-of-contents tree. */
export interface TreeNode {
  children?: string[];
  /**
   * Last line owned by this section (excluding children).
   * @min 0
   */
  end_line: number;
  /** @min 0 */
  level: number;
  node_id: string;
  /** Verbatim section body, excluding sub-sections. */
  own_text: string;
  parent?: string | null;
  /**
   * 1-indexed line where this heading appears.
   * @min 0
   */
  start_line: number;
  /** First sentence of the body — a no-LLM summary. */
  summary: string;
  title: string;
}

export interface TreeQueryRequest {
  cache_key?: string | null;
  /** @min 0 */
  k?: number;
  prev_hash?: string | null;
  query: string;
  tree?: null | TreeIndex;
}

/** One tamper-evident record of a single retrieval, chained with BLAKE3. */
export interface TreeReceipt {
  answer_hash: string;
  evidence_hash: string;
  fetched_ranges: number[][];
  hash_algo: string;
  prev_hash: string;
  query: string;
  query_hash: string;
  receipt_hash: string;
  /**
   * @format int64
   * @min 0
   */
  timestamp: number;
  visited_node_ids: string[];
}

export interface TreeVerifyRequest {
  /** One tamper-evident record of a single retrieval, chained with BLAKE3. */
  receipt: TreeReceipt;
  /** A hierarchical, line-addressable index of one document. */
  tree: TreeIndex;
}

export interface TreeVerifyResponse {
  valid: boolean;
}

/**
 * @format int64
 * @min 0
 */
export type U64 = number;

/** `PATCH /v1/records/{id}/metadata`. */
export interface UpdateMetadataResponse {
  /**
   * @format int32
   * @min 0
   */
  id: number;
  ok: boolean;
}

/**
 * `GET /v1/usage` — raw counters only. The node is plan-agnostic: it never
 * returns quota, plan, or billing context.
 */
export interface UsageResponse {
  /** @min 0 */
  collections: number;
  /** @min 0 */
  records: number;
  /** The `storage` sub-object of `GET /v1/usage`. */
  storage: UsageStorage;
}

/** The `storage` sub-object of `GET /v1/usage`. */
export interface UsageStorage {
  /**
   * Live event-log segment plus every rotated archive segment.
   * @format int64
   * @min 0
   */
  event_log_bytes: number;
  /**
   * @format int64
   * @min 0
   */
  snapshot_bytes: number;
  /**
   * @format int64
   * @min 0
   */
  total_bytes: number;
}

export interface WalEntry {
  /** Full object key. */
  key: string;
  /**
   * Segment size in bytes.
   * @format int64
   * @min 0
   */
  size_bytes: number;
}

export type QueryParamsType = Record<string | number, any>;
export type ResponseFormat = keyof Omit<Body, "body" | "bodyUsed">;

export interface FullRequestParams extends Omit<RequestInit, "body"> {
  /** set parameter to `true` for call `securityWorker` for this request */
  secure?: boolean;
  /** request path */
  path: string;
  /** content type of request body */
  type?: ContentType;
  /** query params */
  query?: QueryParamsType;
  /** format of response (i.e. response.json() -> format: "json") */
  format?: ResponseFormat;
  /** request body */
  body?: unknown;
  /** base url */
  baseUrl?: string;
  /** request cancellation token */
  cancelToken?: CancelToken;
}

export type RequestParams = Omit<
  FullRequestParams,
  "body" | "method" | "query" | "path"
>;

export interface ApiConfig<SecurityDataType = unknown> {
  baseUrl?: string;
  baseApiParams?: Omit<RequestParams, "baseUrl" | "cancelToken" | "signal">;
  securityWorker?: (
    securityData: SecurityDataType | null,
  ) => Promise<RequestParams | void> | RequestParams | void;
  customFetch?: typeof fetch;
}

export interface HttpResponse<D extends unknown, E extends unknown = unknown>
  extends Response {
  data: D;
  error: E;
}

type CancelToken = Symbol | string | number;

export enum ContentType {
  Json = "application/json",
  JsonApi = "application/vnd.api+json",
  FormData = "multipart/form-data",
  UrlEncoded = "application/x-www-form-urlencoded",
  Text = "text/plain",
}

export class HttpClient<SecurityDataType = unknown> {
  public baseUrl: string = "/";
  private securityData: SecurityDataType | null = null;
  private securityWorker?: ApiConfig<SecurityDataType>["securityWorker"];
  private abortControllers = new Map<CancelToken, AbortController>();
  private customFetch = (...fetchParams: Parameters<typeof fetch>) =>
    fetch(...fetchParams);

  private baseApiParams: RequestParams = {
    credentials: "same-origin",
    headers: {},
    redirect: "follow",
    referrerPolicy: "no-referrer",
  };

  constructor(apiConfig: ApiConfig<SecurityDataType> = {}) {
    Object.assign(this, apiConfig);
  }

  public setSecurityData = (data: SecurityDataType | null) => {
    this.securityData = data;
  };

  protected encodeQueryParam(key: string, value: any) {
    const encodedKey = encodeURIComponent(key);
    return `${encodedKey}=${encodeURIComponent(typeof value === "number" ? value : `${value}`)}`;
  }

  protected addQueryParam(query: QueryParamsType, key: string) {
    return this.encodeQueryParam(key, query[key]);
  }

  protected addArrayQueryParam(query: QueryParamsType, key: string) {
    const value = query[key];
    return value.map((v: any) => this.encodeQueryParam(key, v)).join("&");
  }

  protected toQueryString(rawQuery?: QueryParamsType): string {
    const query = rawQuery || {};
    const keys = Object.keys(query).filter(
      (key) => "undefined" !== typeof query[key],
    );
    return keys
      .map((key) =>
        Array.isArray(query[key])
          ? this.addArrayQueryParam(query, key)
          : this.addQueryParam(query, key),
      )
      .join("&");
  }

  protected addQueryParams(rawQuery?: QueryParamsType): string {
    const queryString = this.toQueryString(rawQuery);
    return queryString ? `?${queryString}` : "";
  }

  private contentFormatters: Record<ContentType, (input: any) => any> = {
    [ContentType.Json]: (input: any) =>
      input !== null && (typeof input === "object" || typeof input === "string")
        ? JSON.stringify(input)
        : input,
    [ContentType.JsonApi]: (input: any) =>
      input !== null && (typeof input === "object" || typeof input === "string")
        ? JSON.stringify(input)
        : input,
    [ContentType.Text]: (input: any) =>
      input !== null && typeof input !== "string"
        ? JSON.stringify(input)
        : input,
    [ContentType.FormData]: (input: any) => {
      if (input instanceof FormData) {
        return input;
      }

      return Object.keys(input || {}).reduce((formData, key) => {
        const property = input[key];
        formData.append(
          key,
          property instanceof Blob
            ? property
            : typeof property === "object" && property !== null
              ? JSON.stringify(property)
              : `${property}`,
        );
        return formData;
      }, new FormData());
    },
    [ContentType.UrlEncoded]: (input: any) => this.toQueryString(input),
  };

  protected mergeRequestParams(
    params1: RequestParams,
    params2?: RequestParams,
  ): RequestParams {
    return {
      ...this.baseApiParams,
      ...params1,
      ...(params2 || {}),
      headers: {
        ...(this.baseApiParams.headers || {}),
        ...(params1.headers || {}),
        ...((params2 && params2.headers) || {}),
      },
    };
  }

  protected createAbortSignal = (
    cancelToken: CancelToken,
  ): AbortSignal | undefined => {
    if (this.abortControllers.has(cancelToken)) {
      const abortController = this.abortControllers.get(cancelToken);
      if (abortController) {
        return abortController.signal;
      }
      return void 0;
    }

    const abortController = new AbortController();
    this.abortControllers.set(cancelToken, abortController);
    return abortController.signal;
  };

  public abortRequest = (cancelToken: CancelToken) => {
    const abortController = this.abortControllers.get(cancelToken);

    if (abortController) {
      abortController.abort();
      this.abortControllers.delete(cancelToken);
    }
  };

  public request = async <T = any, E = any>({
    body,
    secure,
    path,
    type,
    query,
    format,
    baseUrl,
    cancelToken,
    ...params
  }: FullRequestParams): Promise<HttpResponse<T, E>> => {
    const secureParams =
      ((typeof secure === "boolean" ? secure : this.baseApiParams.secure) &&
        this.securityWorker &&
        (await this.securityWorker(this.securityData))) ||
      {};
    const requestParams = this.mergeRequestParams(params, secureParams);
    const queryString = query && this.toQueryString(query);
    const payloadFormatter = this.contentFormatters[type || ContentType.Json];
    const responseFormat = format || requestParams.format;

    return this.customFetch(
      `${baseUrl || this.baseUrl || ""}${path}${queryString ? `?${queryString}` : ""}`,
      {
        ...requestParams,
        headers: {
          ...(requestParams.headers || {}),
          ...(type && type !== ContentType.FormData
            ? { "Content-Type": type }
            : {}),
        },
        signal:
          (cancelToken
            ? this.createAbortSignal(cancelToken)
            : requestParams.signal) || null,
        body:
          typeof body === "undefined" || body === null
            ? null
            : payloadFormatter(body),
      },
    ).then(async (response) => {
      const r = response as HttpResponse<T, E>;
      r.data = null as unknown as T;
      r.error = null as unknown as E;

      const responseToParse = responseFormat ? response.clone() : response;
      const data = !responseFormat
        ? r
        : await responseToParse[responseFormat]()
            .then((data) => {
              if (r.ok) {
                r.data = data;
              } else {
                r.error = data;
              }
              return r;
            })
            .catch((e) => {
              r.error = e;
              return r;
            });

      if (cancelToken) {
        this.abortControllers.delete(cancelToken);
      }

      if (!response.ok) throw data;
      return data;
    });
  };
}

/**
 * @title Valori Data Plane API
 * @version 1.0.0
 * @license MIT OR Apache-2.0
 * @baseUrl /
 *
 * The Valori Data Plane REST API v1, emitted directly from `#[utoipa::path]` annotations on the registered axum handlers. Every public route the Rust routers register appears here and nothing else does — `scripts/verify-api-route-contract.py` proves that equality on every run of the contract gate, and fails on any drift in either direction. Administrative and operator-internal routes (key management, cluster membership, replication streams, `/metrics`) are deliberately excluded: they are served by the node but are not part of the SDK surface.
 */
export class GeneratedApi<SecurityDataType extends unknown> {
  http: HttpClient<SecurityDataType>;

  constructor(http: HttpClient<SecurityDataType>) {
    this.http = http;
  }

  health = {
    /**
     * @description Always unauthenticated so load-balancer probes work without a token. Returns the legacy top-level fields alongside the structured `engine` / `cluster` sub-objects (Phase API-3 §11).
     *
     * @tags meta
     * @name GetHealth
     * @summary Node health and capacity snapshot
     * @request GET:/health
     */
    getHealth: (params: RequestParams = {}) =>
      this.http.request<HealthResponse, HealthResponse>({
        path: `/health`,
        method: "GET",
        format: "json",
        ...params,
      }),
  };
  v1 = {
    /**
     * @description `path` is a local path on this node. The segment must already be sealed — archiving the live segment is rejected.
     *
     * @tags storage
     * @name ArchiveWalSegment
     * @summary Archive one sealed WAL segment
     * @request POST:/v1/storage/wal/archive
     * @secure
     */
    archiveWalSegment: (data: ArchiveWalRequest, params: RequestParams = {}) =>
      this.http.request<ArchiveWalResponse, ApiError>({
        path: `/v1/storage/wal/archive`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Stateless: no embedding provider is called and nothing is written. Use it to preview how a strategy will split a document before committing to an ingest.
     *
     * @tags ingest
     * @name ChunkDocument
     * @summary Chunk a document without embedding or storing it
     * @request POST:/v1/ingest/document
     * @secure
     */
    chunkDocument: (data: IngestDocumentRequest, params: RequestParams = {}) =>
      this.http.request<IngestDocumentResponse, ApiError>({
        path: `/v1/ingest/document`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Label propagation, O(n+e), with a lowest-label tie-break so the result is deterministic for a given graph. Produces a BLAKE3 receipt over the sorted assignment. Must run before search or overview.
     *
     * @tags community
     * @name CommunityDetect
     * @summary Detect communities in the knowledge graph
     * @request POST:/v1/community/detect
     * @secure
     */
    communityDetect: (
      data: CommunityDetectRequest,
      params: RequestParams = {},
    ) =>
      this.http.request<CommunityDetectResponse, ApiError>({
        path: `/v1/community/detect`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Largest community first, each with up to 10 sample member node ids.
     *
     * @tags community
     * @name CommunityOverview
     * @summary Summarise every detected community
     * @request GET:/v1/community/overview
     * @secure
     */
    communityOverview: (params: RequestParams = {}) =>
      this.http.request<CommunityOverviewResponse, ApiError>({
        path: `/v1/community/overview`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Ranks communities by cosine similarity against their centroids. `drill_in` additionally returns member-level hits.
     *
     * @tags community
     * @name CommunitySearch
     * @summary Search communities by centroid
     * @request POST:/v1/community/search
     * @secure
     */
    communitySearch: (
      data: CommunitySearchRequest,
      params: RequestParams = {},
    ) =>
      this.http.request<CommunitySearchResponse, ApiError>({
        path: `/v1/community/search`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Idempotent. `dimension` and `metric` are always required — a new project has zero collections and `default` carries no implicit config (Phase 3.3).
     *
     * @tags collections
     * @name CreateCollection
     * @summary Create a collection
     * @request POST:/v1/namespaces
     * @secure
     */
    createCollection: (
      data: CreateCollectionRequest,
      params: RequestParams = {},
    ) =>
      this.http.request<CreateCollectionResponse, ApiError>({
        path: `/v1/namespaces`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description `kind` is the numeric EdgeKind discriminant. Both endpoints must already exist in the same collection.
     *
     * @tags graph
     * @name CreateGraphEdge
     * @summary Create a graph edge
     * @request POST:/v1/graph/edge
     * @secure
     */
    createGraphEdge: (data: CreateEdgeRequest, params: RequestParams = {}) =>
      this.http.request<CreateEdgeResponse, ApiError>({
        path: `/v1/graph/edge`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description `kind` is the numeric NodeKind discriminant (0=Document, 1=Chunk, 2=Concept, …). `record_id` optionally binds the node to a stored vector.
     *
     * @tags graph
     * @name CreateGraphNode
     * @summary Create a graph node
     * @request POST:/v1/graph/node
     * @secure
     */
    createGraphNode: (data: CreateNodeRequest, params: RequestParams = {}) =>
      this.http.request<CreateNodeResponse, ApiError>({
        path: `/v1/graph/node`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Removes the collection and every record in it. Returns no body.
     *
     * @tags collections
     * @name DeleteCollection
     * @summary Drop a collection
     * @request DELETE:/v1/namespaces/{name}
     * @secure
     */
    deleteCollection: (name: string, params: RequestParams = {}) =>
      this.http.request<void, ApiError>({
        path: `/v1/namespaces/${name}`,
        method: "DELETE",
        secure: true,
        ...params,
      }),

    /**
     * @description Cascades to every edge incident on the node. Committed to the audit chain.
     *
     * @tags graph
     * @name DeleteGraphNode
     * @summary Delete a graph node
     * @request DELETE:/v1/graph/node/{id}
     * @secure
     */
    deleteGraphNode: (
      id: number,
      query?: {
        collection?: string;
      },
      params: RequestParams = {},
    ) =>
      this.http.request<DeleteNodeResponse, ApiError>({
        path: `/v1/graph/node/${id}`,
        method: "DELETE",
        query: query,
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Frees the slab slot and unlinks the record from its collection. Use `/v1/soft-delete` to tombstone instead.
     *
     * @tags records
     * @name DeleteRecord
     * @summary Hard-delete a record
     * @request POST:/v1/delete
     * @secure
     */
    deleteRecord: (data: DeleteRecordRequest, params: RequestParams = {}) =>
      this.http.request<DeleteRecordResponse, ApiError>({
        path: `/v1/delete`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Streams the V6 snapshot as raw bytes. The format is versioned and self-describing; restore it with `POST /v1/snapshot/upload`.
     *
     * @tags snapshot
     * @name DownloadSnapshot
     * @summary Download a snapshot of the current state
     * @request GET:/v1/snapshot/download
     * @secure
     */
    downloadSnapshot: (params: RequestParams = {}) =>
      this.http.request<Blob, ApiError>({
        path: `/v1/snapshot/download`,
        method: "GET",
        secure: true,
        ...params,
      }),

    /**
     * @description Sends the text to the configured provider, embeds each entity description, inserts the entities as Concept nodes, and adds relationship edges. Requires `VALORI_EMBED_PROVIDER`. The LLM output is committed to the audit chain, so replay never re-invokes the model.
     *
     * @tags community
     * @name ExtractEntities
     * @summary Extract entities and relationships with an LLM
     * @request POST:/v1/ingest/extract-entities
     * @secure
     */
    extractEntities: (
      data: ExtractEntitiesRequest,
      params: RequestParams = {},
    ) =>
      this.http.request<ExtractEntitiesResponse, ApiError>({
        path: `/v1/ingest/extract-entities`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description 503 with `status: no-leader` during an election. Distinct from `GET /health`, which reports this node's own serving capacity.
     *
     * @tags cluster
     * @name GetClusterHealth
     * @summary Whether this node sees an elected leader
     * @request GET:/v1/cluster/health
     * @secure
     */
    getClusterHealth: (params: RequestParams = {}) =>
      this.http.request<
        ClusterHealthResponse,
        ApiError | ClusterHealthResponse
      >({
        path: `/v1/cluster/health`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description The cluster analogue of `GET /v1/proof/state`. Comparing `final_state_hash` across peers at the same `last_applied_index` is how convergence is verified.
     *
     * @tags cluster
     * @name GetClusterProof
     * @summary This node's state hash and applied index
     * @request GET:/v1/cluster/proof
     * @secure
     */
    getClusterProof: (params: RequestParams = {}) =>
      this.http.request<ClusterProofResponse, ApiError>({
        path: `/v1/cluster/proof`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Always 200 — both `leader` and `follower` are healthy. A load balancer can steer writes with this instead of following redirects on every request.
     *
     * @tags cluster
     * @name GetClusterRole
     * @summary Whether this node is the leader
     * @request GET:/v1/cluster/role
     * @secure
     */
    getClusterRole: (params: RequestParams = {}) =>
      this.http.request<ClusterRoleResponse, ApiError>({
        path: `/v1/cluster/role`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Reported from this node's own metrics. `current_leader` is this node's current belief, not a quorum-confirmed fact.
     *
     * @tags cluster
     * @name GetClusterStatus
     * @summary Raft membership and replication position
     * @request GET:/v1/cluster/status
     * @secure
     */
    getClusterStatus: (params: RequestParams = {}) =>
      this.http.request<StatusView, ApiError>({
        path: `/v1/cluster/status`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description `desired_type` is what was asked for; `active_type` and `status` describe what this node is actually serving. In cluster mode `desired_type` comes from the Raft-replicated spec and is cluster-wide, while activation is node-local — the two differ while a build propagates.
     *
     * @tags index
     * @name GetCollectionIndex
     * @summary Read one collection's index lifecycle state
     * @request GET:/v1/namespaces/{name}/index
     * @secure
     */
    getCollectionIndex: (name: string, params: RequestParams = {}) =>
      this.http.request<IndexStatusResponse, ApiError>({
        path: `/v1/namespaces/${name}/index`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description The receipt primitive: the BLAKE3 hash of the event log, the final state hash, and the committed height. Feed it to `valori-verify` to replay and re-derive the chain independently. Requires `VALORI_EVENT_LOG_PATH`.
     *
     * @tags proof
     * @name GetEventLogProof
     * @summary Audit-chain receipt for the event log
     * @request GET:/v1/proof/event-log
     * @secure
     */
    getEventLogProof: (params: RequestParams = {}) =>
      this.http.request<EventProofResponse, ApiError>({
        path: `/v1/proof/event-log`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * No description
     *
     * @tags graph
     * @name GetGraphNode
     * @summary Fetch one graph node
     * @request GET:/v1/graph/node/{id}
     * @secure
     */
    getGraphNode: (
      id: number,
      query?: {
        collection?: string;
      },
      params: RequestParams = {},
    ) =>
      this.http.request<GetNodeResponse, ApiError>({
        path: `/v1/graph/node/${id}`,
        method: "GET",
        query: query,
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Reports how indexing is configured for the node as a whole. Since Phase 4 indexes are per-collection, so `index_type` is `collection_scoped` and the real state lives at `GET /v1/namespaces/{name}/index`.
     *
     * @tags index
     * @name GetIndexConfig
     * @summary Read node-level index configuration
     * @request GET:/v1/index/config
     * @secure
     */
    getIndexConfig: (params: RequestParams = {}) =>
      this.http.request<IndexConfigResponse, ApiError>({
        path: `/v1/index/config`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Job state is held in-process and does not survive a restart. The payload shape depends on which stage the job has reached.
     *
     * @tags ingest
     * @name GetIngestStatus
     * @summary Poll an asynchronous ingest job
     * @request GET:/v1/ingest/status/{job_id}
     * @secure
     */
    getIngestStatus: (jobId: string, params: RequestParams = {}) =>
      this.http.request<IngestJobStatusResponse, ApiError>({
        path: `/v1/ingest/status/${jobId}`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description `exists: false` means the key was shredded and every record encrypted under it is permanently unreadable.
     *
     * @tags crypto
     * @name GetKeyStatus
     * @summary Check whether a crypto-shredding key still exists
     * @request GET:/v1/crypto/status/{key_id}
     * @secure
     */
    getKeyStatus: (keyId: string, params: RequestParams = {}) =>
      this.http.request<CryptoStatusResponse, ApiError>({
        path: `/v1/crypto/status/${keyId}`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Receipts are held in an in-process store, so a restarted node has none until the next write.
     *
     * @tags proof
     * @name GetLatestReceipt
     * @summary Most recent write receipt
     * @request GET:/v1/proof/receipt
     * @secure
     */
    getLatestReceipt: (params: RequestParams = {}) =>
      this.http.request<Receipt, ApiError>({
        path: `/v1/proof/receipt`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description `metadata` is null when nothing has been stored for `target_id`.
     *
     * @tags memory
     * @name GetMetadataSidecar
     * @summary Read sidecar metadata for a target
     * @request GET:/v1/memory/meta/get
     * @secure
     */
    getMetadataSidecar: (
      query: {
        /** Target identifier the metadata was stored under */
        target_id: string;
      },
      params: RequestParams = {},
    ) =>
      this.http.request<MetadataGetResponse, ApiError>({
        path: `/v1/memory/meta/get`,
        method: "GET",
        query: query,
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Verifies the SHA-256 of every package under `VALORI_MODELS_DIR`. `reclaimable_bytes` counts packages no project references.
     *
     * @tags meta
     * @name GetModelsHealth
     * @summary Integrity report for installed model packages
     * @request GET:/v1/models/health
     * @secure
     */
    getModelsHealth: (params: RequestParams = {}) =>
      this.http.request<SystemHealth, ApiError>({
        path: `/v1/models/health`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Accepts the canonical string `id`. Numeric identifiers minted before Phase API-3 remain resolvable (§13).
     *
     * @tags operations
     * @name GetOperation
     * @summary Fetch one operation with its proof and metrics
     * @request GET:/v1/operations/{id}
     * @secure
     */
    getOperation: (id: string, params: RequestParams = {}) =>
      this.http.request<OperationDetailResponse, ApiError>({
        path: `/v1/operations/${id}`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description The Execution Explorer payload: every pipeline stage with its duration, metrics, and warnings, plus the state hash before and after. Held in an in-process registry, so it does not survive a restart.
     *
     * @tags operations
     * @name GetOperationExecution
     * @summary Per-stage execution breakdown for one operation
     * @request GET:/v1/operations/{id}/execution
     * @secure
     */
    getOperationExecution: (id: string, params: RequestParams = {}) =>
      this.http.request<ExecutionRecord, ApiError>({
        path: `/v1/operations/${id}/execution`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * No description
     *
     * @tags proof
     * @name GetReceipt
     * @summary One write receipt by id
     * @request GET:/v1/proof/receipt/{id}
     * @secure
     */
    getReceipt: (id: string, params: RequestParams = {}) =>
      this.http.request<Receipt, ApiError>({
        path: `/v1/proof/receipt/${id}`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Returns the stored vector converted back to f32, plus whatever metadata was committed with it. The vector round-trips through Q16.16, so it is equal to the inserted value only to the fixed-point quantum.
     *
     * @tags records
     * @name GetRecord
     * @summary Fetch one record by id
     * @request GET:/v1/records/{id}
     * @secure
     */
    getRecord: (
      id: number,
      query?: {
        collection?: string;
      },
      params: RequestParams = {},
    ) =>
      this.http.request<RecordResponse, ApiError>({
        path: `/v1/records/${id}`,
        method: "GET",
        query: query,
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Routing is `namespace_id % shard_count` and is stable for the life of the collection. With `shard_count = 1` every collection reports shard 0.
     *
     * @tags meta
     * @name GetShardRouting
     * @summary Show which shard each collection routes to
     * @request GET:/v1/shard/routing
     * @secure
     */
    getShardRouting: (params: RequestParams = {}) =>
      this.http.request<ShardRoutingResponse, ApiError>({
        path: `/v1/shard/routing`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description The Merkle root over every applied event. Two nodes with identical histories produce byte-identical values, which is what the cluster convergence watcher compares.
     *
     * @tags proof
     * @name GetStateProof
     * @summary Current BLAKE3 state hash
     * @request GET:/v1/proof/state
     * @secure
     */
    getStateProof: (params: RequestParams = {}) =>
      this.http.request<StateProofResponse, ApiError>({
        path: `/v1/proof/state`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Names the current snapshot and every archived WAL segment in one object. `manifest: null` means the store is configured but nothing has been uploaded through it yet — not an error.
     *
     * @tags storage
     * @name GetStorageManifest
     * @summary Read the object-store manifest
     * @request GET:/v1/storage/manifest
     * @secure
     */
    getStorageManifest: (params: RequestParams = {}) =>
      this.http.request<ManifestResponse, ApiError>({
        path: `/v1/storage/manifest`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Breadth-first expansion bounded by `depth`. Traversal never crosses a collection boundary.
     *
     * @tags graph
     * @name GetSubgraph
     * @summary Expand a subgraph around a root node
     * @request GET:/v1/graph/subgraph
     * @secure
     */
    getSubgraph: (
      query: {
        collection?: string;
        /**
         * @format int32
         * @min 0
         */
        depth?: number;
        /**
         * @format int32
         * @min 0
         */
        root: number;
      },
      params: RequestParams = {},
    ) =>
      this.http.request<SubgraphResponse, ApiError>({
        path: `/v1/graph/subgraph`,
        method: "GET",
        query: query,
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Reads the event log directly, so it reflects committed state only. Known limitation: with `VALORI_SHARD_COUNT > 1` this reads shard 0's log.
     *
     * @tags proof
     * @name GetTimeline
     * @summary Committed events in chronological order
     * @request GET:/v1/timeline
     * @secure
     */
    getTimeline: (
      query?: {
        /**
         * Filter to events in a specific collection (not yet applied at kernel level;
         * kept for future use when namespace is stored per-event).
         */
        collection?: string;
        /** ISO 8601 UTC lower bound (inclusive). */
        from?: string;
        /**
         * Return only the N most-recent events. Applied after timestamp filtering.
         * @min 0
         */
        limit?: number;
        /** ISO 8601 UTC upper bound (inclusive). */
        to?: string;
      },
      params: RequestParams = {},
    ) =>
      this.http.request<TimelineResponse, ApiError>({
        path: `/v1/timeline`,
        method: "GET",
        query: query,
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Read-only: takes no write lock, commits no event, and returns no plan or billing context — the node is deliberately plan-agnostic. `event_log_bytes` includes every rotated archive segment, not just the live one.
     *
     * @tags meta
     * @name GetUsage
     * @summary Raw usage counters
     * @request GET:/v1/usage
     * @secure
     */
    getUsage: (params: RequestParams = {}) =>
      this.http.request<UsageResponse, ApiError>({
        path: `/v1/usage`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Plain text, not JSON — the crate version string and nothing else. Authenticated like any other read: a read-only key is enough.
     *
     * @tags meta
     * @name GetVersion
     * @summary Node build version
     * @request GET:/v1/version
     * @secure
     */
    getVersion: (params: RequestParams = {}) =>
      this.http.request<string, ApiError>({
        path: `/v1/version`,
        method: "GET",
        secure: true,
        ...params,
      }),

    /**
     * @description Walks the graph from `start` with optional edge-kind and node-kind filters. Result order is deterministic for a given kernel state.
     *
     * @tags graph
     * @name GraphQuery
     * @summary Deterministic bounded graph traversal
     * @request GET:/v1/graph/query
     * @secure
     */
    graphQuery: (
      query: {
        collection?: string;
        /**
         * @format int32
         * @min 0
         */
        depth?: number;
        /** `"outgoing"` (default) | `"incoming"` | `"both"` — case-insensitive. */
        direction?: string;
        /**
         * Restrict traversal to a single edge kind (same u8 encoding as
         * `CreateEdgeRequest::kind`). Absent = no restriction.
         * @format int32
         * @min 0
         */
        edge_kind?: number;
        /** @min 0 */
        limit?: number;
        /**
         * Restrict traversal to a single node kind (same u8 encoding as
         * `ListNodesQuery::kind`). Absent = no restriction.
         * @format int32
         * @min 0
         */
        node_kind?: number;
        /**
         * @format int32
         * @min 0
         */
        start: number;
      },
      params: RequestParams = {},
    ) =>
      this.http.request<GraphQueryResponse, ApiError>({
        path: `/v1/graph/query`,
        method: "GET",
        query: query,
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Retrieves the K nearest vectors and the connected subgraph around them from a single consistent kernel snapshot. `final_score = (1-graph_weight)*vector_rel + graph_weight*graph_rel`.
     *
     * @tags graph
     * @name Graphrag
     * @summary Vector search plus graph expansion in one read
     * @request POST:/v1/graphrag
     * @secure
     */
    graphrag: (data: GraphRagRequest, params: RequestParams = {}) =>
      this.http.request<GraphRagResponse, ApiError>({
        path: `/v1/graphrag`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description The full pipeline in one call. Requires `VALORI_EMBED_PROVIDER`. With `async: true` the call returns immediately and progress is polled through `GET /v1/ingest/status/{job_id}`.
     *
     * @tags ingest
     * @name IngestDocument
     * @summary Chunk, embed, and insert a document
     * @request POST:/v1/ingest
     * @secure
     */
    ingestDocument: (
      data: IngestRequest,
      query?: {
        async?: boolean;
      },
      params: RequestParams = {},
    ) =>
      this.http.request<IngestResponse, ApiError>({
        path: `/v1/ingest`,
        method: "POST",
        query: query,
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description The payload is encrypted with a per-record key held in the node vault. Deleting that key through `DELETE /v1/crypto/shred/{key_id}` renders the record permanently unreadable without rewriting the audit chain.
     *
     * @tags records
     * @name InsertEncryptedRecord
     * @summary Insert a crypto-shreddable record
     * @request POST:/v1/records/encrypted
     * @secure
     */
    insertEncryptedRecord: (
      data: InsertEncryptedRequest,
      params: RequestParams = {},
    ) =>
      this.http.request<InsertEncryptedResponse, ApiError>({
        path: `/v1/records/encrypted`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Q16.16 fixed-point insert. Supplying `request_id` makes the call idempotent: a replay returns the original record id rather than inserting twice.
     *
     * @tags records
     * @name InsertRecord
     * @summary Insert a record
     * @request POST:/v1/records
     * @secure
     */
    insertRecord: (data: InsertRecordRequest, params: RequestParams = {}) =>
      this.http.request<InsertRecordResponse, ApiError>({
        path: `/v1/records`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Each optional per-item array (`metadata`, `request_ids`, `texts`) must be the same length as `batch` when present. A repeated `request_id` skips that item and returns the id assigned the first time, so the whole call is idempotent per item.
     *
     * @tags records
     * @name InsertRecordsBatch
     * @summary Insert many vectors in one request
     * @request POST:/v1/vectors/batch-insert
     * @secure
     */
    insertRecordsBatch: (
      data: BatchInsertRequest,
      params: RequestParams = {},
    ) =>
      this.http.request<BatchInsertResponse, ApiError>({
        path: `/v1/vectors/batch-insert`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * No description
     *
     * @tags storage
     * @name ListArchivedWalSegments
     * @summary List archived WAL segments in the object store
     * @request GET:/v1/storage/wal
     * @secure
     */
    listArchivedWalSegments: (params: RequestParams = {}) =>
      this.http.request<ListRemoteWalResponse, ApiError>({
        path: `/v1/storage/wal`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Returns an empty list for a brand-new project. Each entry carries its dimension, metric, index kind, and record count.
     *
     * @tags collections
     * @name ListCollections
     * @summary List collections
     * @request GET:/v1/namespaces
     * @secure
     */
    listCollections: (params: RequestParams = {}) =>
      this.http.request<ListCollectionsResponse, ApiError>({
        path: `/v1/namespaces`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description `count` is the size of the filtered set before pagination; `nodes` is the page. Omitting `limit` returns everything.
     *
     * @tags graph
     * @name ListGraphNodes
     * @summary List graph nodes
     * @request GET:/v1/graph/nodes
     * @secure
     */
    listGraphNodes: (
      query?: {
        collection?: string;
        /**
         * Filter to a single node kind (0=Document, 1=Chunk, 2=Concept, …).
         * @format int32
         * @min 0
         */
        kind?: number;
        /** @min 0 */
        limit?: number;
        /**
         * Pagination — applied after the `kind` filter. Absent `limit` returns
         * everything (backward compatible with clients that predate pagination).
         * @min 0
         */
        offset?: number;
      },
      params: RequestParams = {},
    ) =>
      this.http.request<ListNodesResponse, ApiError>({
        path: `/v1/graph/nodes`,
        method: "GET",
        query: query,
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * No description
     *
     * @tags graph
     * @name ListNodeEdges
     * @summary List the edges leaving one node
     * @request GET:/v1/graph/edges/{id}
     * @secure
     */
    listNodeEdges: (
      id: number,
      query?: {
        collection?: string;
      },
      params: RequestParams = {},
    ) =>
      this.http.request<GetEdgesResponse, ApiError>({
        path: `/v1/graph/edges/${id}`,
        method: "GET",
        query: query,
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Requires `VALORI_OBJECT_STORE_URL`. Prefer `GET /v1/storage/manifest` for disaster recovery rather than sorting these keys by hand.
     *
     * @tags storage
     * @name ListObjectStoreSnapshots
     * @summary List snapshots in the object store
     * @request GET:/v1/storage/snapshots
     * @secure
     */
    listObjectStoreSnapshots: (params: RequestParams = {}) =>
      this.http.request<ListRemoteSnapshotsResponse, ApiError>({
        path: `/v1/storage/snapshots`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Derived from the BLAKE3-chained event log. `id` is the canonical string identity (Phase API-3 §13).
     *
     * @tags operations
     * @name ListOperations
     * @summary List committed operations
     * @request GET:/v1/operations
     * @secure
     */
    listOperations: (params: RequestParams = {}) =>
      this.http.request<OperationsListResponse, ApiError>({
        path: `/v1/operations`,
        method: "GET",
        secure: true,
        format: "json",
        ...params,
      }),

    /**
     * @description Commits three events atomically: soft-delete of the old record, insert of the new one, and a Supersedes edge from new to old. The returned `state_hash` covers all three.
     *
     * @tags memory
     * @name MemoryConsolidate
     * @summary Replace a memory and record the supersession
     * @request POST:/v1/memory/consolidate
     * @secure
     */
    memoryConsolidate: (
      data: MemoryConsolidateRequest,
      params: RequestParams = {},
    ) =>
      this.http.request<MemoryConsolidateResponse, ApiError>({
        path: `/v1/memory/consolidate`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Computes cosine similarity between the two records. When it meets `threshold` (default 0.85) a Contradicts edge is committed and its id returned. Below the threshold nothing is written.
     *
     * @tags memory
     * @name MemoryContradict
     * @summary Test two memories for contradiction
     * @request POST:/v1/memory/contradict
     * @secure
     */
    memoryContradict: (
      data: MemoryContradictRequest,
      params: RequestParams = {},
    ) =>
      this.http.request<MemoryContradictResponse, ApiError>({
        path: `/v1/memory/contradict`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Vector recall with optional recency decay, metadata filtering, and hybrid term re-ranking. When `decay_half_life_secs` is set, each hit also carries `decay_factor` and `age_secs`; `score` remains the true distance. Add `?explain=true` for an `_execution` block describing the plan that ran.
     *
     * @tags memory
     * @name MemorySearch
     * @summary Recall agent memories
     * @request POST:/v1/memory/search
     * @secure
     */
    memorySearch: (
      data: MemorySearchVectorRequest,
      query?: {
        explain?: boolean;
      },
      params: RequestParams = {},
    ) =>
      this.http.request<MemorySearchResponse, ApiError>({
        path: `/v1/memory/search`,
        method: "POST",
        query: query,
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Identical to `POST /v1/memory/search`. This is the path the Python SDK has always used; both are supported and neither is deprecated.
     *
     * @tags memory
     * @name MemorySearchVector
     * @summary Recall agent memories (SDK path)
     * @request POST:/v1/memory/search_vector
     * @secure
     */
    memorySearchVector: (
      data: MemorySearchVectorRequest,
      query?: {
        explain?: boolean;
      },
      params: RequestParams = {},
    ) =>
      this.http.request<MemorySearchResponse, ApiError>({
        path: `/v1/memory/search_vector`,
        method: "POST",
        query: query,
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Inserts the vector and links it into the knowledge graph as a chunk node under a document node, returning both ids alongside a stable `memory_id`.
     *
     * @tags memory
     * @name MemoryUpsert
     * @summary Store an agent memory
     * @request POST:/v1/memory/upsert
     * @secure
     */
    memoryUpsert: (
      data: MemoryUpsertVectorRequest,
      params: RequestParams = {},
    ) =>
      this.http.request<MemoryUpsertResponse, ApiError>({
        path: `/v1/memory/upsert`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Identical to `POST /v1/memory/upsert`. This is the path the Python SDK has always used; both are supported and neither is deprecated.
     *
     * @tags memory
     * @name MemoryUpsertVector
     * @summary Store an agent memory (SDK path)
     * @request POST:/v1/memory/upsert_vector
     * @secure
     */
    memoryUpsertVector: (
      data: MemoryUpsertVectorRequest,
      params: RequestParams = {},
    ) =>
      this.http.request<MemoryUpsertResponse, ApiError>({
        path: `/v1/memory/upsert_vector`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Synchronous and project-wide: the write lock is held for the duration. For a single collection, and for asynchronous builds, use `POST /v1/namespaces/{name}/index` instead.
     *
     * @tags index
     * @name RebuildIndexes
     * @summary Rebuild every per-collection index
     * @request POST:/v1/index/rebuild
     * @secure
     */
    rebuildIndexes: (data: IndexRebuildRequest, params: RequestParams = {}) =>
      this.http.request<IndexRebuildResponse, ApiError>({
        path: `/v1/index/rebuild`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Reads `path` from this node's own filesystem. Destructive: the current state is replaced.
     *
     * @tags snapshot
     * @name RestoreSnapshot
     * @summary Restore state from a local snapshot file
     * @request POST:/v1/snapshot/restore
     * @secure
     */
    restoreSnapshot: (
      data: SnapshotRestoreRequest,
      params: RequestParams = {},
    ) =>
      this.http.request<SnapshotRestoreResponse, ApiError>({
        path: `/v1/snapshot/restore`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Omit `key` to restore whatever `manifest.json` names as current — the recommended disaster-recovery entry point. Destructive.
     *
     * @tags storage
     * @name RestoreSnapshotFromObjectStore
     * @summary Restore from a snapshot in the object store
     * @request POST:/v1/storage/snapshots/restore
     * @secure
     */
    restoreSnapshotFromObjectStore: (
      data: RestoreFromStoreRequest,
      params: RequestParams = {},
    ) =>
      this.http.request<RestoreFromStoreResponse, ApiError>({
        path: `/v1/storage/snapshots/restore`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Writes to `path` when given, otherwise to `VALORI_SNAPSHOT_PATH`. Fails when neither is set.
     *
     * @tags snapshot
     * @name SaveSnapshot
     * @summary Write a snapshot to local disk
     * @request POST:/v1/snapshot/save
     * @secure
     */
    saveSnapshot: (data: SnapshotSaveRequest, params: RequestParams = {}) =>
      this.http.request<SnapshotSaveResponse, ApiError>({
        path: `/v1/snapshot/save`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Composable ranking: `decay_half_life_secs` applies recency decay, `query_text` enables the Valori Reranker's term blend, `graph_rerank` nudges by graph proximity, and `metadata_filter` restricts candidates. `k` must be in 1..=5000.
     *
     * @tags search
     * @name Search
     * @summary K-nearest-neighbour search within one collection
     * @request POST:/v1/search
     * @secure
     */
    search: (data: SearchRequest, params: RequestParams = {}) =>
      this.http.request<SearchResponse, ApiError>({
        path: `/v1/search`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description All named collections must share a dimension and metric — scores from different corpora are incomparable and would corrupt the merge. Collections that fail individually are reported in `partial_failures` rather than failing the whole request.
     *
     * @tags search
     * @name SearchMulti
     * @summary Search several compatible collections and merge the results
     * @request POST:/v1/search/multi
     * @secure
     */
    searchMulti: (data: MultiSearchRequest, params: RequestParams = {}) =>
      this.http.request<MultiSearchResponse, ApiError>({
        path: `/v1/search/multi`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description `type` is `hnsw`, `ivf`, `bq`, or null to drop the index and revert to exact search. A build is asynchronous: 202 means the build started, and the response carries the building generation. Poll the GET form for completion.
     *
     * @tags index
     * @name SetCollectionIndex
     * @summary Create, change, or drop a collection index
     * @request POST:/v1/namespaces/{name}/index
     * @secure
     */
    setCollectionIndex: (
      name: string,
      data: IndexBuildRequest,
      params: RequestParams = {},
    ) =>
      this.http.request<IndexStatusResponse, ApiError>({
        path: `/v1/namespaces/${name}/index`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Sidecar metadata is node-local: it is NOT replicated through Raft and NOT part of the BLAKE3 audit chain. Use record metadata when the value must be provable.
     *
     * @tags memory
     * @name SetMetadataSidecar
     * @summary Attach sidecar metadata to a target
     * @request POST:/v1/memory/meta/set
     * @secure
     */
    setMetadataSidecar: (
      data: MetadataSetRequest,
      params: RequestParams = {},
    ) =>
      this.http.request<MetadataSetResponse, ApiError>({
        path: `/v1/memory/meta/set`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Tombstones the record so it stops appearing in search results while its slab slot and audit history are retained.
     *
     * @tags records
     * @name SoftDeleteRecord
     * @summary Soft-delete a record
     * @request POST:/v1/soft-delete
     * @secure
     */
    softDeleteRecord: (data: DeleteRecordRequest, params: RequestParams = {}) =>
      this.http.request<DeleteRecordResponse, ApiError>({
        path: `/v1/soft-delete`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Zero-LLM: pure header parsing. Returns the full tree plus a `cache_key` (BLAKE3 of the input) that later query and hybrid calls can send instead of re-transmitting the whole tree.
     *
     * @tags tree
     * @name TreeBuild
     * @summary Parse a markdown document into a navigable tree
     * @request POST:/v1/tree/build
     * @secure
     */
    treeBuild: (data: TreeBuildRequest, params: RequestParams = {}) =>
      this.http.request<TreeBuildResponse, ApiError>({
        path: `/v1/tree/build`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Checks each receipt individually and that each `prev_hash` matches its predecessor. `broken_at` is the index of the first receipt that fails, or null.
     *
     * @tags tree
     * @name TreeChainVerify
     * @summary Verify an ordered chain of receipts
     * @request POST:/v1/tree/chain-verify
     * @secure
     */
    treeChainVerify: (
      data: TreeChainVerifyRequest,
      params: RequestParams = {},
    ) =>
      this.http.request<TreeChainVerifyResponse, ApiError>({
        path: `/v1/tree/chain-verify`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description `tree_weight` (default 0.6) sets the mix between tree hits and vector hits. Each hit records which source produced it.
     *
     * @tags tree
     * @name TreeHybrid
     * @summary Blend tree navigation with vector search
     * @request POST:/v1/tree/hybrid
     * @secure
     */
    treeHybrid: (data: TreeHybridRequest, params: RequestParams = {}) =>
      this.http.request<TreeHybridResponse, ApiError>({
        path: `/v1/tree/hybrid`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Deterministic table-of-contents navigation with breadcrumb citations. Every answer carries a BLAKE3 receipt chained onto `prev_hash`. Send either `tree` or a `cache_key` from a previous build.
     *
     * @tags tree
     * @name TreeQuery
     * @summary Navigate a document tree to an answer
     * @request POST:/v1/tree/query
     * @secure
     */
    treeQuery: (data: TreeQueryRequest, params: RequestParams = {}) =>
      this.http.request<TreeAnswerResult, ApiError>({
        path: `/v1/tree/query`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Stateless and side-effect free: re-derives the receipt's hashes from the supplied tree and reports whether they match. A caller can run the same check offline.
     *
     * @tags tree
     * @name TreeVerify
     * @summary Replay one receipt against its tree
     * @request POST:/v1/tree/verify
     * @secure
     */
    treeVerify: (data: TreeVerifyRequest, params: RequestParams = {}) =>
      this.http.request<TreeVerifyResponse, ApiError>({
        path: `/v1/tree/verify`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Diffs the new chunk set against the stored one by BLAKE3 content hash. Unchanged chunks keep their existing records and are never re-embedded; the counts in the response say exactly what happened.
     *
     * @tags ingest
     * @name UpdateIngestedDocument
     * @summary Re-ingest a document, re-embedding only what changed
     * @request POST:/v1/ingest/update
     * @secure
     */
    updateIngestedDocument: (
      data: IngestUpdateRequest,
      params: RequestParams = {},
    ) =>
      this.http.request<IngestUpdateResponse, ApiError>({
        path: `/v1/ingest/update`,
        method: "POST",
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description The request body replaces the stored metadata blob wholesale — this is not a merge. The vector is untouched. The change is committed to the BLAKE3 audit chain.
     *
     * @tags records
     * @name UpdateRecordMetadata
     * @summary Replace a record's metadata
     * @request PATCH:/v1/records/{id}/metadata
     * @secure
     */
    updateRecordMetadata: (
      id: number,
      data: Partial<Record<string, any>>,
      query?: {
        collection?: string;
      },
      params: RequestParams = {},
    ) =>
      this.http.request<UpdateMetadataResponse, ApiError>({
        path: `/v1/records/${id}/metadata`,
        method: "PATCH",
        query: query,
        body: data,
        secure: true,
        type: ContentType.Json,
        format: "json",
        ...params,
      }),

    /**
     * @description Replaces the entire in-memory state with the uploaded snapshot and rebuilds the state hash from scratch. Destructive.
     *
     * @tags snapshot
     * @name UploadSnapshot
     * @summary Restore state from an uploaded snapshot
     * @request POST:/v1/snapshot/upload
     * @secure
     */
    uploadSnapshot: (data: SnapshotBytes, params: RequestParams = {}) =>
      this.http.request<void, ApiError>({
        path: `/v1/snapshot/upload`,
        method: "POST",
        body: data,
        secure: true,
        ...params,
      }),

    /**
     * @description Takes a snapshot, uploads it, prunes to `VALORI_OBJECT_STORE_KEEP`, and rewrites `manifest.json` to name the new snapshot as current.
     *
     * @tags storage
     * @name UploadSnapshotToObjectStore
     * @summary Offload a snapshot to the object store
     * @request POST:/v1/storage/snapshots/upload
     * @secure
     */
    uploadSnapshotToObjectStore: (params: RequestParams = {}) =>
      this.http.request<StorageSnapshotUploadResponse, ApiError>({
        path: `/v1/storage/snapshots/upload`,
        method: "POST",
        secure: true,
        format: "json",
        ...params,
      }),
  };
}
