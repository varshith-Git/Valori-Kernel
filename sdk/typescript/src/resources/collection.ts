// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Collections, records, search, index, graph and memory — Phase API-4A §10/§11.
//
// A `Collection` is a handle, not a fetched document: the contract has no
// `GET /v1/namespaces/{name}`, so `client.collection("docs")` costs nothing.
// `collections.get(name)` is the checked form and pays one list request.
// No endpoint was invented to make the ergonomics nicer.

import type {
  ApiError,
  BatchInsertRequest,
  BatchInsertResponse,
  CreateCollectionRequest,
  CreateCollectionResponse,
  CreateEdgeResponse,
  CreateNodeResponse,
  DeleteRecordResponse,
  GetNodeResponse,
  GraphQueryResponse,
  GraphRagRequest,
  GraphRagResponse,
  IndexBuildParameters,
  IndexBuildRequest,
  IndexStatusResponse,
  InsertEncryptedResponse,
  InsertRecordResponse,
  ListCollectionsResponse,
  ListNodesResponse,
  MemoryConsolidateRequest,
  MemoryConsolidateResponse,
  MemoryContradictRequest,
  MemoryContradictResponse,
  MemorySearchVectorRequest,
  MemoryUpsertVectorRequest,
  MetadataGetResponse,
  MetadataSetResponse,
  MultiSearchResponse,
  RecordResponse,
  SearchRequest,
  SearchResponse,
  SubgraphResponse,
} from "../../generated/valori-api.js";
// Value imports: these enums are read at runtime by `asEnum` below.
import {
  BuildableIndexKind,
  IndexKindInput,
  MetricInput,
} from "../../generated/valori-api.js";
import {
  CollectionAlreadyExistsError,
  CollectionNotFoundError,
  ConflictError,
  IndexBuildFailedError,
  OperationTimeoutError,
  ValoriConfigError,
} from "../errors.js";
import type { CallOptions, Transport, V1Data } from "../transport.js";

const omitUndefined = <T extends object>(value: T): T =>
  Object.fromEntries(Object.entries(value).filter(([, v]) => v !== undefined)) as T;

/**
 * Contract enums, re-expressed as string unions — Phase API-4D §5.
 *
 * The generated file emits these as TypeScript `enum`s. Importing the enum
 * *type* into a public signature would force callers to write
 * `MetricInput.SquaredL2` instead of `"squared_l2"`, so each is widened to the
 * union of its own members with a template-literal type. That keeps the
 * ergonomic string form while still rejecting `"hsnw"` at compile time.
 *
 * These are derived from the generated enums, never hand-listed: adding a
 * metric to the contract widens them automatically, and removing one breaks
 * the call sites that still use it. Before API-4D these three were typed
 * `string`, and an `as unknown as` cast at the wire boundary hid the gap.
 */
export type MetricValue = `${MetricInput}`;
export type IndexKindValue = `${IndexKindInput}`;
export type BuildableIndexKindValue = `${BuildableIndexKind}`;

/**
 * The SDK's one unavoidable unsound cast, isolated here — Phase API-4D §5.
 *
 * TypeScript `enum`s are nominal: the string `"hnsw"` is not assignable to
 * `BuildableIndexKind.Hnsw` even though they are the same value at runtime and
 * on the wire. The ergonomic API takes the string union (so callers write
 * `"hnsw"`, not `BuildableIndexKind.Hnsw`); the generated request type wants
 * the enum. Something has to bridge them.
 *
 * §5 permits exactly this shape — one boundary function, documented, with
 * *runtime validation* so the cast can never be wrong. The membership check
 * below is not decoration: it is what makes the assertion sound. A value that
 * is not a member throws here rather than being cast into a lie, so the cast
 * is only reached for values that provably are members of the enum.
 *
 * Covered by `tests/enum-boundary.test.ts`.
 */
const asEnum = <E extends Record<string, string>>(
  members: E,
  value: string | undefined,
  field: string,
): E[keyof E] | undefined => {
  if (value === undefined) return undefined;
  const allowed = Object.values(members);
  if (!allowed.includes(value)) {
    throw new ValoriConfigError(
      `invalid ${field}: ${JSON.stringify(value)} — expected one of ${allowed
        .map((v) => JSON.stringify(v))
        .join(", ")}`,
    );
  }
  // Sound: `value` was just proven to be one of `Object.values(members)`.
  return value as E[keyof E];
};

