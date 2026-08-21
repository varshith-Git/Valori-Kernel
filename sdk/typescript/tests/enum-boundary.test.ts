// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Phase API-4D §5/§10 — the generated/handwritten boundary.
//
// Before API-4D, `create()` took `metric: string` / `index?: string` and
// `index.build()` took `type?: string`, and an `as unknown as` cast at the wire
// boundary let those strings through to a generated type that is an enum. A
// typo reached the server. These tests pin both halves of the fix:
//
//   * compile time — the string unions reject a non-member (see the
//     `@ts-expect-error` cases, which fail the build if the error disappears);
//   * run time     — `asEnum` throws instead of casting a lie onto the wire.

import { describe, expect, it } from "vitest";

import { ValoriConfigError } from "../src/index.js";
import type { ValoriClient } from "../src/index.js";
import { makeClient, noContent, type Recorder } from "./helpers.js";

async function call(invoke: (client: ValoriClient) => Promise<unknown>): Promise<Recorder> {
  const { client, recorder } = makeClient(noContent);
  await invoke(client);
  return recorder;
}

describe("enum boundary — runtime validation", () => {
  it("rejects an invalid metric before any request is made", async () => {
    const { client, recorder } = makeClient(noContent);
    await expect(
      // @ts-expect-error "cosine" is not a contract metric — this must not compile.
      client.collections.create("docs", { dimension: 3, metric: "cosine" }),
    ).rejects.toThrow(ValoriConfigError);
    expect(recorder.count).toBe(0);
  });

  it("rejects an invalid index kind before any request is made", async () => {
    const { client, recorder } = makeClient(noContent);
    await expect(
      // @ts-expect-error "hsnw" is a typo for "hnsw".
      client.collections.create("docs", { dimension: 3, metric: "squared_l2", index: "hsnw" }),
    ).rejects.toThrow(ValoriConfigError);
    expect(recorder.count).toBe(0);
  });

  it("rejects an invalid buildable index type on index.build()", () => {
    // `build()` is not `async`, so the guard throws synchronously — before the
    // request promise is even constructed.
    const { client, recorder } = makeClient(noContent);
    expect(() =>
      // @ts-expect-error "brute" is not *buildable* — the contract allows hnsw/ivf/bq.
      client.collection("docs").index.build("brute"),
    ).toThrow(ValoriConfigError);
    expect(recorder.count).toBe(0);
  });

  it("names the allowed values in the error message", async () => {
    const { client } = makeClient(noContent);
    await expect(
      // @ts-expect-error deliberately invalid
      client.collections.create("d", { dimension: 3, metric: "euclidean" }),
    ).rejects.toThrow(/squared_l2/);
  });
});

describe("enum boundary — valid values still reach the wire verbatim", () => {
  it("sends every contract metric unchanged", async () => {
    for (const metric of ["squared_l2", "l2", "l2sq"] as const) {
      const recorder = await call((c) => c.collections.create("docs", { dimension: 3, metric }));
      expect(recorder.last.body).toMatchObject({ metric });
    }
  });

  it("sends every contract index kind unchanged", async () => {
    for (const index of ["brute", "bruteforce", "hnsw", "ivf", "bq", "auto", "mstg"] as const) {
      const recorder = await call((c) =>
        c.collections.create("docs", { dimension: 3, metric: "squared_l2", index }),
      );
      expect(recorder.last.body).toMatchObject({ index });
    }
  });

  it("sends every buildable index type unchanged", async () => {
    for (const type of ["hnsw", "ivf", "bq"] as const) {
      const recorder = await call((c) => c.collection("docs").index.build(type));
      expect(recorder.last.body).toMatchObject({ type });
    }
  });

  it("omits the index type entirely when not supplied", async () => {
    const recorder = await call((c) => c.collection("docs").index.build());
    expect(recorder.last.body).not.toHaveProperty("type");
  });
});
