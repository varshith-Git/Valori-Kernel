// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// The Valori TypeScript SDK entry point — Phase API-4A §11.

import { ValoriConfigError } from "./errors.js";
import { Collection, Collections } from "./resources/collection.js";
import {
  Cluster,
  Community,
  Crypto,
  IndexConfig,
  Ingest,
  Meta,
  Proof,
  Snapshots,
  Storage,
  Tree,
} from "./resources/node.js";
import { Operations } from "./resources/operations.js";
import type { RetryPolicy } from "./retry.js";
import { Transport } from "./transport.js";
import { API_CONTRACT_VERSION, VERSION } from "./version.js";

export interface ValoriClientOptions {
  /**
   * Base URL of the Valori node, e.g. `http://localhost:3000`.
   *
   * Valori is self-hosted, so there is no default. In Node this falls back to
   * `VALORI_ENDPOINT`.
   */
  endpoint?: string;
  /** Project API key. Falls back to `VALORI_API_KEY` in Node. Never logged. */
  apiKey?: string;
  /** Per-request timeout in milliseconds. `0` disables the SDK-side timeout. */
  timeoutMs?: number;
  /** Headers added to every request. */
  headers?: Record<string, string>;
  /** Retry behaviour. Merged over the defaults. */
  retry?: Partial<RetryPolicy>;
  /** Custom `fetch`, e.g. for tests or a proxying agent. */
  fetch?: typeof fetch;
  /** @internal — injected by tests so retries never actually wait. */
  sleep?: (ms: number) => Promise<void>;
  /** @internal — injected by tests to make jitter deterministic. */
  random?: () => number;
}

function fromEnv(name: string): string | undefined {
  const env = (globalThis as { process?: { env?: Record<string, string | undefined> } }).process
    ?.env;
  return env?.[name];
}

/**
 * A client for one Valori node.
 *
 * ```ts
 * import { ValoriClient } from "@valori/sdk";
 *
 * const client = new ValoriClient({ endpoint: "http://localhost:3000", apiKey: "…" });
 * const docs = await client.collections.create("docs", { dimension: 384, metric: "squared_l2" });
 * await docs.records.insert(new Array(384).fill(0.1), { requestId: "ins-1" });
 * const hits = await docs.search(new Array(384).fill(0.1), 5);
 * ```
 */
export class ValoriClient {
  /** The Valori REST API contract this SDK targets (§14). */
  static readonly apiContractVersion = API_CONTRACT_VERSION;
  readonly apiContractVersion = API_CONTRACT_VERSION;
  readonly sdkVersion = VERSION;

  readonly collections: Collections;
  readonly operations: Operations;
  readonly index: IndexConfig;
  readonly meta: Meta;
  readonly ingest: Ingest;
  readonly tree: Tree;
  readonly community: Community;
  readonly proof: Proof;
  readonly snapshots: Snapshots;
  readonly storage: Storage;
  readonly cluster: Cluster;
  readonly crypto: Crypto;

  readonly #transport: Transport;

  constructor(options: ValoriClientOptions = {}) {
    const endpoint = options.endpoint ?? fromEnv("VALORI_ENDPOINT");
    if (!endpoint) {
      throw new ValoriConfigError(
        "no endpoint given — pass endpoint or set VALORI_ENDPOINT " +
          "(Valori is self-hosted; there is no default host)",
      );
    }
    const apiKey = options.apiKey ?? fromEnv("VALORI_API_KEY");

    this.#transport = new Transport({
      endpoint,
      apiKey,
      timeoutMs: options.timeoutMs,
      headers: options.headers,
      retry: options.retry,
      fetch: options.fetch,
      sleep: options.sleep,
      random: options.random,
    });

    this.collections = new Collections(this.#transport);
    this.operations = new Operations(this.#transport);
    this.index = new IndexConfig(this.#transport);
    this.meta = new Meta(this.#transport);
    this.ingest = new Ingest(this.#transport);
    this.tree = new Tree(this.#transport);
    this.community = new Community(this.#transport);
    this.proof = new Proof(this.#transport);
    this.snapshots = new Snapshots(this.#transport);
    this.storage = new Storage(this.#transport);
    this.cluster = new Cluster(this.#transport);
    this.crypto = new Crypto(this.#transport);
  }

  get endpoint(): string {
    return this.#transport.endpoint;
  }

  get retryPolicy(): RetryPolicy {
    return this.#transport.retryPolicy;
  }

  /** Unchecked handle to a collection — no round trip. */
  collection(name: string): Collection {
    return new Collection(this.#transport, name);
  }

  /** `GET /health`. Shortcut for `client.meta.health()`. */
  health() {
    return this.meta.health();
  }

  /** `GET /v1/version`. Shortcut for `client.meta.version()`. */
  version() {
    return this.meta.version();
  }

  /**
   * The generated client, for operations the ergonomic layer has not wrapped.
   *
   * Every operation in the contract *is* wrapped today (see
   * `api-coverage.yaml`); this exists so a contract that grows an operation is
   * usable before the wrapper lands, without anyone standing up a second HTTP
   * client.
   */
  get raw() {
    return this.#transport.api;
  }

  /** §6: the API key is deliberately absent from every rendering. */
  toJSON(): Record<string, unknown> {
    return {
      endpoint: this.endpoint,
      apiKey: this.#transport.authenticated ? "***" : null,
      apiContractVersion: this.apiContractVersion,
      sdkVersion: this.sdkVersion,
    };
  }

  toString(): string {
    const key = this.#transport.authenticated ? "***" : "null";
    return `ValoriClient(endpoint=${this.endpoint}, apiKey=${key}, apiContract=${this.apiContractVersion})`;
  }
}
