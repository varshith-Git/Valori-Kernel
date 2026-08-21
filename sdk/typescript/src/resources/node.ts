// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Node-scoped resources: meta, ingest, tree, community, proof, snapshots,
// storage, cluster, crypto and the node-wide index config.
//
// These are not scoped to a collection (or take the collection inside their
// body), so they hang off `ValoriClient` directly.

import type {
  IndexKindInput,
  TreeIndex,
  TreeReceipt,
} from "../../generated/valori-api.js";
import type { Transport, V1Data, HealthData } from "../transport.js";

const omitUndefined = <T extends object>(value: T): T =>
  Object.fromEntries(Object.entries(value).filter(([, v]) => v !== undefined)) as T;

export class Meta {
  constructor(private readonly t: Transport) {}

  /** `GET /health` — the one unauthenticated operation in the contract. */
  health(): Promise<HealthData<"getHealth">> {
    return this.t.call(() => this.t.api.health.getHealth(this.t.params()));
  }
  /** `GET /v1/version`. */
  version(): Promise<V1Data<"getVersion">> {
    return this.t.call(() => this.t.api.v1.getVersion(this.t.params()));
  }
  /** `GET /v1/usage`. */
  usage(): Promise<V1Data<"getUsage">> {
    return this.t.call(() => this.t.api.v1.getUsage(this.t.params()));
  }
  /** `GET /v1/models/health`. */
  modelsHealth(): Promise<V1Data<"getModelsHealth">> {
    return this.t.call(() => this.t.api.v1.getModelsHealth(this.t.params()));
  }
  /** `GET /v1/shard/routing`. */
  shardRouting(): Promise<V1Data<"getShardRouting">> {
    return this.t.call(() => this.t.api.v1.getShardRouting(this.t.params()));
  }
}

/** Node-wide index configuration and rebuilds. */
export class IndexConfig {
  constructor(private readonly t: Transport) {}

  /** `GET /v1/index/config`. */
  config(): Promise<V1Data<"getIndexConfig">> {
    return this.t.call(() => this.t.api.v1.getIndexConfig(this.t.params()));
  }
  /** `POST /v1/index/rebuild`. */
  rebuild(index?: IndexKindInput): Promise<V1Data<"rebuildIndexes">> {
    return this.t.call(() =>
      this.t.api.v1.rebuildIndexes(omitUndefined({ index }), this.t.params()),
    );
  }
}

export interface ChunkOptions {
  collection?: string;
  source?: string;
  strategy?: string;
  chunkSize?: number;
  chunkOverlap?: number;
}

export class Ingest {
  constructor(private readonly t: Transport) {}

  #body(text: string, o: ChunkOptions, extra: Record<string, unknown> = {}) {
    return omitUndefined({
      text,
      collection: o.collection,
      source: o.source,
      strategy: o.strategy,
      chunk_size: o.chunkSize,
      chunk_overlap: o.chunkOverlap,
      ...extra,
    });
  }

  /** `POST /v1/ingest/document` — chunk without embedding or storing. */
  chunk(text: string, options: ChunkOptions = {}): Promise<V1Data<"chunkDocument">> {
    return this.t.call(() =>
      this.t.api.v1.chunkDocument(this.#body(text, options) as never, this.t.params()),
    );
  }

  /**
   * `POST /v1/ingest` — full chunk + embed + insert.
   *
   * `background: true` maps to the `async` query flag; the node then answers
   * 202 with a job id you poll with `status()`.
   */
  document(
    text: string,
    options: ChunkOptions & { background?: boolean } = {},
  ): Promise<V1Data<"ingestDocument">> {
    return this.t.call(() =>
      this.t.api.v1.ingestDocument(
        this.#body(text, options) as never,
        omitUndefined({ async: options.background }),
        this.t.params(),
      ),
    );
  }

  /** `POST /v1/ingest/update` — diff-based document update. */
  update(
    documentNodeId: number,
    text: string,
    options: ChunkOptions = {},
  ): Promise<V1Data<"updateIngestedDocument">> {
    return this.t.call(() =>
      this.t.api.v1.updateIngestedDocument(
        this.#body(text, options, { document_node_id: documentNodeId }) as never,
        this.t.params(),
      ),
    );
  }

  /** `GET /v1/ingest/status/{job_id}`. */
  status(jobId: string): Promise<V1Data<"getIngestStatus">> {
    return this.t.call(() => this.t.api.v1.getIngestStatus(jobId, this.t.params()));
  }

  /**
   * `POST /v1/ingest/extract-entities`.
   *
   * Tagged `community` in the contract but served under `/v1/ingest`, so it is
   * wrapped here, where a user will look for it.
   */
  extractEntities(
    text: string,
    options: { namespace?: string; entityTypes?: string[]; model?: string } = {},
  ): Promise<V1Data<"extractEntities">> {
    return this.t.call(() =>
      this.t.api.v1.extractEntities(
        omitUndefined({
          text,
          namespace: options.namespace,
          entity_types: options.entityTypes,
          model: options.model,
        }) as never,
        this.t.params(),
      ),
    );
  }
}

