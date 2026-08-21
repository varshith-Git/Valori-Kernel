// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Code-first OpenAPI generation with `utoipa`.
//!
//! # Why this exists
//!
//! The only sanctioned way to produce `api/openapi/valori-v1.yaml` is:
//!
//! ```text
//! Rust router registrations
//!   -> annotated handlers (#[utoipa::path])
//!   -> public DTOs (#[derive(ToSchema)])
//!   -> utoipa
//!   -> api/openapi/valori-v1.yaml
//! ```
//!
//! Emit it with:
//!
//! ```text
//! cargo run -p valori-node --features utoipa --bin valori-openapi -- \
//!     --output api/openapi/valori-v1.yaml
//! ```
//!
//! Reconstructing `paths` from a route manifest, an inventory document, or the
//! previous YAML is forbidden. A Phase API-3 attempt did exactly that and
//! shipped a contract in which all 79 operations carried the same two
//! placeholder responses and 36 of 40 write endpoints documented no request
//! body at all. See `docs/api/phase-api-3-recovery-audit.md`.
//!
//! # Scope — coverage is measured, never asserted
//!
//! Every public route is annotated as of Phase API-3.1, but that is a fact the
//! tooling establishes on each run, not a claim this comment makes. Run:
//!
//! ```text
//! python3 scripts/verify-api-route-contract.py
//! ```
//!
//! It diffs three independently derived sets — Rust-registered public routes,
//! utoipa-generated operations, and the committed contract's operations — and
//! fails on any discrepancy. `scripts/api-contract-gate.sh` runs it and writes
//! the resulting blocker list to `docs/api/sdk-readiness.json`. Neither number
//! in that file is hand-written.
//!
//! Adding an endpoint means annotating its registered handler and listing it in
//! the `paths(...)` block below. Nothing else makes an operation appear.

use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

use crate::api::{
    BatchInsertRequest, BatchInsertResponse, ClusterHealthStats, CollectionInfo,
    CommunityOverviewEntry, CommunityOverviewResponse, CreateCollectionRequest,
    CreateCollectionResponse, CreateEdgeRequest, CreateEdgeResponse, CreateNodeRequest,
    CreateNodeResponse, DeleteNodeResponse, DeleteRecordRequest, DeleteRecordResponse, EdgeData,
    EngineHealthStats, EventProofResponse, GetEdgesResponse, GetNodeResponse, GraphQueryHitDto,
    GraphQueryResponse, GraphRagHit, GraphRagResponse, GraphRerankRequest, HealthResponse,
    IndexRebuildRequest, IndexRebuildResponse, InsertReceiptJson, InsertRecordRequest,
    InsertRecordResponse, ListCollectionsResponse, ListNodesResponse, MemoryConsolidateRequest,
    MemoryConsolidateResponse, MemoryContradictRequest, MemoryContradictResponse, MemorySearchHit,
    MemorySearchResponse, MemorySearchVectorRequest, MemoryUpsertResponse,
    MemoryUpsertVectorRequest, MetadataGetResponse, MetadataSetRequest, MetadataSetResponse,
    MultiSearchHit, MultiSearchRequest, MultiSearchResponse, NodeInfo, OperationDetails,
    OperationMetrics, OperationOverview, OperationResults, PartialSearchFailure, PoolStatsSchema,
    RecordResponse, RequestId, SearchHit, SearchRequest, SearchResponse, ShardRoutingEntry,
    ShardRoutingResponse, SnapshotRestoreRequest, SnapshotRestoreResponse, SnapshotSaveRequest,
    SnapshotSaveResponse, StateProofResponse, SubgraphEdge, SubgraphNode, SubgraphResponse,
    TimelineEntry, TimelineResponse, UpdateMetadataResponse, UsageResponse, UsageStorage,
};
use crate::api::{OperationDetailResponse, OperationSummary, OperationsListResponse};

/// The canonical error body, as a schema-bearing DTO.
///
/// [`valori_engine::EngineError`] produces this shape at runtime but lives in
/// a crate that does not depend on `utoipa`, so the schema is declared here —
/// the translation layer §36 asks for, rather than a re-export of an internal
/// type.
#[derive(serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct ApiError {
    /// Human-readable message. Do not parse.
    pub error: String,
    /// Stable machine-readable code. Branch on this.
    pub code: ErrorCodeSchema,
}