/**
 * `metadata` has three different wire shapes in the contract, and the ergonomic
 * layer hides all three behind one plain JSON object (§24):
 *
 *   * `POST /v1/records`               → `number[]`, opaque UTF-8 JSON bytes
 *   * `POST /v1/vectors/batch-insert`  → `string[]`, UTF-8 JSON strings
 *   * memory upsert / metadata set     → a real JSON object, sent as-is
 *
 * The first two are committed *inside* the `InsertRecord` event and are
 * therefore covered by the BLAKE3 audit chain, which is why the node takes
 * bytes rather than a map: the encoding has to be the caller's, byte for byte.
 * Encoding here keeps that property while letting callers pass an object.
 */
const encodeMetadataBytes = (metadata: Record<string, unknown> | undefined): number[] | undefined =>
  metadata === undefined ? undefined : Array.from(new TextEncoder().encode(JSON.stringify(metadata)));

const encodeMetadataString = (metadata: Record<string, unknown> | null | undefined): string | null =>
  metadata === undefined || metadata === null ? null : JSON.stringify(metadata);

/** Index lifecycle states meaning "the build is over" (valori-engine emits these). */
export const TERMINAL_INDEX_STATES = ["active", "failed", "none"] as const;

export interface WaitOptions {
  pollIntervalMs?: number;
  timeoutMs?: number;
  throwOnFailure?: boolean;
  /** Injected by tests so nothing actually sleeps. */
  sleep?: (ms: number) => Promise<void>;
  now?: () => number;
}

const realSleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

// ── records ──────────────────────────────────────────────────────────────────

export class Records {
  constructor(
    private readonly t: Transport,
    private readonly collection: string,
  ) {}

  /** `POST /v1/records`. Pass `requestId` to make the write dedupable and retryable. */
  insert(
    values: number[],
    options: {
      metadata?: Record<string, unknown>;
      text?: string;
      tag?: number;
      requestId?: string;
    } = {},
  ): Promise<InsertRecordResponse> {
    const body = omitUndefined({
      values,
      collection: this.collection,
      metadata: encodeMetadataBytes(options.metadata),
      text: options.text,
      tag: options.tag,
      request_id: options.requestId,
    }) satisfies Parameters<Transport["api"]["v1"]["insertRecord"]>[0];
    return this.t.call(() =>
      this.t.api.v1.insertRecord(body, this.t.params({ requestId: options.requestId })),
    );
  }

  /** `POST /v1/vectors/batch-insert`. */
  insertBatch(
    batch: number[][],
    options: {
      metadata?: (Record<string, unknown> | null)[];
      texts?: string[];
      requestIds?: string[];
    } = {},
  ): Promise<BatchInsertResponse> {
    const body = omitUndefined({
      batch,
      collection: this.collection,
      metadata: options.metadata?.map(encodeMetadataString),
      texts: options.texts,
      request_ids: options.requestIds,
    }) satisfies BatchInsertRequest;
    return this.t.call(() => this.t.api.v1.insertRecordsBatch(body, this.t.params()));
  }

  /** `POST /v1/records/encrypted`. */
  insertEncrypted(
    payload: string,
    options: { keyId?: string; tag?: number } = {},
  ): Promise<InsertEncryptedResponse> {
    const body = omitUndefined({
      payload,
      collection: this.collection,
      key_id: options.keyId,
      tag: options.tag,
    }) satisfies Parameters<Transport["api"]["v1"]["insertEncryptedRecord"]>[0];
    return this.t.call(() => this.t.api.v1.insertEncryptedRecord(body, this.t.params()));
  }

  /** `GET /v1/records/{id}`. */
  get(recordId: number): Promise<RecordResponse> {
    return this.t.call(() =>
      this.t.api.v1.getRecord(recordId, { collection: this.collection }, this.t.params()),
    );
  }

  /** `POST /v1/delete` — hard delete. */
  delete(recordId: number): Promise<DeleteRecordResponse> {
    return this.t.call(() =>
      this.t.api.v1.deleteRecord({ id: recordId, collection: this.collection }, this.t.params()),
    );
  }

