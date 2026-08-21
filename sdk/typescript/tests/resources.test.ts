// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Handwritten wrapper tests — Phase API-4A §17/§18.
//
// Each case drives a wrapper end to end and asserts on what actually reached
// the wire: method, path, query string and JSON body. A wrapper is only correct
// if it hits the operation the coverage manifest claims it hits.

import { describe, expect, it } from "vitest";

import { CollectionNotFoundError, ValoriClient } from "../src/index.js";
import {
  COLLECTIONS_OK,
  HEALTH_OK,
  INSERT_OK,
  SEARCH_OK,
  json,
  makeClient,
  noContent,
  type Recorder,
} from "./helpers.js";

async function call(
  invoke: (client: ValoriClient) => Promise<unknown>,
  handler = noContent,
): Promise<Recorder> {
  const { client, recorder } = makeClient(handler);
  await invoke(client);
  return recorder;
}

const TREE_INDEX = { doc_name: "doc.md", roots: [], nodes: [] } as never;
const TREE_RECEIPT = {
  query: "q",
  query_hash: "00",
  visited_node_ids: [],
  fetched_ranges: [],
  evidence_hash: "00",
  answer_hash: "00",
  prev_hash: "00",
  receipt_hash: "00",
  hash_algo: "blake3",
  timestamp: 0,
} as never;

describe("collections", () => {
  it("posts the required triple on create", async () => {
    const rec = await call((c) => c.collections.create("docs", { dimension: 384, metric: "squared_l2" }));
    expect(rec.last.method).toBe("POST");
    expect(rec.last.url.pathname).toBe("/v1/namespaces");
    expect(rec.last.body).toEqual({ name: "docs", dimension: 384, metric: "squared_l2" });
  });

  it("omits an absent optional rather than sending null", async () => {
    const rec = await call((c) => c.collections.create("d", { dimension: 3, metric: "squared_l2" }));
    expect(rec.last.body).not.toHaveProperty("index");
  });

  it("returns a usable handle from create", async () => {
    const { client } = makeClient(noContent);
    const handle = await client.collections.create("docs", { dimension: 3, metric: "squared_l2" });
    expect(handle.name).toBe("docs");
    expect(handle.records).toBeDefined();
  });

  it("lists and deletes", async () => {
    const listed = await call((c) => c.collections.list(), json(COLLECTIONS_OK));
    expect(listed.last.method).toBe("GET");
    expect(listed.last.url.pathname).toBe("/v1/namespaces");

    const deleted = await call((c) => c.collections.delete("docs"));
    expect(deleted.last.method).toBe("DELETE");
    expect(deleted.last.url.pathname).toBe("/v1/namespaces/docs");
  });

  it("gives an unchecked handle with no round trip", async () => {
    const { client, recorder } = makeClient(noContent);
    expect(client.collection("docs").name).toBe("docs");
    expect(recorder.count).toBe(0);
  });

  it("verifies existence in get() and raises when absent", async () => {
    const { client } = makeClient(json(COLLECTIONS_OK));
    await expect(client.collections.get("docs")).resolves.toMatchObject({ name: "docs" });
    await expect(client.collections.get("ghost")).rejects.toBeInstanceOf(CollectionNotFoundError);
  });
});

