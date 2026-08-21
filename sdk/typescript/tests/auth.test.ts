// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Authentication tests — Phase API-4A §6.

import { describe, expect, it } from "vitest";

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
});
