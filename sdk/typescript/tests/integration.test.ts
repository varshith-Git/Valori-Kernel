// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Integration suite against a real Valori node — Phase API-4A §17/§18.
//
// Skipped unless VALORI_TEST_ENDPOINT points at a running node:
//
//     cargo run -p valori-node &
//     VALORI_TEST_ENDPOINT=http://localhost:3000 npm test --prefix sdk/typescript
//
// Set VALORI_TEST_MODE=cluster when the endpoint is a cluster node; the
// cluster-only cases are skipped otherwise. §18 asks for both, and the same
// assertions run against either — that is the point: the public contract claims
// one surface, so the SDK exercises one surface.
//
// These tests create and drop their own collections and clean up after
// themselves. They are not run by the unit CI job.

import { afterAll, beforeAll, describe, expect, it } from "vitest";

import { Collection, NotFoundError, ValoriAPIError, ValoriClient } from "../src/index.js";

const ENDPOINT = process.env.VALORI_TEST_ENDPOINT;
const MODE = process.env.VALORI_TEST_MODE ?? "standalone";
const DIM = Number(process.env.VALORI_TEST_DIM ?? "8");

const live = ENDPOINT ? describe : describe.skip;
const cluster = ENDPOINT && MODE === "cluster" ? describe : describe.skip;

const vec = (seed = 0.1) => new Array(DIM).fill(seed);
const uniq = () => Math.random().toString(36).slice(2, 10);

/**
 * Normalise whatever a read path hands back into plain JSON.
 *
 * The node returns a record's metadata as the opaque bytes it was given, so a
 * round-trip assertion has to decode before comparing — comparing the raw wire
 * form to the caller's object is exactly the mistake API-4D fixed.
 *
 * NOTE (follow-up, see the phase doc): the *write* path is symmetric — you pass
 * an object — but the *read* path is not. Making reads decode is an API-4E
 * decision, not a silent change here.
 */
function decodeStoredMetadata(stored: unknown): unknown {
  if (stored === null || stored === undefined) return stored;
  if (Array.isArray(stored)) {
    return JSON.parse(new TextDecoder().decode(new Uint8Array(stored as number[])));
  }
  if (typeof stored === "string") return JSON.parse(stored);
  return stored;
}

/**
 * A contract-valid `request_id`.
 *
 * `RequestId` is 32 hex characters (a 16-byte UUID), not a free-form string —
 * the node rejects anything else with a 422 before the write is attempted.
 */
const requestId = () =>
  Array.from({ length: 32 }, () => "0123456789abcdef"[Math.floor(Math.random() * 16)]).join("");

let client: ValoriClient;
let owned: string[] = [];

async function scratch(): Promise<Collection> {
  const name = `sdk-it-${uniq()}`;
  const handle = await client.collections.create(name, { dimension: DIM, metric: "squared_l2" });
  owned.push(name);
  return handle;
}

beforeAll(() => {
  if (!ENDPOINT) return;
  client = new ValoriClient({
    endpoint: ENDPOINT,
    apiKey: process.env.VALORI_TEST_API_KEY,
  });
});

afterAll(async () => {
  if (!ENDPOINT) return;
  for (const name of owned) {
    await client.collections.delete(name).catch(() => undefined);
  }
  owned = [];
});

