// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// The one place the handwritten layer touches the generated client —
// Phase API-4A §4/§6/§7.
//
// Owns: construction of the generated `HttpClient`/`GeneratedApi`, the bearer
// auth header, the retry `fetch` wrapper, per-call idempotency keys, and the
// single `call` funnel that turns a generated promise into either typed data or
// a typed error.
//
// The generated layer never imports anything from here. The arrow points one
// way: handwritten → generated → HTTP.

import { GeneratedApi, HttpClient } from "../generated/valori-api.js";
import type { HttpResponse, RequestParams } from "../generated/valori-api.js";
import {
  ValoriAPIError,
  ValoriConfigError,
  ValoriConnectionError,
  ValoriTimeoutError,
  errorFor,
} from "./errors.js";
import {
  IDEMPOTENCY_HEADER,
  type RetryPolicy,
  resolveRetryPolicy,
  withRetry,
} from "./retry.js";

/**
 * The success type of a generated operation, by name.
 *
 * Wrappers declare their return type as `V1Data<"search">` rather than naming a
 * `SearchResponse` interface. Two reasons, both about honesty:
 *
 *   * several operations have no named response interface in the generated file
 *     (the generator inlines them), so naming would mean inventing a name that
 *     does not exist in the contract;
 *   * a wrapper's declared type is then *by construction* the generated type.
 *     It cannot drift from the contract, because it is not a copy.
 */
type V1Methods = GeneratedApi<unknown>["v1"];
type HealthMethods = GeneratedApi<unknown>["health"];

export type V1Data<K extends keyof V1Methods> = V1Methods[K] extends (
  ...args: never[]
) => Promise<HttpResponse<infer D, unknown>>
  ? D
  : never;

export type HealthData<K extends keyof HealthMethods> = HealthMethods[K] extends (
  ...args: never[]
) => Promise<HttpResponse<infer D, unknown>>
  ? D
  : never;

export interface TransportOptions {
  endpoint: string;
  apiKey?: string;
  /** Per-request timeout in milliseconds. `0` disables the SDK-side timeout. */
  timeoutMs?: number;
  headers?: Record<string, string>;
  retry?: Partial<RetryPolicy>;
  fetch?: typeof fetch;
  /** Injectable so tests never actually wait between retries. */
  sleep?: (ms: number) => Promise<void>;
  random?: () => number;
}

/** Extra options a caller may attach to any wrapper call. */
export interface CallOptions {
  /**
   * Server-side dedup key. Sent as the `Idempotency-Key` header, and its
   * presence is what makes a write eligible for automatic retry (§8).
   */
  requestId?: string;
  signal?: AbortSignal;
  /**
   * Overrides the client-wide `timeoutMs` for this one call. `0` disables the
   * SDK-side timeout. Ignored when an explicit `signal` is supplied — the
   * caller's own abort controller wins.
   */
  timeoutMs?: number;
}

export class Transport {
  readonly endpoint: string;
  readonly retryPolicy: RetryPolicy;
  readonly api: GeneratedApi<unknown>;
  readonly http: HttpClient<unknown>;

  readonly #authenticated: boolean;
  readonly #timeoutMs: number;

  constructor(options: TransportOptions) {
    if (!options.endpoint || typeof options.endpoint !== "string") {
      throw new ValoriConfigError(
        "no endpoint given — Valori is self-hosted; there is no default host",
      );
    }
    if (options.apiKey !== undefined && typeof options.apiKey !== "string") {
      throw new ValoriConfigError("apiKey must be a string");
    }

    this.endpoint = options.endpoint.replace(/\/+$/, "");
    this.#authenticated = Boolean(options.apiKey);
    this.#timeoutMs = options.timeoutMs ?? 30_000;
    this.retryPolicy = resolveRetryPolicy(options.retry);

    const baseFetch =
      options.fetch ?? ((input, init) => globalThis.fetch(input, init));
    const retryingFetch = withRetry({
      policy: this.retryPolicy,
      fetch: baseFetch,
      sleep: options.sleep,
      random: options.random,
    });

    this.http = new HttpClient({
      baseUrl: this.endpoint,
      customFetch: retryingFetch,
      baseApiParams: {
        headers: { ...(options.headers ?? {}) },
        // The browser default of `same-origin` would silently drop the auth
        // header on a cross-origin node, which is the normal deployment.
        credentials: "omit",
      },
      // §6: `Authorization: Bearer <api_key>`. Built here and nowhere else.
      securityWorker: options.apiKey
        ? () => ({ headers: { Authorization: `Bearer ${options.apiKey}` } })
        : undefined,
    });
    this.api = new GeneratedApi(this.http);
  }