/// Raw snapshot bytes, as the OpenAPI binary idiom.
///
/// Phase API-3.3: `/v1/snapshot/download` and `/v1/snapshot/upload` were
/// annotated `body = Vec<u8>`, which utoipa renders literally — `type: array,
/// items: {type: integer, format: int32}`. Generators believe it: the
/// throwaway Python client typed the download as `list[int]`, so restoring a
/// snapshot meant round-tripping every byte of a multi-megabyte file through
/// a Python integer list.
///
/// `type: string, format: binary` is the OpenAPI idiom for an opaque byte
/// stream, and generators map it to `bytes` / `Blob` / `File`. The wire format
/// is unchanged — this describes the same octet-stream correctly.
#[derive(utoipa::ToSchema)]
#[schema(value_type = String, format = Binary)]
pub struct SnapshotBytes(#[allow(dead_code)] Vec<u8>);

/// One task's contribution to a [`ReceiptDto`], mirroring
/// [`valori_effect::ReceiptFragment`].
#[derive(serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[schema(as = ReceiptFragment)]
pub struct ReceiptFragmentDto {
    /// Position of this task in the executed graph's topological order.
    pub task_index: u32,
    /// BLAKE3 hex of the kernel state before this task.
    pub state_hash_before: String,
    /// BLAKE3 hex of the kernel state after this task. Equal to `before` for reads.
    pub state_hash_after: String,
    /// True if this task produced kernel writes.
    pub mutated: bool,
    /// BLAKE3 hex of the fragment itself, used for chaining.
    pub fragment_hash: String,
}

/// The unified proof of one completed Operation, as it crosses the wire.
///
/// Mirrors [`valori_effect::Receipt`], which lives in a crate with no `utoipa`
/// dependency — the same translation-layer arrangement as [`ApiError`].
///
/// # Why this type exists
///
/// Phase API-3.3: `GET /v1/proof/receipt` and `GET /v1/proof/receipt/{id}`
/// were annotated `body = Object`, which renders as a bare `type: object` with
/// no properties. Generators produce `object` in TypeScript and
/// `Dict[str, Any]` in Python — so the receipt, which is the entire point of
/// a verifiable memory system, arrived in every SDK as an opaque blob with no
/// discoverable field.
///
/// The handlers return `serde_json::to_value(&Receipt)`, and `Receipt` is a
/// fully concrete struct. Nothing about it was ever unknowable; it simply was
/// not written down.
///
/// `tests/api_contract.rs::receipt_dto_matches_the_runtime_receipt` serialises
/// a real `Receipt` and diffs its key set against this type, so the two cannot
/// drift.
#[derive(serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[schema(as = Receipt)]
pub struct ReceiptDto {
    /// Unique id for this receipt.
    pub receipt_id: String,
    /// Content-addressed BLAKE3 of the receipt, as 32 raw bytes.
    ///
    /// `ReceiptHash` is a `[u8; 32]` newtype, so it crosses the wire as an
    /// array of 32 integers — not the hex string `to_hex()` produces.
    pub receipt_hash: Vec<u8>,
    /// `BLAKE3(kind ‖ inputs ‖ policy)` for the operation.
    pub operation_hash: String,
    /// `BLAKE3(op_hash ‖ fp.hash ‖ ctx_hash ‖ topo_order)` for the task graph.
    pub graph_hash: String,
    /// Kernel ABI the operation ran against.
    pub kernel_abi_version: u32,
    /// `BLAKE3(version ‖ routing_config_hash ‖ feature_flags_hash ‖ schema_version)`.
    pub planner_fingerprint_hash: String,
    /// Whether embedding was enabled on the node that produced this.
    pub embed_enabled: bool,
    /// Whether the producing node was running in cluster mode.
    pub cluster_mode: bool,
    /// Shard count on the producing node.
    pub shard_count: u8,
    /// BLAKE3 hex of kernel state before the operation.
    pub state_hash_before: String,
    /// BLAKE3 hex of kernel state after. Equal to `before` for read-only operations.
    pub state_hash_after: String,
    /// Parent receipt hashes in the Merkle DAG. Empty for a root receipt.
    pub parent_receipts: Vec<Vec<u8>>,
    /// Shard that produced this receipt.
    pub shard_id: u8,
    /// Committed log height at production time.
    pub committed_height: u64,
    /// Unix seconds. Deliberately excluded from `receipt_hash`.
    pub produced_at: u64,
    /// Per-task fragments in topological order.
    pub fragments: Vec<ReceiptFragmentDto>,
}

/// Mirror of [`valori_engine::ErrorCode`] for schema generation.
///
/// `tests/api_contract.rs` diffs the runtime enum against the committed YAML;
/// this type exists so the generated document carries the same closed set.
#[derive(serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = ErrorCode)]
pub enum ErrorCodeSchema {
    ValidationError,
    Unauthorized,
    Forbidden,
    NotFound,
    CollectionNotFound,
    RecordNotFound,
    DimensionMismatch,
    InvalidMetric,
    InvalidIndex,
    IndexBuildFailed,
    Conflict,
    CapacityExceeded,
    NotLeader,
    Unavailable,
    NotImplemented,
    InternalError,
}

// ── Closed domains (Phase API-3.2 §8) ────────────────────────────────────────
//
// `metric` and `index` are closed sets in the runtime — `valori_domain::Metric`
// and `valori_domain::IndexKind` both reject anything they do not recognise —
// but they crossed the wire as bare `type: string`, so a generated SDK offered
// no completion and caught no typo until the server answered 400.
//
// The accepted set and the emitted set are deliberately *different* schemas,
// because they genuinely differ: `FromStr` takes aliases, `as_str` never
// produces them. Modelling responses with the input set would tell a client to
// expect values the server cannot emit.
//
// These are schema-only mirrors, attached with `value_type`. The Rust fields
// stay `Option<String>` so deserialization and the existing `FromStr`
// validation (with its 400s) are untouched — nothing about request handling
// changes.

/// The `metric` values a request may send — canonical plus the aliases
/// [`valori_domain::Metric`]'s `FromStr` accepts.
#[derive(serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = MetricInput)]
pub enum MetricInputSchema {
    /// Canonical spelling. Prefer this.
    SquaredL2,
    /// Alias for `squared_l2`.
    L2,
    /// Alias for `squared_l2`.
    L2sq,
}

/// The `metric` values a response can contain — canonical only, matching
/// [`valori_domain::Metric`]'s `as_str`.
#[derive(serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = Metric)]
pub enum MetricSchema {
    SquaredL2,
}

/// The `index` values a request may send — canonical plus the aliases
/// [`valori_domain::IndexKind`]'s `FromStr` accepts.
///
/// Omitting `index` entirely is a distinct, first-class state (`index = NONE`,
/// exact namespace-scoped search with no dedicated ANN structure) and is not
/// represented here — absence is the representation.
#[derive(serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = IndexKindInput)]
pub enum IndexKindInputSchema {
    Brute,
    /// Alias for `brute`.
    Bruteforce,
    Hnsw,
    Ivf,
    Bq,
    /// Size-driven selection: brute-force under 10k vectors, BQ to 2M, HNSW above.
    Auto,
    /// Alias for `auto`.
    Mstg,
}

/// The `index` values a response can contain — canonical only, matching
/// [`valori_domain::IndexKind`]'s `as_str`.
#[derive(serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = IndexKind)]
pub enum IndexKindSchema {
    Brute,
    Hnsw,
    Ivf,
    Bq,
    Auto,
}

/// Valori's OpenAPI vendor extensions, added after generation (§19).
///
/// # Why this is a `Modify` pass and not part of the annotations
///
/// `utoipa 5.5` models operation-level extensions on
/// [`utoipa::openapi::path::Operation`] but the `#[utoipa::path]` macro has no
/// syntax for setting them. So the extension values are attached here instead.
///
/// # What it is allowed to do
///
/// It **enriches**. For every operation utoipa already generated it adds:
///
/// * `x-required-scope` — read straight out of [`crate::api_keys::required_scope`],
///   the same function the auth middleware calls at request time. The contract
///   therefore cannot claim a scope the server does not enforce.
/// * `x-sdk` — whether the operation is part of the public SDK surface.
///
/// It **never** creates a path, a request body, a response, or an operation.
/// A path absent from `paths(...)` stays absent; there is nothing here that
/// could invent one. That distinction is the whole lesson of the failed Phase
/// API-3 attempt (`docs/api/phase-api-3-recovery-audit.md`).
pub struct VendorExtensionAddon;

impl Modify for VendorExtensionAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use axum::http::Method;
        use utoipa::openapi::extensions::ExtensionsBuilder;
        use utoipa::openapi::path::{Operation, PathItem};

        fn enrich(path: &str, method: Method, op: &mut Operation) {
            // `required_scope` is written against axum paths (`:id`); the
            // document carries OpenAPI paths (`{id}`). None of its rules key
            // on a parameter's spelling, but convert anyway so the two are
            // never accidentally compared in the wrong shape.
            let axum_path = path.replace('{', ":").replace('}', "");

            // Phase API-3.3: only an operation that actually consults
            // credentials gets `x-required-scope`. `GET /health` declares
            // `security: []`, so the auth middleware never runs on it and
            // `required_scope` never gets asked — but this pass used to stamp
            // it with the function's default (`read_only`) anyway. The result
            // told an SDK "this needs a read_only key" about the one endpoint
            // deliberately reachable with no key at all.
            let authenticated = op.security.as_ref().is_some_and(|r| !r.is_empty());
            let mut ext = ExtensionsBuilder::new().add("x-sdk", serde_json::json!(true));
            if authenticated {
                let scope = crate::api_keys::required_scope(&method, &axum_path);
                ext = ext.add("x-required-scope", serde_json::json!(scope.to_string()));
            }
            op.extensions = Some(ext.build());
        }

        fn each(path: &str, item: &mut PathItem) {
            for (method, slot) in [
                (Method::GET, &mut item.get),
                (Method::PUT, &mut item.put),
                (Method::POST, &mut item.post),
                (Method::DELETE, &mut item.delete),
                (Method::OPTIONS, &mut item.options),
                (Method::HEAD, &mut item.head),
                (Method::PATCH, &mut item.patch),
                (Method::TRACE, &mut item.trace),
            ] {
                if let Some(op) = slot.as_mut() {
                    enrich(path, method, op);
                }
            }
        }

        let paths: Vec<String> = openapi.paths.paths.keys().cloned().collect();
        for path in paths {
            if let Some(item) = openapi.paths.paths.get_mut(&path) {
                each(&path, item);
            }
        }
    }
}

