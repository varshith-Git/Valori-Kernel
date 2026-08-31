// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Authentication tests — Phase API-4A §6.

import { describe, expect, it, vi } from "vitest";

import { ValoriClient, ValoriConfigError } from "../src/index.js";
import { COLLECTIONS_OK, HEALTH_OK, json, makeClient } from "./helpers.js";

describe("authentication", () => {
  it("sends the api key as a bearer token on secured operations", async () => {
    const { client, recorder } = makeClient(json(COLLECTIONS_OK), { apiKey: "sk-abc123" });
    await client.collections.list();
    expect(recorder.last.headers.get("authorization")).toBe("Bearer sk-abc123");
  });

  it("sends no authorization header when there is no api key", async () => {
    const { client, recorder } = makeClient(json(COLLECTIONS_OK), { apiKey: undefined });
    await client.collections.list();
    expect(recorder.last.headers.get("authorization")).toBeNull();
  });

  it("leaves GET /health unauthenticated, as the contract declares", async () => {
    // `security: []` on this one operation is what lets load-balancer probes
    // work without a token. Sending a bearer anyway would be harmless but would
    // misrepresent the contract, so the SDK does not.
    const { client, recorder } = makeClient(json(HEALTH_OK), { apiKey: "sk-abc123" });
    await client.health();
    expect(recorder.last.headers.get("authorization")).toBeNull();
  });

  it("never renders the api key", () => {
    const { client } = makeClient(json(HEALTH_OK), { apiKey: "sk-super-secret" });
    for (const rendering of [String(client), JSON.stringify(client)]) {
      expect(rendering).not.toContain("sk-super-secret");
      expect(rendering).toContain("***");
    }
  });

  it("merges custom headers without displacing auth", async () => {
    const { client, recorder } = makeClient(json(COLLECTIONS_OK), {
      apiKey: "sk-1",
      headers: { "X-Tenant": "acme" },
    });
    await client.collections.list();
    expect(recorder.last.headers.get("x-tenant")).toBe("acme");
    expect(recorder.last.headers.get("authorization")).toBe("Bearer sk-1");
  });

  it("refuses to construct without an endpoint", () => {
    const saved = process.env.VALORI_ENDPOINT;
    delete process.env.VALORI_ENDPOINT;
    try {
      expect(() => new ValoriClient()).toThrow(ValoriConfigError);
      expect(() => new ValoriClient()).toThrow(/self-hosted/);
    } finally {
      if (saved !== undefined) process.env.VALORI_ENDPOINT = saved;
    }
  });

  it("reads endpoint and api key from the environment", () => {
    process.env.VALORI_ENDPOINT = "http://from-env:3000";
    process.env.VALORI_API_KEY = "sk-from-env";
    try {
      const client = new ValoriClient();
      expect(client.endpoint).toBe("http://from-env:3000");
      expect(String(client)).not.toContain("sk-from-env");
      expect(String(client)).toContain("***");
    } finally {
      delete process.env.VALORI_ENDPOINT;
      delete process.env.VALORI_API_KEY;
    }
  });

  it("strips a trailing slash from the endpoint", async () => {
    const { client, recorder } = makeClient(json(HEALTH_OK), {
      endpoint: "http://node.test///",
    });
    await client.health();
    expect(recorder.last.url.href).toBe("http://node.test/health");
  });

  // ── Cross-SDK endpoint-resolution contract (G2.14 parity) ─────────────────
  // These exist identically (same intent) in sdk/python/tests/test_auth.py.
  // Endpoint resolution, highest priority first: the endpoint option, then
  // VALORI_ENDPOINT, then — only when apiKey was given and neither of those
  // named an endpoint — Cloud SaaS. No endpoint and no apiKey at all is a
  // configuration error, not a default.

  it("defaults to Cloud SaaS when an api key is given with no endpoint", () => {
    delete process.env.VALORI_ENDPOINT;
    const client = new ValoriClient({ apiKey: "vlk_test_key" });
    expect(client.endpoint).toBe("https://app.valori.systems");
  });

  it("prefers an explicit endpoint over env and the Cloud default", () => {
    process.env.VALORI_ENDPOINT = "http://from-env:3000";
    try {
      const client = new ValoriClient({ endpoint: "http://explicit:9000", apiKey: "vlk_test_key" });
      expect(client.endpoint).toBe("http://explicit:9000");
    } finally {
      delete process.env.VALORI_ENDPOINT;
    }
  });

  it("prefers VALORI_ENDPOINT over the Cloud default when an api key is also set", () => {
    process.env.VALORI_ENDPOINT = "http://from-env:3000";
    try {
      const client = new ValoriClient({ apiKey: "vlk_test_key" });
      expect(client.endpoint).toBe("http://from-env:3000");
    } finally {
      delete process.env.VALORI_ENDPOINT;
    }
  });

  it("rejects a non-string api key", () => {
    expect(
      () => new ValoriClient({ endpoint: "http://node.test", apiKey: 12345 as unknown as string }),
    ).toThrow(ValoriConfigError);
  });

  it("defaults the client-wide timeout to 30 seconds, matching the Python SDK", async () => {
    // The transport is private, so the default is observed the same way it's
    // actually used: via the real AbortSignal.timeout() call it makes when no
    // per-call timeoutMs is given. makeClient's helper default (timeoutMs: 0,
    // to avoid arming real timers in every other test) is deliberately NOT
    // used here — this test needs the real, unconfigured default.
    const spy = vi.spyOn(AbortSignal, "timeout");
    try {
      const client = new ValoriClient({
        endpoint: "http://node.test",
        fetch: async () => new Response(JSON.stringify(HEALTH_OK), { status: 200 }),
      });
      await client.health();
      expect(spy).toHaveBeenCalledWith(30_000);
    } finally {
      spy.mockRestore();
    }
  });
});
