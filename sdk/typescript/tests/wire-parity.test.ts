// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Phase API-4D §7 — cross-SDK metadata wire parity.
//
// `sdk/metadata-wire-fixtures.json` is the shared table; the Python suite reads
// the same file. Both SDKs must turn the same ergonomic mapping into the same
// bytes, because `POST /v1/records` commits those bytes inside the
// `InsertRecord` event and they are therefore covered by the BLAKE3 audit
// chain. Divergence here means the same logical write produces two different
// state hashes depending on which SDK issued it.
//
// This asserts on what the SDK actually put on the socket, not on an internal
// helper, so it fails if the encoder is bypassed at any call site.

import { readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { REPO_ROOT, makeClient, noContent } from "./helpers.js";

interface Fixture {
  name: string;
  metadata: Record<string, unknown>;
  json: string;
  bytes: number[];
}

const fixtures: Fixture[] = JSON.parse(
  readFileSync(path.join(REPO_ROOT, "sdk/metadata-wire-fixtures.json"), "utf-8"),
).cases;

describe("cross-SDK metadata wire parity", () => {
  it("the fixture file is present and non-trivial", () => {
    expect(fixtures.length).toBeGreaterThanOrEqual(10);
  });

  it.each(fixtures)("POST /v1/records sends $name as the canonical bytes", async (fixture) => {
    const { client, recorder } = makeClient(noContent);
    await client.collection("docs").records.insert([0.1], { metadata: fixture.metadata });

    const body = recorder.last.body as { metadata: number[] };
    expect(body.metadata).toEqual(fixture.bytes);
    expect(new TextDecoder().decode(new Uint8Array(body.metadata))).toBe(fixture.json);
  });

  it.each(fixtures)("batch-insert sends $name as the canonical JSON string", async (fixture) => {
    const { client, recorder } = makeClient(noContent);
    await client.collection("docs").records.insertBatch([[0.1]], { metadata: [fixture.metadata] });

    const body = recorder.last.body as { metadata: (string | null)[] };
    expect(body.metadata).toEqual([fixture.json]);
  });

  it("preserves key insertion order rather than sorting", async () => {
    // A sorted serialiser would emit `{"a":2,"b":1}` and silently change the
    // committed bytes — and so the state hash — for every record.
    const { client, recorder } = makeClient(noContent);
    await client.collection("docs").records.insert([0.1], { metadata: { b: 1, a: 2 } });
    const body = recorder.last.body as { metadata: number[] };
    expect(new TextDecoder().decode(new Uint8Array(body.metadata))).toBe('{"b":1,"a":2}');
  });
});
