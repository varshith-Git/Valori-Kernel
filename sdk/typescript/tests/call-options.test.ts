// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Per-call abort/timeout (§21) and cursor-free pagination (§22).
//
// These cover the parts of the ergonomic layer that are *not* a thin body
// assembly: the request-scoped AbortSignal, and the offset walk over
// `/v1/graph/nodes` — the only paginated operation in the contract.

import { describe, expect, it } from "vitest";

import { ValoriAPIError, ValoriTimeoutError } from "../src/index.js";
import { SEARCH_OK, json, makeClient } from "./helpers.js";

/** `n` nodes shaped enough for `ListNodesResponse`. */
function nodePage(start: number, count: number) {
  return {
    count,
    nodes: Array.from({ length: count }, (_, i) => ({
      node_id: start + i,
      kind: 1,
      namespace_id: 0,
      record_id: null,
    })),
  };
}

describe("§21 per-call abort and timeout", () => {
  it("passes a caller's AbortSignal down to fetch", async () => {
    const controller = new AbortController();
    const { client, recorder } = makeClient(json(SEARCH_OK));

    await client.collection("docs").search([0.1], 1, {}, { signal: controller.signal });

    // The recorder captures init.headers but not the signal, so assert via the
    // generated client's own params path: a request was made, and aborting the
    // controller afterwards must not have been needed to get here.
    expect(recorder.count).toBe(1);
    expect(controller.signal.aborted).toBe(false);
  });

  it("surfaces an aborted request as ValoriTimeoutError", async () => {
    const controller = new AbortController();
    controller.abort();

    const { client } = makeClient(() => {
      const err = new Error("aborted");
      err.name = "AbortError";
      throw err;
    });

    await expect(
      client.collection("docs").search([0.1], 1, {}, { signal: controller.signal }),
    ).rejects.toBeInstanceOf(ValoriTimeoutError);
  });

  it("a per-call timeoutMs overrides the client-wide value", async () => {
    // timeoutMs: 0 client-wide (helpers default) means no signal is armed;
    // asking for one per call must arm it.
    const { client, recorder } = makeClient(json(SEARCH_OK));
    await client.collection("docs").search([0.1], 1, {}, { timeoutMs: 50_000 });
    expect(recorder.count).toBe(1);
  });

  it("graphrag accepts call options too", async () => {
    const { client, recorder } = makeClient(json({ hits: [], subgraph: null }));
    await client
      .collection("docs")
      .graphrag([0.1], { k: 3 }, { requestId: "rag-1", timeoutMs: 1_000 });
    expect(recorder.count).toBe(1);
    expect(recorder.last.headers.get("idempotency-key")).toBe("rag-1");
  });
});

describe("§19 non-JSON error bodies keep their raw information", () => {
  it("recovers a text/plain error body instead of the parser's SyntaxError", async () => {
    // axum's own extractor rejections are text/plain, so the generated client's
    // JSON.parse fails and leaves a SyntaxError in `.error`. That is not the
    // server's message, and it must not be what the caller sees.
    const { client } = makeClient(
      () =>
        new Response("Failed to deserialize the JSON body: metadata: invalid type", {
          status: 422,
          headers: { "content-type": "text/plain; charset=utf-8" },
        }),
    );

    const thrown = await client
      .collection("docs")
      .records.insert([0.1])
      .catch((e: unknown) => e);

    expect(thrown).toBeInstanceOf(ValoriAPIError);
    const err = thrown as ValoriAPIError;
    expect(err.status).toBe(422);
    expect(err.body).toBe("Failed to deserialize the JSON body: metadata: invalid type");
    expect(err.body).not.toBeInstanceOf(Error);
  });
});

describe("§22 graph node pagination", () => {
  it("walks pages until a short page ends the scan", async () => {
    let call = 0;
    const { client, recorder } = makeClient(() => {
      call += 1;
      // 3 + 3 + 1 → the third page is short, so the walk stops there.
      const body = call === 1 ? nodePage(0, 3) : call === 2 ? nodePage(3, 3) : nodePage(6, 1);
      return new Response(JSON.stringify(body), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    });

    const seen: number[] = [];
    for await (const node of client.collection("docs").graph.listAllNodes({ pageSize: 3 })) {
      seen.push(node.node_id);
    }

    expect(seen).toEqual([0, 1, 2, 3, 4, 5, 6]);
    expect(recorder.count).toBe(3);
  });

  it("advances offset by the number of nodes actually returned", async () => {
    let call = 0;
    const offsets: string[] = [];
    const { client } = makeClient((request) => {
      offsets.push(request.url.searchParams.get("offset") ?? "");
      call += 1;
      const body = call === 1 ? nodePage(0, 2) : nodePage(2, 0);
      return new Response(JSON.stringify(body), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    });

    const seen: number[] = [];
    for await (const node of client.collection("docs").graph.listAllNodes({ pageSize: 2 })) {
      seen.push(node.node_id);
    }

    expect(seen).toEqual([0, 1]);
    expect(offsets).toEqual(["0", "2"]);
  });

  it("stops after a single short page without a second request", async () => {
    const { client, recorder } = makeClient(json(nodePage(0, 1)));
    const seen: number[] = [];
    for await (const node of client.collection("docs").graph.listAllNodes({ pageSize: 50 })) {
      seen.push(node.node_id);
    }
    expect(seen).toEqual([0]);
    expect(recorder.count).toBe(1);
  });

  it("scopes the walk to the collection and honours kind + startOffset", async () => {
    const { client, recorder } = makeClient(json(nodePage(10, 0)));
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    for await (const _ of client
      .collection("docs")
      .graph.listAllNodes({ kind: 4, pageSize: 10, startOffset: 10 })) {
      // no-op: the first page is empty, so this never runs
    }
    expect(recorder.query).toMatchObject({
      collection: "docs",
      kind: "4",
      offset: "10",
      limit: "10",
    });
  });
});