  /** `POST /v1/soft-delete` — tombstone without reclaiming the slot. */
  softDelete(recordId: number): Promise<DeleteRecordResponse> {
    return this.t.call(() =>
      this.t.api.v1.softDeleteRecord(
        { id: recordId, collection: this.collection },
        this.t.params(),
      ),
    );
  }

  /** `PATCH /v1/records/{id}/metadata`. */
  updateMetadata(
    recordId: number,
    metadata: Record<string, unknown>,
  ): Promise<V1Data<"updateRecordMetadata">> {
    return this.t.call(() =>
      this.t.api.v1.updateRecordMetadata(
        recordId,
        metadata,
        { collection: this.collection },
        this.t.params(),
      ),
    );
  }
}

// ── index ────────────────────────────────────────────────────────────────────

export class CollectionIndexResource {
  constructor(
    private readonly t: Transport,
    private readonly collection: string,
  ) {}

  /** `POST /v1/namespaces/{name}/index`. Returns immediately; the build runs on. */
  build(
    type?: BuildableIndexKindValue,
    parameters?: IndexBuildParameters,
  ): Promise<IndexStatusResponse> {
    const body = omitUndefined({
      type: asEnum(BuildableIndexKind, type, "index type"),
      parameters,
    }) satisfies IndexBuildRequest;
    return this.t.call(() =>
      this.t.api.v1.setCollectionIndex(this.collection, body, this.t.params()),
    );
  }

  /** `GET /v1/namespaces/{name}/index`. */
  status(): Promise<IndexStatusResponse> {
    return this.t.call(() =>
      this.t.api.v1.getCollectionIndex(this.collection, this.t.params()),
    );
  }

  /**
   * Poll `status()` until the build settles.
   *
   * §9: interval, deadline, terminal-state recognition and failure conversion
   * are owned here. The generated client stays a single-shot transport.
   */
  async wait(options: WaitOptions = {}): Promise<IndexStatusResponse> {
    const pollIntervalMs = options.pollIntervalMs ?? 1000;
    const timeoutMs = options.timeoutMs ?? 300_000;
    const throwOnFailure = options.throwOnFailure ?? true;
    const sleep = options.sleep ?? realSleep;
    const now = options.now ?? Date.now;

    const deadline = now() + timeoutMs;
    for (;;) {
      const current = await this.status();
      const state = current.status;
      if ((TERMINAL_INDEX_STATES as readonly string[]).includes(state)) {
        if (state === "failed" && throwOnFailure) {
          throw new IndexBuildFailedError({
            status: 200,
            code: "index_build_failed",
            message: `index build for collection ${this.collection} failed`,
            body: current,
          });
        }
        return current;
      }
      if (now() >= deadline) {
        throw new OperationTimeoutError(
          `index build for collection ${this.collection} did not settle within ${timeoutMs}ms`,
          this.collection,
          state,
        );
      }
      await sleep(pollIntervalMs);
    }
  }
}

// ── graph ────────────────────────────────────────────────────────────────────

export class Graph {
  constructor(
    private readonly t: Transport,
    private readonly collection: string,
  ) {}

  /** `POST /v1/graph/node`. */
  createNode(kind: number, recordId?: number): Promise<CreateNodeResponse> {
    const body = omitUndefined({
      kind,
      record_id: recordId,
      collection: this.collection,
    }) satisfies Parameters<Transport["api"]["v1"]["createGraphNode"]>[0];
    return this.t.call(() => this.t.api.v1.createGraphNode(body, this.t.params()));
  }

  /** `POST /v1/graph/edge`. The wire field is `from`, spelled out here. */
  createEdge(fromNode: number, toNode: number, kind: number): Promise<CreateEdgeResponse> {
    return this.t.call(() =>
      this.t.api.v1.createGraphEdge(
        { from: fromNode, to: toNode, kind, collection: this.collection },
        this.t.params(),
      ),
    );
  }

  /** `GET /v1/graph/node/{id}`. */
  getNode(nodeId: number): Promise<GetNodeResponse> {
    return this.t.call(() =>
      this.t.api.v1.getGraphNode(nodeId, { collection: this.collection }, this.t.params()),
    );
  }