/// Owns the two responses the auth middleware — not the handlers — produces.
///
/// # Why this is a `Modify` pass
///
/// `401` and `403` never come from a handler. `auth_guard_v2` in
/// [`crate::server`] (and its twin in [`crate::cluster_server`]) rejects the
/// request before any handler runs, by returning a bare
/// `Err(StatusCode::UNAUTHORIZED)` / `Err(StatusCode::FORBIDDEN)`.
///
/// Declaring them per-handler duplicated a router-layer fact 70 times, so both
/// are attached here instead, once.
///
/// # Phase API-3.3 correction: the body is `ApiError`, not empty
///
/// Phase API-3.2 attached these two responses with **no body**, reasoning that
/// axum renders a bare `StatusCode` with an empty body. That is true of the
/// guard in isolation, but not of the router: both routers install
/// [`crate::error_codes::attach_error_code`] as their **outermost** layer, and
/// that middleware synthesises a full `{"error", "code"}` object for any error
/// response that left the stack with an empty body. So the bytes a client
/// actually receives on 401/403 are canonical `ApiError` JSON.
///
/// `crates/valori-node/tests/api_contract.rs::unauthorized_has_a_parseable_json_body_with_a_code`
/// asserts exactly that, and passes. The contract, not the runtime, was wrong —
/// on 2 responses × every authenticated operation. A generated SDK read
/// `content?: never` and had no type to parse the auth failure into.
///
/// So both responses are attached here, once, from the single place that knows
/// the middleware's actual behaviour. An operation opts in exactly when it
/// carries a non-empty `security` requirement; `GET /health` declares
/// `security: []` and is skipped, which is why it is the one operation in the
/// contract with no 4xx at all (see `redocly.yaml`).
///
/// Like [`VendorExtensionAddon`], this enriches only. It never creates a path,
/// an operation, a request body, or a schema.
pub struct AuthResponsesAddon;

