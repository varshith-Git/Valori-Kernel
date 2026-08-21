// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Retry tests — Phase API-4A §8.
//
// The rule under test is the one that matters: a write is never repeated unless
// the caller gave the server something to dedup on.

import { describe, expect, it } from "vitest";

import {
  DEFAULT_RETRY_POLICY,
  IDEMPOTENCY_HEADER,
  ServiceUnavailableError,
  ValoriConnectionError,
  delayFor,
  isRetryableRequest,
  resolveRetryPolicy,
  shouldRetryStatus,
} from "../src/index.js";
import { COLLECTIONS_OK, HEALTH_OK, INSERT_OK, flaky, json, makeClient } from "./helpers.js";

const P = resolveRetryPolicy({ jitter: 0 });

describe("retry policy arithmetic", () => {
  it("treats safe methods as retryable with no further evidence", () => {
    for (const method of ["GET", "HEAD", "OPTIONS", "get"]) {
      expect(isRetryableRequest(P, method, false)).toBe(true);
    }
  });

  it("refuses to retry a write with no idempotency key", () => {
    for (const method of ["POST", "PATCH", "DELETE", "PUT"]) {
      expect(isRetryableRequest(P, method, false)).toBe(false);
    }
  });

  it("allows a write with an idempotency key", () => {
    expect(isRetryableRequest(P, "POST", true)).toBe(true);
  });

  it("lets idempotent-write retry be switched off", () => {
    const strict = resolveRetryPolicy({ jitter: 0, retryIdempotentWrites: false });
    expect(isRetryableRequest(strict, "POST", true)).toBe(false);
    expect(isRetryableRequest(strict, "GET", false)).toBe(true);
  });

  it("disables retry entirely at maxAttempts = 1", () => {
    expect(isRetryableRequest(resolveRetryPolicy({ maxAttempts: 1 }), "GET", false)).toBe(false);
  });

  it("backs off exponentially, capped", () => {
    const p = resolveRetryPolicy({
      backoffInitialMs: 1000,
      backoffMultiplier: 2,
      backoffMaxMs: 4000,
      jitter: 0,
    });
    expect([1, 2, 3, 4, 5].map((n) => delayFor(p, n))).toEqual([1000, 2000, 4000, 4000, 4000]);
  });

  it("lets Retry-After win over computed backoff", () => {
    const p = resolveRetryPolicy({ backoffInitialMs: 10_000, jitter: 0 });
    expect(delayFor(p, 1, 2)).toBe(2000);
  });

  it("clamps a hostile Retry-After", () => {
    const p = resolveRetryPolicy({ jitter: 0, retryAfterMaxMs: 30_000 });
    expect(delayFor(p, 1, 3600)).toBe(30_000);
  });

  it("can ignore Retry-After by policy", () => {
    const p = resolveRetryPolicy({ backoffInitialMs: 1000, jitter: 0, respectRetryAfter: false });
    expect(delayFor(p, 1, 99)).toBe(1000);
  });

  it("only ever lengthens the wait with jitter", () => {
    const p = resolveRetryPolicy({ backoffInitialMs: 1000, backoffMultiplier: 1, jitter: 0.5 });
    for (let i = 0; i < 50; i += 1) {
      const d = delayFor(p, 1);
      expect(d).toBeGreaterThanOrEqual(1000);
      expect(d).toBeLessThanOrEqual(1500);
    }
  });

  it("knows which statuses are worth repeating", () => {
    expect(shouldRetryStatus(P, 503)).toBe(true);
    expect(shouldRetryStatus(P, 429)).toBe(true);
    expect(shouldRetryStatus(P, 400)).toBe(false);
  });

  it("ships a conservative default", () => {
    expect(DEFAULT_RETRY_POLICY.maxAttempts).toBe(3);
    expect(DEFAULT_RETRY_POLICY.safeMethods).toEqual(["GET", "HEAD", "OPTIONS"]);
  });
});