  /** `DELETE /v1/graph/node/{id}`. */
  deleteNode(nodeId: number): Promise<unknown> {
    return this.t.call(() =>
      this.t.api.v1.deleteGraphNode(nodeId, { collection: this.collection }, this.t.params()),
    );
  }

  /** `GET /v1/graph/nodes`. */
  listNodes(
    query: { kind?: number; offset?: number; limit?: number } = {},
  ): Promise<ListNodesResponse> {
    return this.t.call(() =>
      this.t.api.v1.listGraphNodes(
        omitUndefined({ collection: this.collection, ...query }),
        this.t.params(),
      ),
    );
  }

  /**
   * `GET /v1/graph/nodes`, walked page by page (§22).
   *
   * `/v1/graph/nodes` is the only offset/limit endpoint in the contract, so it
   * is the only one that gets an iterator — nothing here invents pagination the
   * server does not have. Stops on a short page, which is what the handler
   * returns once the scan runs past the end.
   */
  async *listAllNodes(
    query: { kind?: number; pageSize?: number; startOffset?: number } = {},
  ): AsyncGenerator<ListNodesResponse["nodes"][number], void, undefined> {
    const limit = query.pageSize ?? 100;
    let offset = query.startOffset ?? 0;
    for (;;) {
      const page = await this.listNodes({ kind: query.kind, offset, limit });
      const nodes = page.nodes ?? [];
      for (const node of nodes) yield node;
      if (nodes.length < limit) return;
      offset += nodes.length;
    }
  }

  /** `GET /v1/graph/edges/{id}`. */
  listEdges(nodeId: number): Promise<V1Data<"listNodeEdges">> {
    return this.t.call(() =>
      this.t.api.v1.listNodeEdges(nodeId, { collection: this.collection }, this.t.params()),
    );
  }

  /** `GET /v1/graph/subgraph`. */
  subgraph(root: number, depth?: number): Promise<SubgraphResponse> {
    return this.t.call(() =>
      this.t.api.v1.getSubgraph(
        omitUndefined({ root, depth, collection: this.collection }),
        this.t.params(),
      ),
    );
  }

  /** `GET /v1/graph/query`. */
  query(
    start: number,
    options: {
      direction?: string;
      edgeKind?: number;
      nodeKind?: number;
      depth?: number;
      limit?: number;
    } = {},
  ): Promise<GraphQueryResponse> {
    return this.t.call(() =>
      this.t.api.v1.graphQuery(
        omitUndefined({
          start,
          direction: options.direction,
          edge_kind: options.edgeKind,
          node_kind: options.nodeKind,
          depth: options.depth,
          limit: options.limit,
          collection: this.collection,
        }),
        this.t.params(),
      ),
    );
  }
}

// ── memory ───────────────────────────────────────────────────────────────────

export interface MemorySearchOptions {
  explain?: boolean;
  queryText?: string;
  rerank?: boolean;
  decayHalfLifeSecs?: number;
  metadataFilter?: Record<string, unknown>;
  consistency?: string;
}

export class Memory {
  constructor(
    private readonly t: Transport,
    private readonly collection: string,
  ) {}