impl Modify for AuthResponsesAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::path::{Operation, PathItem};
        use utoipa::openapi::{ContentBuilder, Ref, ResponseBuilder};

        /// The canonical `{"error", "code"}` body that `attach_error_code`
        /// guarantees on every error response leaving either router.
        fn api_error_content() -> utoipa::openapi::Content {
            ContentBuilder::new()
                .schema(Some(Ref::from_schema_name("ApiError")))
                .build()
        }

        fn apply(op: &mut Operation) {
            // Unauthenticated operations (security: []) cannot 401 or 403.
            // `security(("BearerAuth" = []))` yields a non-empty requirement
            // list; `GET /health` declares an empty one, which renders as
            // `security: []` and means "no credentials are consulted".
            let authenticated = op.security.as_ref().is_some_and(|reqs| !reqs.is_empty());
            if !authenticated {
                return;
            }

            // Replace, not merge: any hand-written 401 is superseded by the
            // router-layer truth, which is that the body is empty.
            op.responses.responses.insert(
                "401".to_string(),
                ResponseBuilder::new()
                    .description(
                        "Missing or invalid credentials. The auth middleware rejects the \
                         request before the handler runs; `attach_error_code` renders the \
                         rejection as a canonical `ApiError` with `code: \"unauthorized\"`.",
                    )
                    .content("application/json", api_error_content())
                    .build()
                    .into(),
            );
            op.responses.responses.insert(
                "403".to_string(),
                ResponseBuilder::new()
                    .description(
                        "The presented key authenticated but its scope does not satisfy this \
                         operation's `x-required-scope`. Rendered as a canonical `ApiError` \
                         with `code: \"forbidden\"`.",
                    )
                    .content("application/json", api_error_content())
                    .build()
                    .into(),
            );
        }

        fn each(item: &mut PathItem) {
            for slot in [
                &mut item.get,
                &mut item.put,
                &mut item.post,
                &mut item.delete,
                &mut item.options,
                &mut item.head,
                &mut item.patch,
                &mut item.trace,
            ] {
                if let Some(op) = slot.as_mut() {
                    apply(op);
                }
            }
        }

        for item in openapi.paths.paths.values_mut() {
            each(item);
        }
    }
}

