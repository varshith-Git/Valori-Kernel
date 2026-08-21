// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Async-operation polling tests — Phase API-4A §9.

import { describe, expect, it } from "vitest";

import {
  FAILED_STATES,
  IndexBuildFailedError,
  OperationFailedError,
  OperationTimeoutError,
  TERMINAL_INDEX_STATES,
  TERMINAL_STATES,
} from "../src/index.js";
import { makeClient } from "./helpers.js";

const op = (status: string) => ({
  id: "op-1",
  type: "insert_record",
  status,
  timing: "0ms",
  timestamp_unix: 0,
  collection: "docs",
  overview: { id: "op-1", type: "insert_record", status, timing: "0ms", collection: "docs" },
  results: { status, records_affected: 0, nodes_affected: 0, edges_affected: 0, message: "" },
  metrics: { duration_ms: 0, memory_bytes: 0, cpu_cycles: 0, status },
  proof: {},
});

const indexState = (status: string) => ({
  collection: "docs",
  desired_type: "hnsw",
  active_type: "none",
  status,
});

/** Answers one status per call, repeating the last forever. */
function sequence(statuses: string[], shape: (s: string) => unknown) {
  const remaining = [...statuses];
  return () => {
    const status = remaining.length > 1 ? remaining.shift()! : remaining[0];
    return new Response(JSON.stringify(shape(status)), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  };
}

/** A clock the test advances by hand, so nothing ever sleeps. */
function clock() {
  let t = 0;
  return {
    now: () => t,
    sleep: async (ms: number) => {
      t += ms;
    },
    get elapsed() {
      return t;
    },
  };
}

describe("operations", () => {
  it("returns a handle carrying the current status", async () => {
    const { client } = makeClient(sequence(["processing"], op));
    const handle = await client.operations.get("op-1");
    expect(handle.id).toBe("op-1");
    expect(handle.status).toBe("processing");
    expect(handle.done).toBe(false);
  });

  it("polls until the operation completes", async () => {
    const c = clock();
    const { client, recorder } = makeClient(sequence(["processing", "processing", "completed"], op));
    const handle = await (await client.operations.get("op-1")).wait({ now: c.now, sleep: c.sleep });
    expect(handle.status).toBe("completed");
    expect(handle.done).toBe(true);
    expect(recorder.count).toBe(3);
  });

  it("treats operations.wait(id) as get-then-wait", async () => {
    const c = clock();
    const { client } = makeClient(sequence(["completed"], op));
    const handle = await client.operations.wait("op-1", { now: c.now, sleep: c.sleep });
    expect(handle.status).toBe("completed");
  });

  it("does not poll again when the operation is already terminal", async () => {
    const c = clock();
    const { client, recorder } = makeClient(sequence(["completed"], op));
    await (await client.operations.get("op-1")).wait({ now: c.now, sleep: c.sleep });
    expect(recorder.count).toBe(1);
  });

  it("re-reads on refresh", async () => {
    const { client } = makeClient(sequence(["processing", "completed"], op));
    const handle = await client.operations.get("op-1");
    expect(handle.status).toBe("processing");
    expect((await handle.refresh()).status).toBe("completed");
  });

  it("raises with the id and status on failure", async () => {
    const c = clock();
    const { client } = makeClient(sequence(["processing", "failed"], op));
    const handle = await client.operations.get("op-1");
    const thrown = await handle.wait({ now: c.now, sleep: c.sleep }).catch((e) => e);
    expect(thrown).toBeInstanceOf(OperationFailedError);
    expect(thrown.operationId).toBe("op-1");
    expect(thrown.status).toBe("failed");
    expect(thrown.detail).toBeDefined();
  });

  it("can be told not to convert a failure into a throw", async () => {
    const c = clock();
    const { client } = makeClient(sequence(["failed"], op));
    const handle = await (await client.operations.get("op-1")).wait({
      throwOnFailure: false,
      now: c.now,
      sleep: c.sleep,
    });
    expect(handle.failed).toBe(true);
  });

  it("times out and reports the last status seen", async () => {
    const c = clock();
    const { client } = makeClient(sequence(["processing"], op));
    const handle = await client.operations.get("op-1");
    await expect(
      handle.wait({ pollIntervalMs: 1000, timeoutMs: 3000, now: c.now, sleep: c.sleep }),
    ).rejects.toMatchObject({ operationId: "op-1", lastStatus: "processing" });
  });

  it("lets the poll interval govern the wait", async () => {
    const c = clock();
    const { client } = makeClient(sequence(["processing"], op));
    const handle = await client.operations.get("op-1");
    await expect(
      handle.wait({ pollIntervalMs: 5000, timeoutMs: 12_000, now: c.now, sleep: c.sleep }),
    ).rejects.toBeInstanceOf(OperationTimeoutError);
    expect(c.elapsed).toBe(15_000); // three sleeps of five seconds
  });

  it("keeps the terminal and failed state sets consistent", () => {
    for (const state of FAILED_STATES) {
      expect(TERMINAL_STATES).toContain(state);
    }
    expect(TERMINAL_STATES).toContain("completed");
    expect(FAILED_STATES).not.toContain("completed");
    expect(TERMINAL_STATES).not.toContain("processing");
  });
});

describe("index builds get the same ergonomics", () => {
  it("polls until the build is active", async () => {
    const c = clock();
    const { client, recorder } = makeClient(
      sequence(["building", "building", "active"], indexState),
    );
    const settled = await client.collection("docs").index.wait({ now: c.now, sleep: c.sleep });
    expect(settled.status).toBe("active");
    expect(recorder.count).toBe(3);
  });

  it("raises on a failed build", async () => {
    const c = clock();
    const { client } = makeClient(sequence(["building", "failed"], indexState));
    await expect(
      client.collection("docs").index.wait({ now: c.now, sleep: c.sleep }),
    ).rejects.toBeInstanceOf(IndexBuildFailedError);
  });

  it("times out", async () => {
    const c = clock();
    const { client } = makeClient(sequence(["building"], indexState));
    await expect(
      client
        .collection("docs")
        .index.wait({ pollIntervalMs: 2000, timeoutMs: 4000, now: c.now, sleep: c.sleep }),
    ).rejects.toMatchObject({ lastStatus: "building" });
  });

  it("matches the terminal states the engine actually emits", () => {
    // valori-engine's IndexStatusResponse::from_state emits exactly these.
    expect([...TERMINAL_INDEX_STATES].sort()).toEqual(["active", "failed", "none"]);
  });
});
