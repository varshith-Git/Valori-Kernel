// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Public surface of `@valori/client-sdk`.
//
// Layering (Phase API-4A §4):
//
//     src/        handwritten — ergonomics, retry, error mapping, polling
//         ↓
//     generated/  machine output from api/openapi/valori-v1.yaml — DO NOT EDIT
//         ↓
//     fetch
//
// The arrow never points the other way: generated code must not import `src/`.

export { ValoriClient } from "./client.js";
export type { ValoriClientOptions } from "./client.js";

export {
  Collection,
  CollectionIndexResource,
  Collections,
  Graph,
  Memory,
  Records,
  TERMINAL_INDEX_STATES,
} from "./resources/collection.js";
export type {
  GraphRagOptions,
  MemorySearchOptions,
  SearchOptions,
  WaitOptions,
} from "./resources/collection.js";

export {
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
export type { ChunkOptions } from "./resources/node.js";

export { FAILED_STATES, Operation, Operations, TERMINAL_STATES } from "./resources/operations.js";

export {
  AuthenticationError,
  AuthorizationError,
  BadRequestError,
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
  OperationFailedError,
  OperationTimeoutError,
  RateLimitError,
  RecordNotFoundError,
  ServerError,
  ServiceUnavailableError,
  ValidationError,
  ValoriAPIError,
  ValoriConfigError,
  ValoriConnectionError,
  ValoriError,
  ValoriTimeoutError,
  errorFor,
} from "./errors.js";

export {
  DEFAULT_RETRY_POLICY,
  IDEMPOTENCY_HEADER,
  delayFor,
  isRetryableRequest,
  resolveRetryPolicy,
  shouldRetryStatus,
  withRetry,
} from "./retry.js";
export type { RetryPolicy } from "./retry.js";

export { Transport } from "./transport.js";
export type { CallOptions, TransportOptions } from "./transport.js";

export {
  API_CONTRACT_VERSION,
  MAX_SUPPORTED_API_CONTRACT,
  MIN_SUPPORTED_API_CONTRACT,
  VERSION,
  checkApiCompatibility,
} from "./version.js";

// Wire types (request/response models) come straight from the contract.
export * from "../generated/valori-api.js";
