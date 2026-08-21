# Valori Public API Error Taxonomy & Request ID Idempotency Contract

## 1. Error Taxonomy Architecture

Valori uses a closed, machine-readable `ErrorCode` taxonomy for all HTTP error responses on both Standalone and Raft Cluster paths.

Every JSON error response emitted by the node carries the canonical structure:
```json
{
  "error": "human readable message string",
  "code": "machine_readable_error_code"
}
```

### Response-Layer Guarantee (`attach_error_code` Middleware)

To prevent hand-rolled error responses or framework-generated status codes (such as bare 401/403 responses) from lacking a structured body:
1. `crates/valori-node/src/error_codes.rs` installs `attach_error_code` response middleware on both standalone (`server.rs`) and cluster (`cluster_server.rs`) routers.
2. If a handler emits a custom error via `error_response(status, code, message)` or `EngineError::parts()`, its specific `code` is preserved.
3. If an error response lacks a `code` field (or is an empty body bare status), `attach_error_code` automatically injects a `code` derived from the HTTP status code.
4. Non-JSON responses (such as `GET /v1/version` text or Prometheus `/metrics`) pass through untouched.

### Compile-Time Exhaustiveness Invariant

Every `EngineError` and `KernelError` variant converts to `(StatusCode, ErrorCode, String)` in `EngineError::parts()` using an exhaustive Rust `match` statement. There are **no wildcard `_ => ...` fallback branches**. Adding a new internal error variant without assigning it a public `ErrorCode` results in a **compile error**.

---

## 2. Canonical Error Code Mapping Table

| ErrorCode String | Enum Variant | Target HTTP Status | Primary Origin | Public Meaning |
|------------------|--------------|-------------------|----------------|----------------|
| `validation_error` | `ValidationError` | `400 BAD_REQUEST` | `EngineError::InvalidInput`, `KernelError::QueryOutOfRange`, `KernelError::MetadataTooLarge` | Request input is malformed, out of Q16.16 range, or invalid. |
| `unauthorized` | `Unauthorized` | `401 UNAUTHORIZED` | `attach_error_code` middleware | Bearer token is missing, invalid, or unparseable. |
| `forbidden` | `Forbidden` | `403 FORBIDDEN` | Auth scope verification | Token is valid but lacks required scope (`read_only` vs `read_write` vs `admin`). |
| `not_found` | `NotFound` | `404 NOT_FOUND` | `KernelError::NotFound` | Generic item (node, edge, timeline record) not found. |
| `collection_not_found` | `CollectionNotFound` | `404 NOT_FOUND` | `EngineError::CollectionNotFound` | Addressed Collection does not exist. |
| `record_not_found` | `RecordNotFound` | `404 NOT_FOUND` | Record pool lookup | The specified `record_id` was not found in the Collection. |
| `dimension_mismatch` | `DimensionMismatch` | `400 BAD_REQUEST` | `KernelError::DimensionMismatch` | Input vector length disagrees with Collection dimension. |
| `invalid_metric` | `InvalidMetric` | `400 BAD_REQUEST` | Index configuration | Specified distance metric is unsupported. |
| `invalid_index` | `InvalidIndex` | `400 BAD_REQUEST` | Index configuration | Specified index algorithm/kind is unsupported. |
| `index_build_failed` | `IndexBuildFailed` | `500 INTERNAL_SERVER_ERROR` | Async index worker | Asynchronous background index build failed. |
| `conflict` | `Conflict` | `409 CONFLICT` | `EngineError::Conflict`, `KernelError::NamespaceAlreadyConfigured` | Request conflicts with immutable state. |
| `capacity_exceeded` | `CapacityExceeded` | `507 INSUFFICIENT_STORAGE` | `KernelError::CapacityExceeded`, `CommitError::Capacity` | Record, node, or edge slab capacity reached. |
| `not_leader` | `NotLeader` | `307 TEMPORARY_REDIRECT` | `CommitError::NotLeader` | Node is follower; redirects to Raft leader. |
| `unavailable` | `Unavailable` | `503 SERVICE_UNAVAILABLE` | Network or cluster election | Leader unavailable or node shutting down. |
| `not_implemented` | `NotImplemented` | `501 NOT_IMPLEMENTED` | `KernelError::NotImplemented` | Capability not implemented in current version. |
| `internal_error` | `InternalError` | `500 INTERNAL_SERVER_ERROR` | `EngineError::Internal`, `KernelError::Overflow` | Unhandled internal runtime failure. |

---

## 3. Request ID Idempotency & Retention Model

### Model: Idempotent Within Retained Request-ID Window

Valori implements bounded deduplication for record creation on both Standalone and Cluster execution paths.

- **Request ID Format**: 16-byte UUID / binary array (`[u8; 16]`) passed in JSON payload as `request_id` or hex string.
- **Standalone Execution Path**: `Engine` maintains `batch_seen: HashMap<[u8; 16], u32>` (capped at 65,536 entries).
  - First receipt: record allocated, `(request_id -> record_id)` stored in `batch_seen`.
  - Replayed receipt (within window): original `record_id` returned directly without duplicate allocation.
  - Expiry / Eviction: `batch_seen` is cleared wholesale once capacity reaches 65,536 entries.
  - Restart Behavior: `batch_seen` is process-scoped and ephemeral; in-flight tokens are forgotten across process restarts.
- **Cluster Execution Path**: Raft state machine replicates `dedup_map` across all peers inside `StateMachineInner::dedup_map`.
  - Replicated state ensures exact-once record creation across cluster failovers within the retained window.
- **Scoping Invariant**: Request ID deduplication is scoped per node process instance in Standalone mode and per Raft group / Shard instance in Cluster mode.
