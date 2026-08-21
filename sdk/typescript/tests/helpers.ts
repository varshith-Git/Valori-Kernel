// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Shared test helpers.
//
// Tests drive the SDK through an injected `fetch`, which sits *below* the retry
// wrapper and below the generated client. A wrapper test therefore exercises
// the real code path — body assembly, the generated method, header assembly,
// response parsing, error mapping — and stubs only the socket.

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { ValoriClient, type ValoriClientOptions } from "../src/index.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = path.resolve(HERE, "../../..");
export const CONTRACT_PATH = path.join(REPO_ROOT, "api/openapi/valori-v1.yaml");
export const SDK_ROOT = path.resolve(HERE, "..");

export interface RecordedRequest {
  url: URL;
  method: string;
  headers: Headers;
  body: unknown;
}

export class Recorder {
  readonly requests: RecordedRequest[] = [];

  get last(): RecordedRequest {
    const request = this.requests.at(-1);
    if (!request) throw new Error("no request was made");
    return request;
  }

  get count(): number {
    return this.requests.length;
  }

  /** Query parameters of the last request, as a plain object. */
  get query(): Record<string, string> {
    return Object.fromEntries(this.last.url.searchParams.entries());
  }
}

export type Handler = (request: RecordedRequest) => Response;

/** Answer every request with the same JSON body. */
export function json(payload: unknown, status = 200, headers: Record<string, string> = {}): Handler {
  return () =>
    new Response(JSON.stringify(payload), {
      status,
      headers: { "content-type": "application/json", ...headers },
    });
}

/** Answer with no content — for cases that only assert on what was sent. */
export const noContent: Handler = () => new Response(null, { status: 204 });

/** Answer `statuses` in order (as 503-ish errors), then `final` forever. */
export function flaky(statuses: number[], final: unknown): Handler {
  const remaining = [...statuses];
  return () => {
    const next = remaining.shift();
    if (next !== undefined) {
      return new Response(JSON.stringify({ error: "later", code: "unavailable" }), {
        status: next,
        headers: { "content-type": "application/json" },
      });
    }
    return new Response(JSON.stringify(final), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  };
}

export interface TestClient {
  client: ValoriClient;
  recorder: Recorder;
  /** Milliseconds the SDK decided to wait between retries. */
  waits: number[];
}

export function makeClient(
  handler: Handler,
  options: Partial<ValoriClientOptions> = {},
): TestClient {
  const recorder = new Recorder();
  const waits: number[] = [];

  const fetchImpl: typeof fetch = async (input, init) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input.href : input.url,
    );
    const rawBody = init?.body;
    let body: unknown;
    if (typeof rawBody === "string") {
      try {
        body = JSON.parse(rawBody);
      } catch {
        body = rawBody;
      }
    } else {
      body = rawBody;
    }
    const request: RecordedRequest = {
      url,
      method: (init?.method ?? "GET").toUpperCase(),
      headers: new Headers(init?.headers ?? {}),
      body,
    };
    recorder.requests.push(request);
    return handler(request);
  };

  const client = new ValoriClient({
    endpoint: "http://node.test",
    apiKey: "test-key",
    // AbortSignal.timeout would arm a real timer in every test; the SDK-side
    // timeout is exercised explicitly in transport.test.ts instead.
    timeoutMs: 0,
    fetch: fetchImpl,
    sleep: async (ms) => {
      waits.push(ms);
    },
    random: () => 0, // deterministic jitter
    ...options,
  });

  return { client, recorder, waits };
}

// ── minimal bodies that satisfy the contract's required fields ───────────────

export const HEALTH_OK = {
  status: "ok",
  mode: "standalone",
  version: "0.0.0-test",
  shard_count: 1,
};

export const COLLECTIONS_OK = {
  collections: [
    { name: "docs", id: 0 },
    { name: "notes", id: 1 },
  ],
};

export const INSERT_OK = {
  id: 7,
  deduplicated: false,
  receipt: {
    record_id: 7,
    old_root: "00",
    new_root: "ab",
    proof: [],
    sequence: 1,
    timestamp: 0,
    state_hash: "ab",
  },
};

export const SEARCH_OK = { results: [] };

export function readContract(): Record<string, unknown> {
  // A tiny reader rather than a YAML dependency: everything these tests need
  // from the contract is either a top-level scalar or the ErrorCode enum.
  return { raw: readFileSync(CONTRACT_PATH, "utf8") } as Record<string, unknown>;
}

/** The closed `ErrorCode` enum, read straight out of the contract. */
export function contractErrorCodes(): string[] {
  const raw = readFileSync(CONTRACT_PATH, "utf8");
  const block = raw.match(/\n    ErrorCode:\n((?:.|\n)*?)\n    [A-Za-z]/);
  if (!block) throw new Error("ErrorCode schema not found in the contract");
  return [...block[1].matchAll(/^\s*-\s+(\w+)\s*$/gm)].map((m) => m[1]);
}

/** `info.version` from the contract. */
export function contractInfoVersion(): string {
  const raw = readFileSync(CONTRACT_PATH, "utf8");
  const match = raw.match(/^info:\n(?:.|\n)*?^  version:\s*['"]?([\d.]+)/m);
  if (!match) throw new Error("info.version not found in the contract");
  return match[1];
}

/** Every `operationId` in the contract. */
export function contractOperationIds(): string[] {
  const raw = readFileSync(CONTRACT_PATH, "utf8");
  return [...raw.matchAll(/^\s*operationId:\s*(\w+)\s*$/gm)].map((m) => m[1]);
}