describe("records", () => {
  it("carries the collection from the handle", async () => {
    const rec = await call(
      (c) => c.collection("docs").records.insert([0.1, 0.2], { metadata: { a: 1 } }),
      json(INSERT_OK),
    );
    expect(rec.last.url.pathname).toBe("/v1/records");
    // `InsertRecordRequest.metadata` is `number[]` on the wire — opaque UTF-8
    // JSON bytes, committed inside the audit-chained event — not a JSON map.
    // The ergonomic layer takes the object and does the encoding (§24).
    expect(rec.last.body).toMatchObject({
      collection: "docs",
      values: [0.1, 0.2],
      metadata: [...new TextEncoder().encode('{"a":1}')],
    });
  });

  it("encodes batch metadata as UTF-8 JSON strings, not maps", async () => {
    const rec = await call((c) =>
      c.collection("docs").records.insertBatch([[0.1], [0.2]], {
        metadata: [{ a: 1 }, null],
      }),
    );
    // `BatchInsertRequest.metadata` is `(string | null)[]` — a different wire
    // shape from the single-insert path, for the same domain-level input.
    expect(rec.last.body).toMatchObject({ metadata: ['{"a":1}', null] });
  });

  it("sends requestId in the body and as the idempotency header", async () => {
    const rec = await call(
      (c) => c.collection("docs").records.insert([0.1], { requestId: "r-1" }),
      json(INSERT_OK),
    );
    expect(rec.last.body).toMatchObject({ request_id: "r-1" });
    expect(rec.last.headers.get("Idempotency-Key")).toBe("r-1");
  });

  it("hits the batch path", async () => {
    const rec = await call((c) =>
      c.collection("docs").records.insertBatch([[0.1], [0.2]], { texts: ["a", "b"] }),
    );
    expect(rec.last.url.pathname).toBe("/v1/vectors/batch-insert");
    expect(rec.last.body).toMatchObject({ batch: [[0.1], [0.2]], texts: ["a", "b"] });
  });

  it("hits the encrypted path", async () => {
    const rec = await call((c) =>
      c.collection("docs").records.insertEncrypted("cipher", { keyId: "k1" }),
    );
    expect(rec.last.url.pathname).toBe("/v1/records/encrypted");
    expect(rec.last.body).toEqual({ payload: "cipher", collection: "docs", key_id: "k1" });
  });

  it("puts the collection in the query string on read", async () => {
    const rec = await call((c) => c.collection("docs").records.get(42));
    expect(rec.last.method).toBe("GET");
    expect(rec.last.url.pathname).toBe("/v1/records/42");
    expect(rec.query).toEqual({ collection: "docs" });
  });

  it("keeps delete and soft-delete on distinct paths", async () => {
    expect((await call((c) => c.collection("d").records.delete(1))).last.url.pathname).toBe("/v1/delete");
    expect((await call((c) => c.collection("d").records.softDelete(1))).last.url.pathname).toBe(
      "/v1/soft-delete",
    );
  });

  it("patches metadata with a free-form body", async () => {
    const rec = await call((c) => c.collection("docs").records.updateMetadata(9, { tier: { x: 1 } }));
    expect(rec.last.method).toBe("PATCH");
    expect(rec.last.url.pathname).toBe("/v1/records/9/metadata");
    expect(rec.last.body).toEqual({ tier: { x: 1 } });
  });
});

describe("search and graphrag", () => {
  it("sends query, k and collection", async () => {
    const rec = await call((c) => c.collection("docs").search([0.1, 0.2], 5), json(SEARCH_OK));
    expect(rec.last.url.pathname).toBe("/v1/search");
    expect(rec.last.body).toEqual({ query: [0.1, 0.2], k: 5, collection: "docs" });
  });

  it("passes the optional ranking knobs", async () => {
    const rec = await call(
      (c) =>
        c.collection("docs").search([0.1], 1, {
          queryText: "optimizer",
          rerank: true,
          decayHalfLifeSecs: 86400,
          metadataFilter: { year: { gte: 2020 } },
          graphRerank: { weight: 0.15 },
        }),
      json(SEARCH_OK),
    );
    expect(rec.last.body).toMatchObject({
      query_text: "optimizer",
      rerank: true,
      decay_half_life_secs: 86400,
      metadata_filter: { year: { gte: 2020 } },
      graph_rerank: { weight: 0.15 },
    });
  });

  it("keeps multi-search off the Collection handle", async () => {
    const rec = await call((c) => c.collections.searchMulti([0.1], 3, ["a", "b"]));
    expect(rec.last.url.pathname).toBe("/v1/search/multi");
    expect(rec.last.body).toMatchObject({ collections: ["a", "b"] });
  });

  it("uses query_vector for graphrag, not query", async () => {
    const rec = await call((c) => c.collection("docs").graphrag([0.1], { k: 5, depth: 2 }));
    expect(rec.last.url.pathname).toBe("/v1/graphrag");
    expect(rec.last.body).toEqual({ query_vector: [0.1], collection: "docs", k: 5, depth: 2 });
  });
});

describe("index", () => {
  it("posts to the collection index path", async () => {
    const rec = await call((c) => c.collection("docs").index.build("hnsw", { m: 16 }));
    expect(rec.last.method).toBe("POST");
    expect(rec.last.url.pathname).toBe("/v1/namespaces/docs/index");
    expect(rec.last.body).toEqual({ type: "hnsw", parameters: { m: 16 } });
  });

  it("reads status from the same path", async () => {
    const rec = await call((c) => c.collection("docs").index.status());
    expect(rec.last.method).toBe("GET");
    expect(rec.last.url.pathname).toBe("/v1/namespaces/docs/index");
  });

  it("exposes the node-wide config and rebuild", async () => {
    expect((await call((c) => c.index.config())).last.url.pathname).toBe("/v1/index/config");
    const rec = await call((c) => c.index.rebuild("hnsw" as never));
    expect(rec.last.url.pathname).toBe("/v1/index/rebuild");
    expect(rec.last.body).toEqual({ index: "hnsw" });
  });
});