export class Tree {
  constructor(private readonly t: Transport) {}

  /** `POST /v1/tree/build`. Returns a `cache_key` to reuse. */
  build(text: string, docName?: string): Promise<V1Data<"treeBuild">> {
    return this.t.call(() =>
      this.t.api.v1.treeBuild(omitUndefined({ text, doc_name: docName }) as never, this.t.params()),
    );
  }

  /** `POST /v1/tree/query`. */
  query(
    query: string,
    options: { tree?: TreeIndex; cacheKey?: string; k?: number; prevHash?: string } = {},
  ): Promise<V1Data<"treeQuery">> {
    return this.t.call(() =>
      this.t.api.v1.treeQuery(
        omitUndefined({
          query,
          tree: options.tree,
          cache_key: options.cacheKey,
          k: options.k,
          prev_hash: options.prevHash,
        }) as never,
        this.t.params(),
      ),
    );
  }

  /** `POST /v1/tree/hybrid`. */
  hybrid(
    query: string,
    options: {
      text?: string;
      tree?: TreeIndex;
      cacheKey?: string;
      namespace?: string;
      k?: number;
      treeWeight?: number;
      prevHash?: string;
      docName?: string;
    } = {},
  ): Promise<V1Data<"treeHybrid">> {
    return this.t.call(() =>
      this.t.api.v1.treeHybrid(
        omitUndefined({
          query,
          text: options.text,
          tree: options.tree,
          cache_key: options.cacheKey,
          namespace: options.namespace,
          k: options.k,
          tree_weight: options.treeWeight,
          prev_hash: options.prevHash,
          doc_name: options.docName,
        }) as never,
        this.t.params(),
      ),
    );
  }

  /** `POST /v1/tree/verify` — stateless receipt verification. */
  verify(tree: TreeIndex, receipt: TreeReceipt): Promise<V1Data<"treeVerify">> {
    return this.t.call(() => this.t.api.v1.treeVerify({ tree, receipt }, this.t.params()));
  }

  /** `POST /v1/tree/chain-verify` — verify a whole receipt chain. */
  chainVerify(receipts: TreeReceipt[]): Promise<V1Data<"treeChainVerify">> {
    return this.t.call(() => this.t.api.v1.treeChainVerify({ receipts }, this.t.params()));
  }
}

export class Community {
  constructor(private readonly t: Transport) {}

  /** `POST /v1/community/detect`. Run before search or overview. */
  detect(options: { namespace?: string; maxIter?: number } = {}): Promise<V1Data<"communityDetect">> {
    return this.t.call(() =>
      this.t.api.v1.communityDetect(
        omitUndefined({ namespace: options.namespace, max_iter: options.maxIter }),
        this.t.params(),
      ),
    );
  }

  /** `POST /v1/community/search`. */
  search(
    vector: number[],
    options: { k?: number; namespace?: string; depth?: number; drillIn?: boolean } = {},
  ): Promise<V1Data<"communitySearch">> {
    return this.t.call(() =>
      this.t.api.v1.communitySearch(
        omitUndefined({
          vector,
          k: options.k,
          namespace: options.namespace,
          depth: options.depth,
          drill_in: options.drillIn,
        }) as never,
        this.t.params(),
      ),
    );
  }

  /** `GET /v1/community/overview`. */
  overview(): Promise<V1Data<"communityOverview">> {
    return this.t.call(() => this.t.api.v1.communityOverview(this.t.params()));
  }
}

export class Proof {
  constructor(private readonly t: Transport) {}

  /** `GET /v1/proof/event-log` — the receipt primitive. */
  eventLog(): Promise<V1Data<"getEventLogProof">> {
    return this.t.call(() => this.t.api.v1.getEventLogProof(this.t.params()));
  }
  /** `GET /v1/proof/state`. */
  state(): Promise<V1Data<"getStateProof">> {
    return this.t.call(() => this.t.api.v1.getStateProof(this.t.params()));
  }
  /** `GET /v1/proof/receipt/{id}`. */
  receipt(receiptId: string): Promise<V1Data<"getReceipt">> {
    return this.t.call(() => this.t.api.v1.getReceipt(receiptId, this.t.params()));
  }
  /** `GET /v1/proof/receipt`. */
  latestReceipt(): Promise<V1Data<"getLatestReceipt">> {
    return this.t.call(() => this.t.api.v1.getLatestReceipt(this.t.params()));
  }
  /** `GET /v1/timeline`. */
  timeline(
    options: { from?: string; to?: string; limit?: number; collection?: string } = {},
  ): Promise<V1Data<"getTimeline">> {
    return this.t.call(() =>
      this.t.api.v1.getTimeline(
        omitUndefined({
          from: options.from,
          to: options.to,
          limit: options.limit,
          collection: options.collection,
        }),
        this.t.params(),
      ),
    );
  }
}

