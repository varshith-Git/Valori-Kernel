// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Typed errors for the Valori REST API — Phase API-4A §7/§11.
//
// Every error carries the full raw response: HTTP status, API `code`, API
// message, request id when present, and the undecoded body. Giving an error a
// class must never cost the caller information.
//
// The code table is the closed `ErrorCode` enum from
// api/openapi/valori-v1.yaml. `tests/errors.test.ts` reads the contract and
// fails if the server grows a code this table does not name.

import type { ApiError, ErrorCode } from "../generated/valori-api.js";

export interface ValoriErrorFields {
  status: number;
  code?: string;
  message: string;
  requestId?: string;
  body?: unknown;
  headers?: Headers;
}

/** Base class for every error this SDK throws. */
export class ValoriError extends Error {
  constructor(message: string) {
    super(message);
    this.name = new.target.name;
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

/** The client was constructed or called with an unusable configuration. */
export class ValoriConfigError extends ValoriError {}

/** The node could not be reached at all (DNS, TCP, TLS). */
export class ValoriConnectionError extends ValoriError {}

/** A request exceeded the configured timeout or was aborted. */
export class ValoriTimeoutError extends ValoriError {}

/**
 * The node answered with an error status.
 *
 * Also the concrete class used when the server's `code` is one this SDK does
 * not recognise — nothing is lost in that case, so an older SDK keeps working
 * against a newer node.
 */
export class ValoriAPIError extends ValoriError {
  readonly status: number;
  readonly code?: string;
  readonly requestId?: string;
  readonly body?: unknown;
  readonly headers?: Headers;

  constructor(fields: ValoriErrorFields) {
    super(fields.message);
    this.status = fields.status;
    this.code = fields.code;
    this.requestId = fields.requestId;
    this.body = fields.body;
    this.headers = fields.headers;
  }

  override toString(): string {
    const parts = [`HTTP ${this.status}`];
    if (this.code) parts.push(this.code);
    parts.push(this.message);
    if (this.requestId) parts.push(`(requestId=${this.requestId})`);
    return `${this.name}: ${parts.join(" ")}`;
  }
}

export class BadRequestError extends ValoriAPIError {}
export class ValidationError extends BadRequestError {}
export class AuthenticationError extends ValoriAPIError {}
export class AuthorizationError extends ValoriAPIError {}
export class NotFoundError extends ValoriAPIError {}
export class CollectionNotFoundError extends NotFoundError {}
export class RecordNotFoundError extends NotFoundError {}
export class DimensionMismatchError extends BadRequestError {}
export class InvalidMetricError extends BadRequestError {}
export class InvalidIndexError extends BadRequestError {}
export class IndexBuildFailedError extends ValoriAPIError {}
export class ConflictError extends ValoriAPIError {}

/**
 * A collection create collided with an existing collection.
 *
 * Honest note: the node has no distinct `collection_already_exists` code today
 * — it reports this as `conflict`. This subclass is thrown by
 * `client.collections.create` only, where the operation makes the meaning
 * unambiguous. Everywhere else a `conflict` stays a `ConflictError`.
 */
export class CollectionAlreadyExistsError extends ConflictError {}

export class CapacityExceededError extends ValoriAPIError {}
export class NotLeaderError extends ValoriAPIError {}
export class ServiceUnavailableError extends ValoriAPIError {}
export class NotImplementedAPIError extends ValoriAPIError {}
export class ServerError extends ValoriAPIError {}

/** HTTP 429. `retryAfter` is in seconds when the server sent a numeric header. */
export class RateLimitError extends ValoriAPIError {
  readonly retryAfter?: number;

  constructor(fields: ValoriErrorFields & { retryAfter?: number }) {
    super(fields);
    this.retryAfter = fields.retryAfter;
  }
}

/** A polled long-running operation reached a terminal failure state. */
export class OperationFailedError extends ValoriError {
  constructor(
    message: string,
    readonly operationId: string,
    readonly status: string,
    readonly detail?: unknown,
  ) {
    super(message);
  }
}

/** A polled operation did not reach a terminal state before the deadline. */
export class OperationTimeoutError extends ValoriTimeoutError {
  constructor(
    message: string,
    readonly operationId: string,
    readonly lastStatus?: string,
  ) {
    super(message);
  }
}

type ApiErrorCtor = new (fields: ValoriErrorFields) => ValoriAPIError;

/** The closed `ErrorCode` enum from the contract, mapped to classes. */
export const CODE_MAP: Record<string, ApiErrorCtor> = {
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

/** Fallback when the body carries no usable `code` — a proxy 502, an HTML page. */
const STATUS_MAP: Record<number, ApiErrorCtor> = {
  400: BadRequestError,
  401: AuthenticationError,
  403: AuthorizationError,
  404: NotFoundError,
  409: ConflictError,
  429: RateLimitError,
  500: ServerError,
  501: NotImplementedAPIError,
  503: ServiceUnavailableError,
  507: CapacityExceededError,
};

const REQUEST_ID_HEADERS = ["x-request-id", "x-valori-request-id", "request-id"];

function requestIdFrom(headers: Headers | undefined, body: unknown): string | undefined {
  if (headers) {
    for (const name of REQUEST_ID_HEADERS) {
      const value = headers.get(name);
      if (value) return value;
    }
  }
  if (body && typeof body === "object") {
    const record = body as Record<string, unknown>;
    for (const key of ["request_id", "requestId"]) {
      if (typeof record[key] === "string") return record[key] as string;
    }
  }
  return undefined;
}

function retryAfterFrom(headers: Headers | undefined): number | undefined {
  const raw = headers?.get("retry-after");
  if (!raw) return undefined;
  const seconds = Number(raw);
  // An HTTP-date form is not guessed at — a caller who needs it reads `headers`.
  return Number.isFinite(seconds) ? seconds : undefined;
}

/**
 * Build the most specific error for a failing response.
 *
 * Resolution order: 429 is always a RateLimitError (the contract has no
 * `rate_limited` code, so status is the only signal); then a recognised
 * `code`; then the status; then `ValoriAPIError`.
 */
export function errorFor(
  status: number,
  body: unknown,
  headers?: Headers,
): ValoriAPIError {
  const apiError = (body ?? {}) as Partial<ApiError>;
  const code =
    typeof apiError.code === "string" ? (apiError.code as ErrorCode | string) : undefined;
  const message =
    (typeof apiError.error === "string" && apiError.error) ||
    (typeof body === "string" && body) ||
    "request failed";
  const requestId = requestIdFrom(headers, body);
  const fields: ValoriErrorFields = { status, code, message, requestId, body, headers };

  if (status === 429) {
    return new RateLimitError({ ...fields, retryAfter: retryAfterFrom(headers) });
  }
  const Ctor = (code && CODE_MAP[code]) || STATUS_MAP[status] || ValoriAPIError;
  return new Ctor(fields);
}