describe("graph", () => {
  it("translates fromNode to the `from` wire field", async () => {
    const rec = await call((c) => c.collection("docs").graph.createEdge(1, 2, 7));
    expect(rec.last.url.pathname).toBe("/v1/graph/edge");
    expect(rec.last.body).toEqual({ from: 1, to: 2, kind: 7, collection: "docs" });
  });

  it("creates, reads and deletes nodes", async () => {
    expect((await call((c) => c.collection("d").graph.createNode(3))).last.url.pathname).toBe(
      "/v1/graph/node",
    );
    expect((await call((c) => c.collection("d").graph.getNode(5))).last.url.pathname).toBe(
      "/v1/graph/node/5",
    );
    const deleted = await call((c) => c.collection("d").graph.deleteNode(5));
    expect(deleted.last.method).toBe("DELETE");
  });

  it("builds the traversal query strings", async () => {
    const nodes = await call((c) =>
      c.collection("d").graph.listNodes({ kind: 2, offset: 10, limit: 5 }),
    );
    expect(nodes.last.url.pathname).toBe("/v1/graph/nodes");
    expect(nodes.query).toEqual({ collection: "d", kind: "2", offset: "10", limit: "5" });

    const sub = await call((c) => c.collection("d").graph.subgraph(9, 2));
    expect(sub.query).toEqual({ root: "9", depth: "2", collection: "d" });

    const q = await call((c) => c.collection("d").graph.query(1, { direction: "out", limit: 3 }));
    expect(q.query).toEqual({ start: "1", direction: "out", limit: "3", collection: "d" });

    expect((await call((c) => c.collection("d").graph.listEdges(4))).last.url.pathname).toBe(
      "/v1/graph/edges/4",
    );
  });

  it("omits absent query parameters from the url", async () => {
    const rec = await call((c) => c.collection("d").graph.listNodes());
    expect(rec.query).toEqual({ collection: "d" });
  });
});

describe("memory", () => {
  it("keeps upsert and upsert_vector on distinct paths", async () => {
    expect((await call((c) => c.collection("d").memory.upsert([0.1]))).last.url.pathname).toBe(
      "/v1/memory/upsert",
    );
    expect((await call((c) => c.collection("d").memory.upsertVector([0.1]))).last.url.pathname).toBe(
      "/v1/memory/upsert_vector",
    );
  });

  it("keeps search and search_vector on distinct paths", async () => {
    expect((await call((c) => c.collection("d").memory.search([0.1], 3))).last.url.pathname).toBe(
      "/v1/memory/search",
    );
    expect(
      (await call((c) => c.collection("d").memory.searchVector([0.1], 3))).last.url.pathname,
    ).toBe("/v1/memory/search_vector");
  });

  it("sends explain as a query flag, not a body field", async () => {
    const rec = await call((c) => c.collection("d").memory.search([0.1], 3, { explain: true }));
    expect(rec.query).toEqual({ explain: "true" });
    expect(rec.last.body).not.toHaveProperty("explain");
  });

  it("wraps maintenance and the metadata sidecar", async () => {
    const consolidated = await call((c) => c.collection("d").memory.consolidate(7, [0.2]));
    expect(consolidated.last.body).toEqual({
      old_record_id: 7,
      new_vector: [0.2],
      collection: "d",
    });

    const contradicted = await call((c) => c.collection("d").memory.contradict(3, 9, 0.9));
    expect(contradicted.last.body).toEqual({
      record_a: 3,
      record_b: 9,
      threshold: 0.9,
      collection: "d",
    });

    const got = await call((c) => c.collection("d").memory.getMetadata("mem-1"));
    expect(got.last.url.pathname).toBe("/v1/memory/meta/get");
    expect(got.query).toEqual({ target_id: "mem-1" });

    const set = await call((c) => c.collection("d").memory.setMetadata("mem-1", { k: { v: 1 } }));
    expect(set.last.url.pathname).toBe("/v1/memory/meta/set");
    expect(set.last.body).toMatchObject({ target_id: "mem-1" });
  });
});