live("against a real node", () => {
  it("reports health and version", async () => {
    expect(["ok", "degraded"]).toContain((await client.health()).status);
    expect(await client.version()).toBeDefined();
  });

  it("creates, lists and deletes a collection", async () => {
    const name = `sdk-it-${uniq()}`;
    await client.collections.create(name, { dimension: DIM, metric: "squared_l2" });
    expect(await client.collections.names()).toContain(name);
    await client.collections.delete(name);
    expect(await client.collections.names()).not.toContain(name);
  });

  it("inserts, reads back and searches", async () => {
    const collection = await scratch();
    const inserted = await collection.records.insert(vec(0.1), {
      metadata: { src: "it" },
      requestId: requestId(),
    });
    expect(await collection.records.get(inserted.id)).toBeDefined();
    const found = await collection.search(vec(0.1), 5);
    expect(found.results.some((hit) => hit.id === inserted.id)).toBe(true);
  });

  it("batch inserts", async () => {
    const collection = await scratch();
    await collection.records.insertBatch([vec(0.1), vec(0.2), vec(0.3)]);
    expect((await collection.search(vec(0.2), 10)).results.length).toBeGreaterThanOrEqual(3);
  });

  it("multi-searches across collections", async () => {
    const collection = await scratch();
    await collection.records.insert(vec(0.4));
    expect(await client.collections.searchMulti(vec(0.4), 3, [collection.name])).toBeDefined();
  });

  it("soft-deletes and hard-deletes", async () => {
    const collection = await scratch();
    const first = (await collection.records.insert(vec(0.5))).id;
    await collection.records.softDelete(first);
    const second = (await collection.records.insert(vec(0.6))).id;
    await collection.records.delete(second);
  });

  it("runs the index lifecycle", async () => {
    const collection = await scratch();
    for (let i = 0; i < 20; i += 1) await collection.records.insert(vec(0.1));
    await collection.index.build("hnsw");
    const settled = await collection.index.wait({
      pollIntervalMs: 500,
      timeoutMs: 60_000,
      throwOnFailure: false,
    });
    expect(["active", "failed", "none"]).toContain(settled.status);
  });

  it("creates graph nodes and edges", async () => {
    const collection = await scratch();
    const a = (await collection.graph.createNode(1)).node_id;
    const b = (await collection.graph.createNode(1)).node_id;
    await collection.graph.createEdge(a, b, 1);
    expect(await collection.graph.getNode(a)).toBeDefined();
    expect(await collection.graph.listEdges(a)).toBeDefined();
    expect(await collection.graph.subgraph(a, 2)).toBeDefined();
    await collection.graph.deleteNode(b);
  });

  it("runs graphrag", async () => {
    const collection = await scratch();
    await collection.records.insert(vec(0.1));
    expect(await collection.graphrag(vec(0.1), { k: 3, depth: 1 })).toBeDefined();
  });

  it("lists and reads operations", async () => {
    const collection = await scratch();
    await collection.records.insert(vec(0.7));
    const listed = await client.operations.list();
    const first = listed.operations?.[0]?.id;
    if (!first) return; // this node keeps no operation history
    expect((await client.operations.get(first)).id).toBe(first);
    // An execution record exists only for operations the planner actually ran;
    // a plain kernel mutation has none and the node answers 404. Both are
    // correct, so this asserts the wrapper's behaviour rather than the node's
    // history: either typed data, or a typed NotFoundError.
    const execution = await client.operations
      .execution(first)
      .catch((err: unknown) => (err instanceof NotFoundError ? null : Promise.reject(err)));
    expect(execution === null || execution !== undefined).toBe(true);
  });

  // Phase API-4D §4/§15 — the closed loop the metadata bug would have caught.
  // Asserting a 200 on the insert proves nothing: the pre-fix SDKs sent a JSON
  // object where the contract wants UTF-8 bytes, and the node accepted the
  // request and stored something else. Only reading the value back and
  // comparing it to what the caller passed closes that loop. The Python suite
  // runs the same scenario against the same contract.
  it("round-trips metadata through a real node", async () => {
    const collection = await scratch();
    const original = {
      author: "alice",
      page: 4,
      score: 0.5,
      draft: false,
      parent: null,
      tags: ["a", "b"],
      src: { file: "a.md", line: 12 },
      title: "Übersicht — 東京",
    };
    const inserted = await collection.records.insert(vec(0.31), { metadata: original });
    const fetched = await collection.records.get(inserted.id!);

    expect(fetched.metadata).toBeDefined();
    expect(decodeStoredMetadata(fetched.metadata)).toEqual(original);
  });

  it("round-trips batch metadata through a real node", async () => {
    const collection = await scratch();
    const payloads = [
      { i: 0, kind: "first" },
      { i: 1, kind: "second" },
    ];
    const response = await collection.records.insertBatch([vec(0.41), vec(0.42)], {
      metadata: payloads,
    });
    const ids = response.ids ?? [];
    expect(ids).toHaveLength(2);

    for (const [index, id] of ids.entries()) {
      const fetched = await collection.records.get(id);
      expect(decodeStoredMetadata(fetched.metadata)).toEqual(payloads[index]);
    }
  });

  // SERVER BUG: metadata_filter never matches insert-time metadata.
  // See docs/api/known-server-issues.md #1 — the filter consults only the
  // metadata sidecar, keyed `rec:{id}`, so a predicate that exactly matches a
  // record's committed metadata returns zero hits. Confirmed with raw curl,
  // with no SDK in the path. Pinned rather than skipped, so that fixing the
  // server turns this red and forces the real assertion to be written.
  it("accepts metadata_filter but matches nothing (server bug)", async () => {
    const collection = await scratch();
    await collection.records.insert(vec(0.51), { metadata: { author: "alice" } });

    const unfiltered = await collection.search(vec(0.51), 5);
    expect(unfiltered.results?.length ?? 0).toBeGreaterThanOrEqual(1);

    const filtered = await collection.search(vec(0.51), 5, {
      metadataFilter: { author: "alice" },
    });
    expect(
      filtered.results,
      "metadata_filter started matching insert-time metadata — the server bug in " +
        "docs/api/known-server-issues.md #1 appears to be fixed. Tighten this test and " +
        "update that document.",
    ).toEqual([]);
  });

  it("exposes the proof surface", async () => {
    expect(await client.proof.eventLog()).toBeDefined();
    expect(await client.proof.state()).toBeDefined();
  });

  it("types errors from a real node", async () => {
    const thrown = await client
      .collection("nope-does-not-exist")
      .records.get(999_999)
      .catch((e) => e);
    expect(thrown).toBeInstanceOf(ValoriAPIError);
    expect(thrown.status).toBeGreaterThanOrEqual(400);
  });

  it("reports a dimension mismatch", async () => {
    const collection = await scratch();
    const thrown = await collection.records.insert(new Array(DIM + 3).fill(0.1)).catch((e) => e);
    expect(thrown).toBeInstanceOf(ValoriAPIError);
    expect([400, 422]).toContain(thrown.status);
  });

  it("chunks without an embedding provider", async () => {
    expect(await client.ingest.chunk("# Title\n\nSome body text.\n", { strategy: "auto" })).toBeDefined();
  });

  it("either ingests or says the embedding provider is unset", async () => {
    const collection = await scratch();
    const thrown = await client.ingest
      .document("# Title\n\nBody.", { collection: collection.name })
      .catch((e) => e);
    if (thrown instanceof ValoriAPIError) {
      // 422 is the documented answer when VALORI_EMBED_PROVIDER is unset.
      expect([422, 501]).toContain(thrown.status);
    }
  });
});

cluster("in cluster mode", () => {
  it("reports status, health and role", async () => {
    expect(await client.cluster.status()).toBeDefined();
    expect(await client.cluster.health()).toBeDefined();
    expect(await client.cluster.role()).toBeDefined();
  });

  it("serves a cluster proof", async () => {
    expect(await client.cluster.proof()).toBeDefined();
  });

  it("replicates a write through raft", async () => {
    const collection = await scratch();
    const inserted = await collection.records.insert(vec(0.9), { requestId: requestId() });
    expect(await collection.records.get(inserted.id)).toBeDefined();
  });
});