  #upsertBody(
    vector: number[],
    options: {
      metadata?: Record<string, unknown>;
      tags?: string[];
      attachToDocumentNode?: number;
    },
  ): MemoryUpsertVectorRequest {
    return omitUndefined({
      vector,
      collection: this.collection,
      metadata: options.metadata,
      tags: options.tags,
      attach_to_document_node: options.attachToDocumentNode,
    }) satisfies MemoryUpsertVectorRequest;
  }

  #searchBody(vector: number[], k: number, o: MemorySearchOptions): MemorySearchVectorRequest {
    return omitUndefined({
      query_vector: vector,
      k,
      collection: this.collection,
      query_text: o.queryText,
      rerank: o.rerank,
      decay_half_life_secs: o.decayHalfLifeSecs,
      metadata_filter: o.metadataFilter,
      consistency: o.consistency,
    }) satisfies MemorySearchVectorRequest;
  }

  /** `POST /v1/memory/upsert`. */
  upsert(
    vector: number[],
    options: { metadata?: Record<string, unknown>; tags?: string[]; attachToDocumentNode?: number } = {},
  ): Promise<V1Data<"memoryUpsert">> {
    return this.t.call(() =>
      this.t.api.v1.memoryUpsert(this.#upsertBody(vector, options), this.t.params()),
    );
  }

  /** `POST /v1/memory/upsert_vector`. A distinct path, not an alias. */
  upsertVector(
    vector: number[],
    options: { metadata?: Record<string, unknown>; tags?: string[]; attachToDocumentNode?: number } = {},
  ): Promise<V1Data<"memoryUpsertVector">> {
    return this.t.call(() =>
      this.t.api.v1.memoryUpsertVector(this.#upsertBody(vector, options), this.t.params()),
    );
  }

  /** `POST /v1/memory/search`. */
  search(
    vector: number[],
    k: number,
    options: MemorySearchOptions = {},
  ): Promise<V1Data<"memorySearch">> {
    return this.t.call(() =>
      this.t.api.v1.memorySearch(
        this.#searchBody(vector, k, options),
        omitUndefined({ explain: options.explain }),
        this.t.params(),
      ),
    );
  }

  /** `POST /v1/memory/search_vector`. A distinct path, not an alias. */
  searchVector(
    vector: number[],
    k: number,
    options: MemorySearchOptions = {},
  ): Promise<V1Data<"memorySearchVector">> {
    return this.t.call(() =>
      this.t.api.v1.memorySearchVector(
        this.#searchBody(vector, k, options),
        omitUndefined({ explain: options.explain }),
        this.t.params(),
      ),
    );
  }

  /** `POST /v1/memory/consolidate`. */
  consolidate(
    oldRecordId: number,
    newVector: number[],
    metadata?: Record<string, unknown>,
  ): Promise<MemoryConsolidateResponse> {
    const body = omitUndefined({
      old_record_id: oldRecordId,
      new_vector: newVector,
      collection: this.collection,
      metadata,
    }) satisfies MemoryConsolidateRequest;
    return this.t.call(() => this.t.api.v1.memoryConsolidate(body, this.t.params()));
  }

  /** `POST /v1/memory/contradict`. */
  contradict(
    recordA: number,
    recordB: number,
    threshold?: number,
  ): Promise<MemoryContradictResponse> {
    const body = omitUndefined({
      record_a: recordA,
      record_b: recordB,
      threshold,
      collection: this.collection,
    }) satisfies MemoryContradictRequest;
    return this.t.call(() => this.t.api.v1.memoryContradict(body, this.t.params()));
  }

  /** `GET /v1/memory/meta/get`. */
  getMetadata(targetId: string): Promise<MetadataGetResponse> {
    return this.t.call(() =>
      this.t.api.v1.getMetadataSidecar({ target_id: targetId }, this.t.params()),
    );
  }

  /** `POST /v1/memory/meta/set`. */
  setMetadata(
    targetId: string,
    metadata: Record<string, unknown>,
  ): Promise<MetadataSetResponse> {
    return this.t.call(() =>
      this.t.api.v1.setMetadataSidecar(
        { target_id: targetId, metadata },
        this.t.params(),
      ),
    );
  }
}

// ── the collection handle ────────────────────────────────────────────────────

export interface SearchOptions {
  queryText?: string;
  rerank?: boolean;
  decayHalfLifeSecs?: number;
  metadataFilter?: Record<string, unknown>;
  graphRerank?: Record<string, unknown>;
  asOf?: string;
  asOfLogIndex?: number;
}

export interface GraphRagOptions {
  k?: number;
  depth?: number;
  retrievalK?: number;
  finalK?: number;
  graphWeight?: number;
  maxNodes?: number;
  maxEdges?: number;
  maxGraphCandidates?: number;
}

export class Collection {
  readonly records: Records;
  readonly index: CollectionIndexResource;
  readonly graph: Graph;
  readonly memory: Memory;

  constructor(
    private readonly t: Transport,
    readonly name: string,
  ) {
    this.records = new Records(t, name);
    this.index = new CollectionIndexResource(t, name);
    this.graph = new Graph(t, name);
    this.memory = new Memory(t, name);
  }

  /** `POST /v1/search`. */
  search(
    query: number[],
    k: number,
    options: SearchOptions = {},
    call: CallOptions = {},
  ): Promise<SearchResponse> {
    const body = omitUndefined({
      query,
      k,
      collection: this.name,
      query_text: options.queryText,
      rerank: options.rerank,
      decay_half_life_secs: options.decayHalfLifeSecs,
      metadata_filter: options.metadataFilter,
      graph_rerank: options.graphRerank,
      as_of: options.asOf,
      as_of_log_index: options.asOfLogIndex,
    }) satisfies SearchRequest;
    return this.t.call(() => this.t.api.v1.search(body, this.t.params(call)));
  }

  /** `POST /v1/graphrag` — vector hits plus the connected subgraph. */
  graphrag(
    queryVector: number[],
    options: GraphRagOptions = {},
    call: CallOptions = {},
  ): Promise<GraphRagResponse> {
    const body = omitUndefined({
      query_vector: queryVector,
      collection: this.name,
      k: options.k,
      depth: options.depth,
      retrieval_k: options.retrievalK,
      final_k: options.finalK,
      graph_weight: options.graphWeight,
      max_nodes: options.maxNodes,
      max_edges: options.maxEdges,
      max_graph_candidates: options.maxGraphCandidates,
    }) satisfies GraphRagRequest;
    return this.t.call(() => this.t.api.v1.graphrag(body, this.t.params(call)));
  }
}

// ── collections ──────────────────────────────────────────────────────────────

export class Collections {
  constructor(private readonly t: Transport) {}

  /** `POST /v1/namespaces`. `dimension` and `metric` are required by the contract. */
  async create(
    name: string,
    options: { dimension: number; metric: MetricValue; index?: IndexKindValue },
  ): Promise<Collection> {
    const body = omitUndefined({
      name,
      dimension: options.dimension,
      metric: asEnum(MetricInput, options.metric, "metric") ?? null,
      index: asEnum(IndexKindInput, options.index, "index"),
    }) satisfies CreateCollectionRequest;
    try {
      await this.t.call<CreateCollectionResponse>(() =>
        this.t.api.v1.createCollection(body, this.t.params()),
      );
    } catch (thrown) {
      // See CollectionAlreadyExistsError: the node reports this as a plain
      // `conflict`, and this is the one call site where it can only mean one thing.
      if (thrown instanceof ConflictError) {
        throw new CollectionAlreadyExistsError({
          status: thrown.status,
          code: thrown.code,
          message: thrown.message,
          requestId: thrown.requestId,
          body: thrown.body,
          headers: thrown.headers,
        });
      }
      throw thrown;
    }
    return new Collection(this.t, name);
  }

  /** `GET /v1/namespaces`. */
  list(): Promise<ListCollectionsResponse> {
    return this.t.call(() => this.t.api.v1.listCollections(this.t.params()));
  }

  /** Just the names. */
  async names(): Promise<string[]> {
    const listed = await this.list();
    return (listed.collections ?? []).map((entry) => entry.name);
  }

  /** A checked handle — costs one list, because the contract has no single read. */
  async get(name: string): Promise<Collection> {
    if (!(await this.names()).includes(name)) {
      throw new CollectionNotFoundError({
        status: 404,
        code: "collection_not_found",
        message: `collection ${name} does not exist on ${this.t.endpoint}`,
      });
    }
    return new Collection(this.t, name);
  }

  /** `DELETE /v1/namespaces/{name}`. */
  delete(name: string): Promise<unknown> {
    return this.t.call(() => this.t.api.v1.deleteCollection(name, this.t.params()));
  }

  /**
   * `POST /v1/search/multi` — fan a query across several collections.
   *
   * Lives here rather than on a Collection because it is deliberately not
   * scoped to one.
   */
  searchMulti(
    query: number[],
    k: number,
    collections: string[],
    options: { decayHalfLifeSecs?: number; metadataFilter?: Record<string, unknown> } = {},
  ): Promise<MultiSearchResponse> {
    const body = omitUndefined({
      query,
      k,
      collections,
      decay_half_life_secs: options.decayHalfLifeSecs,
      metadata_filter: options.metadataFilter,
    }) satisfies Parameters<Transport["api"]["v1"]["searchMulti"]>[0];
    return this.t.call(() => this.t.api.v1.searchMulti(body, this.t.params()));
  }
}

export type { ApiError };