describe("node-scoped resources", () => {
  const cases: Array<[string, (c: ValoriClient) => Promise<unknown>, string, string]> = [
    ["health", (c) => c.meta.health(), "GET", "/health"],
    ["version", (c) => c.meta.version(), "GET", "/v1/version"],
    ["usage", (c) => c.meta.usage(), "GET", "/v1/usage"],
    ["models health", (c) => c.meta.modelsHealth(), "GET", "/v1/models/health"],
    ["shard routing", (c) => c.meta.shardRouting(), "GET", "/v1/shard/routing"],
    ["ingest chunk", (c) => c.ingest.chunk("hello"), "POST", "/v1/ingest/document"],
    ["ingest document", (c) => c.ingest.document("hello"), "POST", "/v1/ingest"],
    ["ingest update", (c) => c.ingest.update(1, "hello"), "POST", "/v1/ingest/update"],
    ["ingest status", (c) => c.ingest.status("job-1"), "GET", "/v1/ingest/status/job-1"],
    ["extract entities", (c) => c.ingest.extractEntities("hi"), "POST", "/v1/ingest/extract-entities"],
    ["tree build", (c) => c.tree.build("# doc"), "POST", "/v1/tree/build"],
    ["tree query", (c) => c.tree.query("q"), "POST", "/v1/tree/query"],
    ["tree hybrid", (c) => c.tree.hybrid("q"), "POST", "/v1/tree/hybrid"],
    ["tree verify", (c) => c.tree.verify(TREE_INDEX, TREE_RECEIPT), "POST", "/v1/tree/verify"],
    ["tree chain verify", (c) => c.tree.chainVerify([]), "POST", "/v1/tree/chain-verify"],
    ["community detect", (c) => c.community.detect(), "POST", "/v1/community/detect"],
    ["community search", (c) => c.community.search([0.1]), "POST", "/v1/community/search"],
    ["community overview", (c) => c.community.overview(), "GET", "/v1/community/overview"],
    ["proof event log", (c) => c.proof.eventLog(), "GET", "/v1/proof/event-log"],
    ["proof state", (c) => c.proof.state(), "GET", "/v1/proof/state"],
    ["proof receipt", (c) => c.proof.receipt("r-1"), "GET", "/v1/proof/receipt/r-1"],
    ["latest receipt", (c) => c.proof.latestReceipt(), "GET", "/v1/proof/receipt"],
    ["timeline", (c) => c.proof.timeline({ limit: 10 }), "GET", "/v1/timeline"],
    ["snapshot save", (c) => c.snapshots.save(), "POST", "/v1/snapshot/save"],
    ["snapshot restore", (c) => c.snapshots.restore("/tmp/s"), "POST", "/v1/snapshot/restore"],
    ["snapshot download", (c) => c.snapshots.download(), "GET", "/v1/snapshot/download"],
    ["snapshot upload", (c) => c.snapshots.upload(new Uint8Array([1, 2])), "POST", "/v1/snapshot/upload"],
    ["store upload", (c) => c.storage.uploadSnapshot(), "POST", "/v1/storage/snapshots/upload"],
    ["store restore", (c) => c.storage.restoreSnapshot(), "POST", "/v1/storage/snapshots/restore"],
    ["store list", (c) => c.storage.listSnapshots(), "GET", "/v1/storage/snapshots"],
    ["store manifest", (c) => c.storage.manifest(), "GET", "/v1/storage/manifest"],
    ["wal archive", (c) => c.storage.archiveWal("/tmp/w"), "POST", "/v1/storage/wal/archive"],
    ["wal list", (c) => c.storage.listWalSegments(), "GET", "/v1/storage/wal"],
    ["cluster status", (c) => c.cluster.status(), "GET", "/v1/cluster/status"],
    ["cluster health", (c) => c.cluster.health(), "GET", "/v1/cluster/health"],
    ["cluster role", (c) => c.cluster.role(), "GET", "/v1/cluster/role"],
    ["cluster proof", (c) => c.cluster.proof(), "GET", "/v1/cluster/proof"],
    ["key status", (c) => c.crypto.keyStatus("k-1"), "GET", "/v1/crypto/status/k-1"],
    ["operations list", (c) => c.operations.list(), "GET", "/v1/operations"],
    ["operation execution", (c) => c.operations.execution("op-1"), "GET", "/v1/operations/op-1/execution"],
  ];

  for (const [name, invoke, method, path] of cases) {
    it(`${name} hits ${method} ${path}`, async () => {
      const rec = await call(invoke);
      expect(rec.last.method).toBe(method);
      expect(rec.last.url.pathname).toBe(path);
    });
  }

  it("maps background ingest to the async query flag", async () => {
    const rec = await call((c) => c.ingest.document("hello", { background: true }));
    expect(rec.query).toEqual({ async: "true" });
  });

  it("reaches health without an api key", async () => {
    const { client } = makeClient(json(HEALTH_OK), { apiKey: undefined });
    await expect(client.health()).resolves.toMatchObject({ status: "ok" });
  });

  it("exposes the generated client as an escape hatch", () => {
    const { client } = makeClient(noContent);
    expect(client.raw).toBeDefined();
    expect(client.raw.v1).toBeDefined();
  });
});