  get authenticated(): boolean {
    return this.#authenticated;
  }

  /** §6: never leak the key — not in a log line, not in a stringified client. */
  toJSON(): Record<string, unknown> {
    return {
      endpoint: this.endpoint,
      apiKey: this.#authenticated ? "***" : null,
    };
  }

  toString(): string {
    return `Transport(endpoint=${this.endpoint}, apiKey=${
      this.#authenticated ? "***" : "null"
    })`;
  }

  /** Request params for one call: idempotency key, abort signal, timeout. */
  params(options?: CallOptions): RequestParams {
    const headers: Record<string, string> = {};
    if (options?.requestId) headers[IDEMPOTENCY_HEADER] = options.requestId;

    let signal = options?.signal;
    const timeoutMs = options?.timeoutMs ?? this.#timeoutMs;
    if (!signal && timeoutMs > 0 && typeof AbortSignal?.timeout === "function") {
      signal = AbortSignal.timeout(timeoutMs);
    }
    return { headers, ...(signal ? { signal } : {}) };
  }

  /**
   * Run a generated call and unwrap it.
   *
   * The generated client throws its `HttpResponse` on a non-2xx; this converts
   * that into the right `ValoriAPIError` subclass, and network/abort failures
   * into `ValoriConnectionError`/`ValoriTimeoutError`.
   */
  async call<T>(invoke: () => Promise<HttpResponse<T, unknown>>): Promise<T> {
    try {
      const response = await invoke();
      return response.data;
    } catch (thrown) {
      throw await this.#convert(thrown);
    }
  }

  async #convert(thrown: unknown): Promise<Error> {
    if (thrown instanceof ValoriAPIError) return thrown;

    // The generated client throws the HttpResponse itself on !response.ok.
    if (thrown && typeof thrown === "object" && "status" in thrown && "headers" in thrown) {
      const response = thrown as HttpResponse<unknown, unknown>;
      // `error` is only populated for operations the generator gave a response
      // `format` to. Operations documented as bodiless (a 204, a bare DELETE)
      // still carry a real ApiError body at runtime when they fail, so read it
      // rather than reporting a status with no code.
      let body: unknown = response.error;
      // A non-JSON error body (axum's own 422 rejections are `text/plain`)
      // leaves the generated client's `JSON.parse` failure sitting in `.error`.
      // That SyntaxError is not the server's message, and keeping it would lose
      // the real one — §19 requires the raw information survive.
      if (
        body === null ||
        body === undefined ||
        body === response ||
        body instanceof Error
      ) {
        body = await Transport.#readBody(response);
      }
      return errorFor(response.status, body, response.headers);
    }

    if (thrown instanceof Error) {
      if (thrown.name === "TimeoutError" || thrown.name === "AbortError") {
        return new ValoriTimeoutError(`request to ${this.endpoint} timed out: ${thrown.message}`);
      }
      if (thrown.name === "TypeError" || thrown.name === "FetchError") {
        return new ValoriConnectionError(`could not reach ${this.endpoint}: ${thrown.message}`);
      }
      return thrown;
    }
    return new ValoriError_(String(thrown));
  }

  static async #readBody(response: Response): Promise<unknown> {
    if (response.bodyUsed) return undefined;
    try {
      const text = await response.clone().text();
      if (!text) return undefined;
      try {
        return JSON.parse(text);
      } catch {
        return text;
      }
    } catch {
      return undefined;
    }
  }
}

// Local alias so the `unknown thrown value` branch above still produces an Error
// without widening the public export surface.
class ValoriError_ extends ValoriAPIError {
  constructor(message: string) {
    super({ status: 0, message });
  }
}