/// Guarantees, structurally, that every documented error response carries the
/// `ApiError` body the runtime actually sends.
///
/// # Why this is a `Modify` pass and not 16 annotation edits
///
/// This is the contract-side mirror of [`crate::error_codes::attach_error_code`].
/// That middleware is the outermost layer on both routers and rewrites *any*
/// error response — handler-built JSON, a bare `StatusCode`, an empty body —
/// into the canonical `{"error", "code"}` object. So at runtime the body of a
/// `>= 400` response is `ApiError` unconditionally.
///
/// Phase API-3.2 tried to keep the contract in step by hand, and left 16
/// responses declaring no `content`. Two of them (`POST /v1/tree/*`,
/// `POST /v1/ingest/document`) are annotated in `valori-rag` and
/// `valori-ingest`, which cannot even name [`ApiError`] — it is declared in
/// this crate. Hand-editing was therefore not only lossy, it was impossible to
/// complete at the call sites.
///
/// Fixing it here instead means the contract cannot drift from the middleware:
/// a new endpoint that forgets `body = ApiError` gets the correct body anyway,
/// exactly as it gets the correct `code` at runtime without asking.
///
/// Enriches only. A response that already declares `content` is left untouched,
/// so a handler documenting a *more specific* error schema keeps it.
pub struct ErrorBodyAddon;

/// The one case where a `>= 400` response genuinely has no body.
///
/// `attach_error_code` passes through a non-empty **non-JSON** body untouched.
/// Nothing in the public surface relies on that any more (Phase API-3.3
/// converged `GET /v1/crypto/status/{key_id}`, the last `text/plain` error),
/// so this list is empty — and the emptiness is the point: it is checked, not
/// assumed.
const NO_ERROR_BODY: &[(&str, &str)] = &[];

impl Modify for ErrorBodyAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::path::Operation;
        use utoipa::openapi::{ContentBuilder, Ref, RefOr};

        fn apply(path: &str, method: &str, op: &mut Operation) {
            for (code, resp) in op.responses.responses.iter_mut() {
                // Only error statuses. 2xx/3xx bodies are the handler's own
                // business and a 204 must stay empty.
                if !code.parse::<u16>().is_ok_and(|c| c >= 400) {
                    continue;
                }
                if NO_ERROR_BODY.contains(&(path, method)) {
                    continue;
                }
                let RefOr::T(r) = resp else { continue };
                if !r.content.is_empty() {
                    continue; // already documented — never override
                }
                r.content.insert(
                    "application/json".to_string(),
                    ContentBuilder::new()
                        .schema(Some(Ref::from_schema_name("ApiError")))
                        .build(),
                );
            }
        }

        for (path, item) in openapi.paths.paths.iter_mut() {
            let path = path.clone();
            let slots: [(&str, &mut Option<Operation>); 8] = [
                ("get", &mut item.get),
                ("put", &mut item.put),
                ("post", &mut item.post),
                ("delete", &mut item.delete),
                ("options", &mut item.options),
                ("head", &mut item.head),
                ("patch", &mut item.patch),
                ("trace", &mut item.trace),
            ];
            for (method, slot) in slots {
                if let Some(op) = slot.as_mut() {
                    apply(&path, method, op);
                }
            }
        }
    }
}

