// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Error-mapping tests — Phase API-4A §7.

import { describe, expect, it } from "vitest";

import {
  AuthenticationError,
  AuthorizationError,
  CapacityExceededError,
  CODE_MAP,
  CollectionAlreadyExistsError,
  CollectionNotFoundError,
  ConflictError,
  DimensionMismatchError,
  IndexBuildFailedError,
  InvalidIndexError,
  InvalidMetricError,
  NotFoundError,
  NotImplementedAPIError,
  NotLeaderError,
  RateLimitError,
  RecordNotFoundError,
  ServerError,
  ServiceUnavailableError,
  ValidationError,
  ValoriAPIError,
  ValoriConnectionError,
  errorFor,
} from "../src/index.js";
import { HEALTH_OK, contractErrorCodes, json, makeClient } from "./helpers.js";

const EXPECTED: Record<string, unknown> = {
  validation_error: ValidationError,
  unauthorized: AuthenticationError,
  forbidden: AuthorizationError,
  not_found: NotFoundError,
  collection_not_found: CollectionNotFoundError,
  record_not_found: RecordNotFoundError,
  dimension_mismatch: DimensionMismatchError,
  invalid_metric: InvalidMetricError,
  invalid_index: InvalidIndexError,
  index_build_failed: IndexBuildFailedError,
  conflict: ConflictError,
  capacity_exceeded: CapacityExceededError,
  not_leader: NotLeaderError,
  unavailable: ServiceUnavailableError,
  not_implemented: NotImplementedAPIError,
  internal_error: ServerError,
};

describe("error mapping", () => {
  for (const [code, Ctor] of Object.entries(EXPECTED)) {
    it(`maps ${code} to its exception`, () => {
      const error = errorFor(400, { error: "boom", code });
      expect(error).toBeInstanceOf(Ctor as never);
      expect(error.code).toBe(code);
      expect(error.message).toBe("boom");
    });
  }

  it("covers the contract's closed ErrorCode enum exactly", () => {
    // Add a code to the Rust ErrorCode enum and this fails until the SDK names it.
    const declared = new Set(contractErrorCodes());
    const mapped = new Set(Object.keys(CODE_MAP));
    expect([...declared].filter((c) => !mapped.has(c))).toEqual([]);
    expect([...mapped].filter((c) => !declared.has(c))).toEqual([]);
  });

  it("degrades an unknown code to the generic error without losing anything", () => {
    const body = { error: "something new", code: "quantum_desync", request_id: "req-9" };
    const error = errorFor(418, body);
    expect(error.constructor).toBe(ValoriAPIError);
    expect(error.status).toBe(418);
    expect(error.code).toBe("quantum_desync");
    expect(error.message).toBe("something new");
    expect(error.requestId).toBe("req-9");
    expect(error.body).toBe(body);
  });

  it("falls back to the status when there is no code", () => {
    expect(errorFor(401, "<html>nginx</html>")).toBeInstanceOf(AuthenticationError);
    expect(errorFor(503, null)).toBeInstanceOf(ServiceUnavailableError);
    expect(errorFor(418, null).constructor).toBe(ValoriAPIError);
  });

  it("treats 429 as a rate limit regardless of code, with retryAfter", () => {
    const error = errorFor(429, {}, new Headers({ "retry-after": "12" }));
    expect(error).toBeInstanceOf(RateLimitError);
    expect((error as RateLimitError).retryAfter).toBe(12);
  });

  it("does not guess at an HTTP-date Retry-After", () => {
    const headers = new Headers({ "retry-after": "Wed, 21 Oct 2026 07:28:00 GMT" });
    const error = errorFor(429, {}, headers) as RateLimitError;
    expect(error.retryAfter).toBeUndefined();
    expect(error.headers?.get("retry-after")).toMatch(/^Wed/);
  });

  it("reads the request id from headers first, then the body", () => {
    const headers = new Headers({ "x-request-id": "hdr-1" });
    expect(errorFor(500, { request_id: "body-1" }, headers).requestId).toBe("hdr-1");
    expect(errorFor(500, { request_id: "body-1" }).requestId).toBe("body-1");
  });

  it("keeps toString informative", () => {
    const error = errorFor(404, { error: "gone", code: "not_found", request_id: "r-1" });
    expect(String(error)).toContain("HTTP 404");
    expect(String(error)).toContain("not_found");
    expect(String(error)).toContain("r-1");
  });
});

describe("errors through the real call path", () => {
  it("carries the full response", async () => {
    const { client } = makeClient(
      json({ error: "no such collection", code: "collection_not_found" }, 404),
    );
    await expect(client.collections.delete("ghost")).rejects.toMatchObject({
      status: 404,
      code: "collection_not_found",
      message: "no such collection",
    });
  });

  it("turns a create-collection conflict into CollectionAlreadyExists", async () => {
    const { client } = makeClient(json({ error: "exists", code: "conflict" }, 409));
    const promise = client.collections.create("docs", { dimension: 3, metric: "squared_l2" });
    await expect(promise).rejects.toBeInstanceOf(CollectionAlreadyExistsError);
    // Still a ConflictError, so `catch (e) { if (e instanceof ConflictError) }` works.
    await expect(promise).rejects.toBeInstanceOf(ConflictError);
  });

  it("leaves a conflict elsewhere as a plain ConflictError", async () => {
    const { client } = makeClient(json({ error: "busy", code: "conflict" }, 409));
    await expect(
      client.collection("docs").records.delete(1),
    ).rejects.toSatisfy((e: unknown) => (e as object).constructor === ConflictError);
  });

  it("preserves a non-JSON error body as text", async () => {
    const { client } = makeClient(() => new Response("<html>bad gateway</html>", { status: 502 }));
    await expect(client.health()).rejects.toMatchObject({ status: 502 });
  });

  it("turns a network failure into a connection error", async () => {
    const { client } = makeClient(() => {
      throw new TypeError("fetch failed");
    });
    await expect(client.health()).rejects.toBeInstanceOf(ValoriConnectionError);
  });

  it("does not turn a success into an error", async () => {
    const { client } = makeClient(json(HEALTH_OK));
    await expect(client.health()).resolves.toMatchObject({ status: "ok" });
  });
});