export class Snapshots {
  constructor(private readonly t: Transport) {}

  /** `POST /v1/snapshot/save`. */
  save(path?: string): Promise<V1Data<"saveSnapshot">> {
    return this.t.call(() => this.t.api.v1.saveSnapshot(omitUndefined({ path }), this.t.params()));
  }
  /** `POST /v1/snapshot/restore`. */
  restore(path: string): Promise<V1Data<"restoreSnapshot">> {
    return this.t.call(() => this.t.api.v1.restoreSnapshot({ path }, this.t.params()));
  }
  /** `GET /v1/snapshot/download`. */
  download(): Promise<V1Data<"downloadSnapshot">> {
    return this.t.call(() => this.t.api.v1.downloadSnapshot(this.t.params()));
  }
  /** `POST /v1/snapshot/upload`. */
  upload(bytes: Blob | ArrayBuffer | Uint8Array): Promise<V1Data<"uploadSnapshot">> {
    return this.t.call(() => this.t.api.v1.uploadSnapshot(bytes as never, this.t.params()));
  }
}

export class Storage {
  constructor(private readonly t: Transport) {}

  /** `POST /v1/storage/snapshots/upload`. */
  uploadSnapshot(): Promise<V1Data<"uploadSnapshotToObjectStore">> {
    return this.t.call(() => this.t.api.v1.uploadSnapshotToObjectStore(this.t.params()));
  }
  /** `POST /v1/storage/snapshots/restore`. Omit `key` to resolve via the manifest. */
  restoreSnapshot(key?: string): Promise<V1Data<"restoreSnapshotFromObjectStore">> {
    return this.t.call(() =>
      this.t.api.v1.restoreSnapshotFromObjectStore(omitUndefined({ key }), this.t.params()),
    );
  }
  /** `GET /v1/storage/snapshots`. */
  listSnapshots(): Promise<V1Data<"listObjectStoreSnapshots">> {
    return this.t.call(() => this.t.api.v1.listObjectStoreSnapshots(this.t.params()));
  }
  /** `GET /v1/storage/manifest` — the disaster-recovery entry point. */
  manifest(): Promise<V1Data<"getStorageManifest">> {
    return this.t.call(() => this.t.api.v1.getStorageManifest(this.t.params()));
  }
  /** `POST /v1/storage/wal/archive`. */
  archiveWal(path: string): Promise<V1Data<"archiveWalSegment">> {
    return this.t.call(() => this.t.api.v1.archiveWalSegment({ path }, this.t.params()));
  }
  /** `GET /v1/storage/wal`. */
  listWalSegments(): Promise<V1Data<"listArchivedWalSegments">> {
    return this.t.call(() => this.t.api.v1.listArchivedWalSegments(this.t.params()));
  }
}

export class Cluster {
  constructor(private readonly t: Transport) {}

  /** `GET /v1/cluster/status`. */
  status(): Promise<V1Data<"getClusterStatus">> {
    return this.t.call(() => this.t.api.v1.getClusterStatus(this.t.params()));
  }
  /** `GET /v1/cluster/health`. */
  health(): Promise<V1Data<"getClusterHealth">> {
    return this.t.call(() => this.t.api.v1.getClusterHealth(this.t.params()));
  }
  /** `GET /v1/cluster/role`. */
  role(): Promise<V1Data<"getClusterRole">> {
    return this.t.call(() => this.t.api.v1.getClusterRole(this.t.params()));
  }
  /** `GET /v1/cluster/proof` — the cluster analog of `proof.state()`. */
  proof(): Promise<V1Data<"getClusterProof">> {
    return this.t.call(() => this.t.api.v1.getClusterProof(this.t.params()));
  }
}

export class Crypto {
  constructor(private readonly t: Transport) {}

  /** `GET /v1/crypto/status/{key_id}`. */
  keyStatus(keyId: string): Promise<V1Data<"getKeyStatus">> {
    return this.t.call(() => this.t.api.v1.getKeyStatus(keyId, this.t.params()));
  }
}