/// Declares the `BearerAuth` scheme once, so annotated operations can reference
/// it by name instead of restating it.
pub struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "BearerAuth",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .description(Some(
                        "Project API key or the legacy VALORI_AUTH_TOKEN bearer token.",
                    ))
                    .build(),
            ),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Valori Data Plane API",
        version = "1.0.0",
        description = "The Valori Data Plane REST API v1, emitted directly from \
                       `#[utoipa::path]` annotations on the registered axum handlers. \
                       Every public route the Rust routers register appears here and \
                       nothing else does — `scripts/verify-api-route-contract.py` \
                       proves that equality on every run of the contract gate, and \
                       fails on any drift in either direction. Administrative and \
                       operator-internal routes (key management, cluster membership, \
                       replication streams, `/metrics`) are deliberately excluded: \
                       they are served by the node but are not part of the SDK \
                       surface."
    ),
    servers((url = "/", description = "The node this document was generated by. \
                                      Valori is self-hosted: point your client at \
                                      your own node's base URL.")),
    // ErrorBodyAddon runs last: it fills in the `ApiError` body on any error
    // response the passes above left bodyless, and never overrides one they set.
    modifiers(
        &SecurityAddon,
        &VendorExtensionAddon,
        &AuthResponsesAddon,
        &ErrorBodyAddon
    ),
    paths(
        // ── meta ────────────────────────────────────────────────────────────
        crate::server::health_check,
        crate::routes::version,
        crate::server::usage_handler,
        crate::server::shard_routing_handler,
        crate::server::models_health,
        // ── collections ─────────────────────────────────────────────────────
        crate::server::create_collection_handler,
        crate::server::list_collections_handler,
        crate::server::drop_collection_handler,
        // ── index lifecycle ─────────────────────────────────────────────────
        crate::server::index_lifecycle_status_handler,
        crate::server::index_lifecycle_create_handler,
        crate::server::index_config_handler,
        crate::server::index_rebuild_handler,
        // ── records ─────────────────────────────────────────────────────────
        crate::server::insert_record,
        crate::server::batch_insert,
        crate::server::get_record_by_id,
        crate::server::update_record_metadata,
        crate::server::delete_record,
        crate::server::soft_delete_record,
        crate::server::insert_encrypted_handler,
        // ── search ──────────────────────────────────────────────────────────
        crate::server::search,
        crate::server::multi_search,
        // ── graph ───────────────────────────────────────────────────────────
        crate::server::create_node,
        crate::server::get_node,
        crate::server::delete_node,
        crate::server::list_nodes,
        crate::server::create_edge,
        crate::server::get_edges,
        crate::server::get_subgraph,
        crate::server::graph_query,
        crate::server::graphrag,
        // ── memory ──────────────────────────────────────────────────────────
        crate::server::memory_upsert_vector,
        crate::server::memory_upsert_vector_alias,
        crate::server::memory_search_vector,
        crate::server::memory_search_vector_alias,
        crate::server::memory_consolidate,
        crate::server::memory_contradict,
        crate::server::meta_set,
        crate::server::meta_get,
        // ── operations ──────────────────────────────────────────────────────
        crate::server::get_operations,
        crate::server::get_operation_by_id,
        crate::server::get_operation_execution,
        // ── proof ───────────────────────────────────────────────────────────
        crate::server::get_proof,
        crate::server::get_event_proof,
        crate::server::get_latest_receipt,
        crate::server::get_receipt_by_id,
        crate::server::get_timeline,
        // ── snapshot / storage ──────────────────────────────────────────────
        crate::server::snapshot,
        crate::server::restore,
        crate::server::snapshot_save,
        crate::server::snapshot_restore,
        crate::server::list_remote_snapshots,
        crate::server::upload_snapshot_to_store,
        crate::server::restore_from_store,
        crate::server::get_manifest,
        crate::server::list_remote_wal,
        crate::server::archive_wal_segment,
        // ── ingest ──────────────────────────────────────────────────────────
        crate::ingest::ingest,
        crate::ingest::ingest_update,
        crate::ingest::get_ingest_status,
        valori_ingest::handler::ingest_document,
        // ── tree-RAG ────────────────────────────────────────────────────────
        crate::server::tree_build,
        crate::server::tree_query,
        crate::server::tree_hybrid,
        valori_rag::tree::tree_verify,
        valori_rag::tree::tree_chain_verify,
        // ── community ───────────────────────────────────────────────────────
        crate::server::community_detect,
        crate::server::community_search,
        crate::server::community_overview,
        crate::server::extract_entities,
        // ── crypto ──────────────────────────────────────────────────────────
        crate::server::crypto_status_handler,
        // ── cluster ─────────────────────────────────────────────────────────
        crate::cluster_api::status,
        crate::cluster_api::role,
        crate::cluster_api::health,
        crate::cluster_server::cluster_proof,
    ),
    components(schemas(
        // ── errors ──────────────────────────────────────────────────────────
        ApiError,
        SnapshotBytes,
        ReceiptDto,
        ReceiptFragmentDto,
        crate::ingest::IngestJobStatusResponse,
        crate::ingest::IngestJobState,
        ErrorCodeSchema,
        // ── closed domains (§8) ─────────────────────────────────────────────
        MetricSchema,
        MetricInputSchema,
        IndexKindSchema,
        IndexKindInputSchema,
        // ── meta / health ───────────────────────────────────────────────────
        HealthResponse,
        EngineHealthStats,
        ClusterHealthStats,
        PoolStatsSchema,
        UsageResponse,
        UsageStorage,
        ShardRoutingResponse,
        ShardRoutingEntry,
        valori_models::health::SystemHealth,
        valori_models::health::PackageHealth,
        valori_models::health::PackageHealthStatus,
        // ── collections ─────────────────────────────────────────────────────
        CreateCollectionRequest,
        CreateCollectionResponse,
        ListCollectionsResponse,
        CollectionInfo,
        // ── index lifecycle ─────────────────────────────────────────────────
        valori_engine::index_manager::IndexBuildRequest,
        valori_engine::index_manager::IndexBuildParameters,
        valori_engine::index_manager::BuildableIndexKind,
        valori_engine::index_manager::IndexStatusResponse,
        crate::server::IndexConfigResponse,
        crate::server::HnswConfigView,
        IndexRebuildRequest,
        IndexRebuildResponse,
        // ── records ─────────────────────────────────────────────────────────
        RequestId,
        InsertRecordRequest,
        InsertRecordResponse,
        InsertReceiptJson,
        BatchInsertRequest,
        BatchInsertResponse,
        RecordResponse,
        UpdateMetadataResponse,
        DeleteRecordRequest,
        DeleteRecordResponse,
        crate::server::InsertEncryptedRequest,
        crate::server::InsertEncryptedResponse,
        crate::server::CryptoStatusResponse,
        // ── search ──────────────────────────────────────────────────────────
        SearchRequest,
        SearchHit,
        SearchResponse,
        GraphRerankRequest,
        MultiSearchRequest,
        MultiSearchResponse,
        MultiSearchHit,
        PartialSearchFailure,
        // ── graph ───────────────────────────────────────────────────────────
        CreateNodeRequest,
        CreateNodeResponse,
        GetNodeResponse,
        DeleteNodeResponse,
        NodeInfo,
        ListNodesResponse,
        CreateEdgeRequest,
        CreateEdgeResponse,
        EdgeData,
        GetEdgesResponse,
        GraphQueryHitDto,
        GraphQueryResponse,
        SubgraphResponse,
        SubgraphNode,
        SubgraphEdge,
        GraphRagHit,
        OperationOverview,
        OperationResults,
        OperationMetrics,
        OperationDetails,
        crate::server::GraphRagRequest,
        GraphRagResponse,
        // ── memory ──────────────────────────────────────────────────────────
        MemoryUpsertVectorRequest,
        MemoryUpsertResponse,
        MemorySearchVectorRequest,
        MemorySearchHit,
        MemorySearchResponse,
        MemoryConsolidateRequest,
        MemoryConsolidateResponse,
        MemoryContradictRequest,
        MemoryContradictResponse,
        MetadataSetRequest,
        MetadataSetResponse,
        MetadataGetResponse,
        // ── operations ──────────────────────────────────────────────────────
        OperationSummary,
        OperationsListResponse,
        OperationDetailResponse,
        crate::execution_registry::ExecutionRecord,
        crate::execution_registry::StageView,
        valori_ingest::execution::StageName,
        valori_ingest::execution::StageMetrics,
        // ── proof / timeline ────────────────────────────────────────────────
        StateProofResponse,
        EventProofResponse,
        TimelineEntry,
        TimelineResponse,
        // ── snapshot / storage ──────────────────────────────────────────────
        SnapshotSaveRequest,
        SnapshotSaveResponse,
        SnapshotRestoreRequest,
        SnapshotRestoreResponse,
        crate::server::ListRemoteSnapshotsResponse,
        crate::server::StorageSnapshotUploadResponse,
        crate::server::RestoreFromStoreRequest,
        crate::server::RestoreFromStoreResponse,
        crate::server::ManifestResponse,
        crate::server::ListRemoteWalResponse,
        crate::server::ArchiveWalRequest,
        crate::server::ArchiveWalResponse,
        valori_storage::object_store::SnapshotEntry,
        valori_storage::object_store::WalEntry,
        valori_storage::object_store::SnapshotManifest,
        // ── ingest ──────────────────────────────────────────────────────────
        crate::ingest::IngestRequest,
        crate::ingest::IngestResponse,
        crate::ingest::IngestUpdateRequest,
        crate::ingest::IngestUpdateResponse,
        valori_ingest::handler::IngestDocumentRequest,
        valori_ingest::handler::IngestDocumentResponse,
        valori_ingest::chunker::IngestChunk,
        // ── tree-RAG ────────────────────────────────────────────────────────
        valori_rag::tree::BuildRequest,
        valori_rag::tree::BuildResponse,
        valori_rag::tree::QueryRequest,
        valori_rag::tree::AnswerResult,
        valori_rag::tree::VerifyRequest,
        valori_rag::tree::VerifyResponse,
        valori_rag::tree::HybridRequest,
        valori_rag::tree::HybridResponse,
        valori_rag::tree::HybridHit,
        valori_rag::tree::ChainVerifyRequest,
        valori_rag::tree::ChainVerifyResponse,
        valori_rag::tree::TreeIndex,
        valori_rag::tree::TreeNode,
        valori_rag::tree::StructureNode,
        valori_rag::tree::Citation,
        valori_rag::tree::Receipt,
        // ── community ───────────────────────────────────────────────────────
        valori_rag::community::DetectRequest,
        valori_rag::community::DetectResponse,
        valori_rag::community::CommunitySummary,
        valori_rag::community::SearchRequest,
        valori_rag::community::SearchResponse,
        valori_rag::community::CommunityHit,
        valori_rag::community::ExtractEntitiesRequest,
        valori_rag::community::ExtractEntitiesResponse,
        valori_rag::community::InsertedEntity,
        valori_rag::community::InsertedRelationship,
        CommunityOverviewResponse,
        CommunityOverviewEntry,
        // ── cluster ─────────────────────────────────────────────────────────
        crate::cluster_api::StatusView,
        crate::cluster_api::MemberView,
        crate::cluster_api::ClusterRoleResponse,
        crate::cluster_api::ClusterHealthResponse,
        crate::cluster_server::ClusterProofResponse,
    ))
)]
pub struct ValoriApi;

