// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Operations and polling ergonomics — Phase API-4A §9.

import type {
  OperationDetailResponse,
  OperationsListResponse,
} from "../../generated/valori-api.js";
import { OperationFailedError, OperationTimeoutError } from "../errors.js";
import type { Transport, V1Data } from "../transport.js";
import type { WaitOptions } from "./collection.js";

/**
 * Statuses the node uses to mean "this operation is over".
 *
 * Taken from the strings the handlers actually emit (`ingest.rs`, `server.rs`,
 * `cluster_server.rs`): `processing` while running, `completed`/`failed` at the
 * end. The wider set is defensive, not speculative — a node that reports
 * `cancelled` must not hang a caller forever.
 */
export const TERMINAL_STATES = [
  "completed",
  "complete",
  "succeeded",
  "failed",
  "error",
  "cancelled",
] as const;

export const FAILED_STATES = ["failed", "error", "cancelled"] as const;

const realSleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/** A handle to one operation, with `wait()`. */
export class Operation {
  constructor(
    private readonly operations: Operations,
    readonly id: string,
    public data?: OperationDetailResponse,
  ) {}

  get status(): string | undefined {
    return this.data?.status;
  }

  get done(): boolean {
    const s = this.status;
    return s !== undefined && (TERMINAL_STATES as readonly string[]).includes(s);
  }

  get failed(): boolean {
    const s = this.status;
    return s !== undefined && (FAILED_STATES as readonly string[]).includes(s);
  }

  /** Re-read the operation. One request. */
  async refresh(): Promise<Operation> {
    this.data = await this.operations.fetch(this.id);
    return this;
  }

  /** `GET /v1/operations/{id}/execution`. */
  execution(): Promise<V1Data<"getOperationExecution">> {
    return this.operations.execution(this.id);
  }

  /** Poll until the operation reaches a terminal state. */
  async wait(options: WaitOptions = {}): Promise<Operation> {
    const pollIntervalMs = options.pollIntervalMs ?? 1000;
    const timeoutMs = options.timeoutMs ?? 300_000;
    const throwOnFailure = options.throwOnFailure ?? true;
    const sleep = options.sleep ?? realSleep;
    const now = options.now ?? Date.now;

    const deadline = now() + timeoutMs;
    for (;;) {
      if (this.data === undefined) await this.refresh();
      if (this.done) {
        if (this.failed && throwOnFailure) {
          throw new OperationFailedError(
            `operation ${this.id} ended in status ${this.status}`,
            this.id,
            this.status ?? "unknown",
            this.data,
          );
        }
        return this;
      }
      if (now() >= deadline) {
        throw new OperationTimeoutError(
          `operation ${this.id} did not finish within ${timeoutMs}ms`,
          this.id,
          this.status,
        );
      }
      await sleep(pollIntervalMs);
      await this.refresh();
    }
  }
}

export class Operations {
  constructor(private readonly t: Transport) {}

  /** `GET /v1/operations`. */
  list(): Promise<OperationsListResponse> {
    return this.t.call(() => this.t.api.v1.listOperations(this.t.params()));
  }

  /** @internal — the raw read behind `get` and `Operation.refresh`. */
  fetch(operationId: string): Promise<OperationDetailResponse> {
    return this.t.call(() => this.t.api.v1.getOperation(operationId, this.t.params()));
  }

  /** `GET /v1/operations/{id}`, wrapped in an `Operation` handle. */
  async get(operationId: string): Promise<Operation> {
    return new Operation(this, operationId, await this.fetch(operationId));
  }

  /** `GET /v1/operations/{id}/execution`. */
  execution(operationId: string): Promise<V1Data<"getOperationExecution">> {
    return this.t.call(() =>
      this.t.api.v1.getOperationExecution(operationId, this.t.params()),
    );
  }

  /** Convenience: `operations.wait(id)` == `(await operations.get(id)).wait()`. */
  wait(operationId: string, options: WaitOptions = {}): Promise<Operation> {
    return new Operation(this, operationId).wait(options);
  }
}
