// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Phase API-2: this file used to be a hand-written mirror of the Rust API
// response shapes — a byte-identical duplicate of `ui/src/types/valori.ts`,
// with the same drift from the wire (a `SearchResponse.state_hash` the node
// never emits, a `queried_at` produced by the host app's own BFF route, a
// `ClusterStatus.converged` that is derived in `useCluster`).
//
// Both copies are gone. The wire model comes from `@valori/api-types`,
// generated from `api/openapi/valori-v1.yaml` by
// `scripts/generate-api-types.sh`. The aliases below keep this package's
// public type surface (re-exported from `src/index.ts`) unchanged.

export type {
  Collection,
  Error as ApiError,
  ErrorCode,
  Health as HealthResponse,
  ClusterStatus,
  SearchRequest,
  SearchResponse,
  SearchHit as SearchResult,
  MultiSearchHit,
  InsertRecordRequest,
  InsertRecordResponse,
  IndexStatus,
  ProofResponse,
  PoolStats,
  // View models — explicitly NOT wire types.
  AnnotatedSearchResponse,
  ClusterView,
} from "@valori/api-types";