/// Every schema name this module currently generates. The conformance test
/// asserts each one exists in the committed contract.
pub fn generated_schema_names() -> Vec<String> {
    let doc = ValoriApi::openapi();
    let mut names: Vec<String> = doc
        .components
        .map(|c| c.schemas.keys().cloned().collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// The generated document as YAML.
///
/// # Why this does not call `utoipa`'s own `to_yaml`
///
/// `utoipa::openapi::extensions::Extensions` stores vendor extensions in a
/// `HashMap<String, serde_json::Value>` flattened into the operation object.
/// Rust's `HashMap` iterates in a per-process random order, so rendering
/// straight from the `OpenApi` struct puts `x-required-scope` and `x-sdk` in a
/// different order on every run — the document is semantically identical but
/// not byte-identical, and the contract gate's reproducibility check (rightly)
/// fails on that.
///
/// Going through `serde_json::Value` first fixes it: `serde_json::Map` is a
/// `BTreeMap` in this build (no `preserve_order` feature anywhere in the
/// tree), so every object comes out key-sorted and therefore identical across
/// runs and across machines.
///
/// This is a rendering detail. It adds nothing, removes nothing, and cannot
/// invent a path — it re-serialises exactly the document utoipa produced.
pub fn to_yaml() -> Result<String, Box<dyn std::error::Error>> {
    let value = serde_json::to_value(ValoriApi::openapi())?;
    Ok(serde_norway::to_string(&value)?)
}