describe("retry behaviour through the real transport", () => {
  it("retries a GET until it succeeds", async () => {
    const { client, recorder } = makeClient(flaky([503, 503], COLLECTIONS_OK), {
      retry: { jitter: 0 },
    });
    await expect(client.collections.names()).resolves.toEqual(["docs", "notes"]);
    expect(recorder.count).toBe(3);
  });

  it("stops at maxAttempts and surfaces the last error", async () => {
    const { client, recorder } = makeClient(flaky([503, 503, 503], COLLECTIONS_OK), {
      retry: { jitter: 0 },
    });
    await expect(client.collections.list()).rejects.toBeInstanceOf(ServiceUnavailableError);
    expect(recorder.count).toBe(3);
  });

  it("never repeats a POST that carries no request id", async () => {
    const { client, recorder } = makeClient(flaky([503], INSERT_OK), { retry: { jitter: 0 } });
    await expect(
      client.collection("docs").records.insert([0.1, 0.2, 0.3]),
    ).rejects.toBeInstanceOf(ServiceUnavailableError);
    expect(recorder.count).toBe(1);
  });

  it("retries a POST that carries a request id, keeping the header on every attempt", async () => {
    const { client, recorder } = makeClient(flaky([503], INSERT_OK), { retry: { jitter: 0 } });
    await client.collection("docs").records.insert([0.1], { requestId: "ins-1" });
    expect(recorder.count).toBe(2);
    for (const request of recorder.requests) {
      expect(request.headers.get(IDEMPOTENCY_HEADER)).toBe("ins-1");
    }
  });

  it("does not leak the idempotency key into the next call", async () => {
    const bodies = [INSERT_OK, HEALTH_OK];
    const { client, recorder } = makeClient(
      () =>
        new Response(JSON.stringify(bodies.shift()), {
          status: 200,
          headers: { "content-type": "application/json" },
        }),
      { retry: { jitter: 0 } },
    );
    await client.collection("docs").records.insert([0.1], { requestId: "ins-1" });
    await client.health();
    expect(recorder.last.headers.get(IDEMPOTENCY_HEADER)).toBeNull();
  });

  it("does not retry a non-retryable status", async () => {
    const { client, recorder } = makeClient(flaky([400], COLLECTIONS_OK), { retry: { jitter: 0 } });
    await expect(client.collections.list()).rejects.toThrow();
    expect(recorder.count).toBe(1);
  });

  it("retries network failures for safe methods", async () => {
    let calls = 0;
    const { client } = makeClient(
      () => {
        calls += 1;
        if (calls < 3) throw new TypeError("fetch failed");
        return new Response(JSON.stringify(COLLECTIONS_OK), {
          status: 200,
          headers: { "content-type": "application/json" },
        });
      },
      { retry: { jitter: 0 } },
    );
    await expect(client.collections.names()).resolves.toEqual(["docs", "notes"]);
    expect(calls).toBe(3);
  });

  it("can be told not to retry network failures", async () => {
    const { client, recorder } = makeClient(
      () => {
        throw new TypeError("fetch failed");
      },
      { retry: { jitter: 0, retryOnNetworkError: false } },
    );
    await expect(client.collections.list()).rejects.toBeInstanceOf(ValoriConnectionError);
    expect(recorder.count).toBe(1);
  });

  it("honours a server-named Retry-After", async () => {
    let first = true;
    const { client, waits } = makeClient(
      () => {
        if (first) {
          first = false;
          return new Response(JSON.stringify({ error: "slow down" }), {
            status: 429,
            headers: { "content-type": "application/json", "retry-after": "7" },
          });
        }
        return new Response(JSON.stringify(COLLECTIONS_OK), {
          status: 200,
          headers: { "content-type": "application/json" },
        });
      },
      { retry: { jitter: 0 } },
    );
    await client.collections.list();
    expect(waits).toEqual([7000]);
  });

  it("exposes the resolved policy on the client", () => {
    const { client } = makeClient(json(HEALTH_OK), { retry: { maxAttempts: 9 } });
    expect(client.retryPolicy.maxAttempts).toBe(9);
  });
});
