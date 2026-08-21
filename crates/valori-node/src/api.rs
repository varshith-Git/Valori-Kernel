// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use serde::{Deserialize, Serialize};

// Note: We need to make sure valori-kernel exports NodeKind/EdgeKind publicly or we redefine/wrap them.
// Since valori-kernel is a dependency, we can use its types if they are pub.
// Assuming valori_kernel::types::enums::* is pub.

// ── Collections / namespace seam ─────────────────────────────────────────────
//
// The API accepts a `collection` string on every data-path request.
// Phase 4 wires this to a live NamespaceRegistry; today the only registered
// collection is "default" (NamespaceId 0).  Any other name returns 400 so
// clients get a clear error rather than silently landing in the wrong bucket.

pub use valori_kernel::types::id::{NamespaceId, DEFAULT_NS};

/// Name of the default (always-existing) collection.
pub const DEFAULT_COLLECTION: &str = "default";

// `resolve_namespace` is intentionally removed — server handlers call
// `engine.resolve_collection(name)` directly so the live registry is consulted.

/// Not wired to any live handler — real handlers call
/// `engine.resolve_collection(name)` directly so the live registry (not a
/// static string check) decides. Kept only for its own unit tests, which
/// pin the intended contract as documentation.
///
/// Phase 3.3: `"default"` has no special meaning — this can no longer
/// unconditionally accept `None`/`Some("default")`, since neither is
/// guaranteed to resolve to anything on a real engine. This function has
/// no registry to consult, so the honest thing it can still assert is
/// syntactic: a name (if given) is well-formed. Whether it actually
/// resolves is entirely the registry's call, made by `resolve_collection`.
pub fn validate_collection(collection: Option<&str>) -> Result<(), crate::errors::EngineError> {
    match collection {
        Some(name) if name.is_empty() => Err(crate::errors::EngineError::InvalidInput(
            "collection name cannot be empty".into(),
        )),
        _ => Ok(()),
    }
}

// ── Idempotency token (Phase API-2) ──────────────────────────────────────────

/// A 16-byte client idempotency token.
///
/// Two spellings existed on the wire before this phase and both are still
/// accepted, normalised here to the one canonical in-memory form:
///
/// * a **16-element byte array** — what cluster `POST /v1/records` has always
///   taken and what the Python SDK sends (`list(idempotency_key)`);
/// * a **32-character hex string**, optionally dash-separated as a UUID —
///   what `POST /v1/vectors/batch-insert` has always taken in `request_ids`.
///
/// Accepting both is what lets one `InsertRecordRequest` serve both routers
/// without breaking either existing client. Anything else — wrong length,
/// non-hex, wrong JSON type — is a hard deserialisation error, never a
/// silently-dropped field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestId(pub [u8; 16]);

impl RequestId {
    /// Parse the string spelling: 32 hex chars, with or without UUID dashes.
    pub fn parse_hex(s: &str) -> Option<Self> {
        let cleaned: String = s.chars().filter(|c| *c != '-').collect();
        if cleaned.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 16];
        for (i, chunk) in cleaned.as_bytes().chunks(2).enumerate() {
            bytes[i] = u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
        }
        Some(RequestId(bytes))
    }

    pub fn to_hex(self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

impl<'de> Deserialize<'de> for RequestId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Bytes([u8; 16]),
            Hex(String),
        }
        match Wire::deserialize(d)? {
            Wire::Bytes(b) => Ok(RequestId(b)),
            Wire::Hex(s) => RequestId::parse_hex(&s).ok_or_else(|| {
                D::Error::custom(
                    "request_id must be 32 hex characters (UUID dashes optional) \
                     or an array of exactly 16 bytes",
                )
            }),
        }
    }
}

impl Serialize for RequestId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

// Hand-written because the type accepts two JSON shapes; `derive(ToSchema)`
// cannot express that from a newtype over `[u8; 16]`.
#[cfg(feature = "utoipa")]
impl utoipa::PartialSchema for RequestId {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        use utoipa::openapi::schema::{
            ArrayBuilder, ObjectBuilder, OneOfBuilder, SchemaType, Type,
        };
        OneOfBuilder::new()
            .item(
                ObjectBuilder::new()
                    .schema_type(SchemaType::Type(Type::String))
                    .description(Some("32 hex characters; UUID dashes optional.")),
            )
            .item(
                ArrayBuilder::new()
                    .items(ObjectBuilder::new().schema_type(SchemaType::Type(Type::Integer)))
                    .min_items(Some(16))
                    .max_items(Some(16)),
            )
            .description(Some(
                "16-byte client idempotency token. A replay returns the record the \
                 first request created, with `deduplicated: true`.",
            ))
            .into()
    }
}

#[cfg(feature = "utoipa")]
impl utoipa::ToSchema for RequestId {
    fn name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("RequestId")
    }
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
/// **The** public request body for `POST /v1/records` — one model, both routers.
///
/// Phase API-2 merged the two divergent bodies that existed before:
/// standalone accepted `{values, collection, text}` and silently discarded
/// everything else; cluster accepted `{values, collection, metadata, tag,
/// request_id}` and silently discarded `text`. Every field below is now
/// honoured on **both** paths.
#[derive(Deserialize)]
pub struct InsertRecordRequest {
    pub values: Vec<f32>,
    #[serde(default)]
    pub collection: Option<String>,
    /// Optional raw text for BM25 hybrid reranking. When provided, stored
    /// in the reranker index alongside the vector so future searches can
    /// use term-frequency scoring to reorder results.
    #[serde(default)]
    pub text: Option<String>,
    /// Opaque per-record metadata bytes, committed inside the `InsertRecord`
    /// event and therefore covered by the BLAKE3 audit chain.
    #[serde(default)]
    pub metadata: Option<Vec<u8>>,
    /// Opaque user tag stored alongside the record.
    #[serde(default)]
    pub tag: u64,
    /// Client idempotency token. Replaying the same token inside the dedup
    /// window returns the record the first request created and performs no
    /// second write.
    #[serde(default)]
    pub request_id: Option<RequestId>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct InsertReceiptJson {
    pub record_id: u32,
    pub old_root: String,
    pub new_root: String,
    pub proof: String,
    pub sequence: u64,
    pub timestamp: u64,
    pub state_hash: String,
}

impl From<valori_kernel::proof::InsertReceipt> for InsertReceiptJson {
    fn from(r: valori_kernel::proof::InsertReceipt) -> Self {
        let hex = |b: &[u8; 32]| b.iter().map(|x| format!("{:02x}", x)).collect::<String>();
        InsertReceiptJson {
            record_id: r.record_id,
            old_root: hex(&r.old_root),
            new_root: hex(&r.new_root),
            proof: hex(&r.proof),
            sequence: r.sequence,
            timestamp: r.timestamp,
            state_hash: hex(&r.state_hash),
        }
    }
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
/// **The** public response body for `POST /v1/records` — one model, both routers.
///
/// `log_index` is Raft-only and omitted in standalone; `deduplicated` is
/// present on both paths and is `true` exactly when the request carried a
/// `request_id` that had already been applied, in which case `id` is the
/// record the original request created and no new write happened.
#[derive(Serialize)]
pub struct InsertRecordResponse {
    pub id: u32,
    /// Raft log index of the committed write — cluster path only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_index: Option<u64>,
    /// `true` when this request was recognised as a replay of a previous
    /// `request_id` and no new record was created.
    pub deduplicated: bool,
    pub receipt: InsertReceiptJson,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Deserialize)]
pub struct DeleteRecordRequest {
    pub id: u32,
    #[serde(default)]
    pub collection: Option<String>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct DeleteRecordResponse {
    pub success: bool,
    /// Raft log index of the committed write — cluster path only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_index: Option<u64>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Deserialize)]
pub struct SearchRequest {
    pub query: Vec<f32>,
    pub k: usize,
    #[serde(default)]
    pub collection: Option<String>,
    /// ISO 8601 UTC timestamp — search the vector state as it existed at this moment.
    /// Requires the event log to be enabled (`VALORI_EVENT_LOG_PATH`).
    #[serde(default)]
    pub as_of: Option<String>,
    /// Log index — search the vector state after exactly this many committed events.
    /// Mutually exclusive with `as_of`; `as_of_log_index` takes precedence if both given.
    #[serde(default)]
    pub as_of_log_index: Option<u64>,
    /// Phase C4.1 — recency half-life in seconds. When set (> 0), results are
    /// re-ranked so older records decay: a record one half-life old has its L2
    /// distance doubled. `0`/absent uses the server default (or pure distance).
    /// Ignored for `as_of` / point-in-time queries.
    #[serde(default)]
    pub decay_half_life_secs: Option<u64>,
    /// BM25 hybrid reranking. When `true` (default), the server fetches
    /// `k × POOL_FACTOR` candidates by vector similarity and re-ranks them by
    /// a 50/50 blend of normalised vector score + BM25 term-frequency score
    /// before returning the top-k. Requires `query_text` to be set.
    /// Set to `false` to get pure vector ranking (legacy behaviour).
    #[serde(default = "default_rerank")]
    pub rerank: bool,
    /// The raw query string used for BM25 scoring. Required when `rerank=true`.
    /// Ignored when `rerank=false`.
    #[serde(default)]
    pub query_text: Option<String>,
    /// Optional JSON object whose key-value pairs must ALL be present (and equal)
    /// in a record's metadata for the record to be returned.
    /// Numeric values support optional range operators: `{"gte": 2020, "lte": 2024}`.
    /// Example: `{"author": "Alice", "year": {"gte": 2020}}`
    #[serde(default)]
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<Object>))]
    pub metadata_filter: Option<serde_json::Map<String, serde_json::Value>>,
    /// G1.4.1 — optional graph-aware reranking. Presence enables it;
    /// absence is a complete no-op (identical to pre-G1.4.1 behavior).
    /// Composes with either `rerank` (BM25) or `decay_half_life_secs` — it
    /// runs as a final pass over whatever score the rest of the pipeline
    /// already produced, using each hit's existing `score` as the base.
    #[serde(default)]
    pub graph_rerank: Option<GraphRerankRequest>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Deserialize, Debug, Clone)]
pub struct GraphRerankRequest {
    /// Number of top vector hits to resolve as graph seeds. Clamped
    /// `[1, 10]` server-side (never rejected).
    #[serde(default = "default_graph_rerank_seed_count")]
    pub seed_count: usize,
    /// Multiplier weight per hop of graph distance. Clamped `[0.0, 1.0]`
    /// server-side. `adjusted = score * (1 + weight * distance)`.
    #[serde(default = "default_graph_rerank_weight")]
    pub weight: f32,
    /// `"outgoing"` (default) | `"incoming"` | `"both"` — case-insensitive,
    /// same convention as `GET /v1/graph/query`.
    #[serde(default)]
    pub direction: Option<String>,
    /// Max hop count from the seed set. Clamped to `query_graph`'s own
    /// `MAX_DEPTH` (4), never rejected.
    #[serde(default = "default_graph_rerank_depth")]
    pub max_depth: u32,
}

// ── Phase 5: Cross-Collection (Multi) Search ─────────────────────────────────
//
// `POST /v1/search/multi` fans out the query to each listed Collection
// independently, then merges results globally by Squared L2 (smaller = better).
// All Collections must share the same `dim` and `metric`; different index types
// (HNSW/IVF/None) are allowed within the same request.
//
// Score semantics: raw Squared L2 distance — no normalization, no flipping.
// BM25 reranking is intentionally excluded: hybrid scores from different
// Collection corpora are not comparable and would distort the merge.
// Graph reranking is excluded: graph edges are Collection-scoped.

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
/// Request body for `POST /v1/search/multi`.
#[derive(Deserialize)]
pub struct MultiSearchRequest {
    /// Query vector. Must match the shared dimension of all listed collections.
    pub query: Vec<f32>,
    /// Number of global top-k results to return.
    pub k: usize,
    /// One or more collection names. All must share the same `dim` and `metric`.
    pub collections: Vec<String>,
    /// Phase C4.1 — decay half-life in seconds. Applied per-collection before merge.
    #[serde(default)]
    pub decay_half_life_secs: Option<u64>,
    /// Metadata predicate applied per-collection after vector search.
    #[serde(default)]
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<Object>))]
    pub metadata_filter: Option<serde_json::Map<String, serde_json::Value>>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
/// A single result from a multi-collection search, annotated with its source.
#[derive(Serialize)]
pub struct MultiSearchHit {
    /// The collection this record lives in.
    pub collection: String,
    /// Record ID within the collection.
    pub id: u32,
    /// Squared L2 distance to the query (smaller = closer).
    pub score: f32,
    /// Phase C4.1 — applied decay factor in (0, 1]. Present only when decay is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decay_factor: Option<f32>,
    /// Age of the record in seconds. Present only when decay is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_secs: Option<u64>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
/// Response for `POST /v1/search/multi`.
#[derive(Serialize)]
pub struct MultiSearchResponse {
    /// Global top-k hits sorted by score ascending (smaller = closer).
    pub results: Vec<MultiSearchHit>,
    /// Names of all collections included in this query.
    pub collections_searched: Vec<String>,
    /// Runtime failures from individual collections, if any.
    /// Present only when at least one collection's search failed after
    /// dimension/metric compatibility was confirmed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_failures: Option<Vec<PartialSearchFailure>>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
/// A per-collection runtime failure reported in `MultiSearchResponse`.
#[derive(Serialize)]
pub struct PartialSearchFailure {
    pub collection: String,
    pub error: String,
}

fn default_rerank() -> bool {
    true
}

/// G1.4.1 — see docs/reviews/graph-g1.4.1-graph-aware-reranking-design.md.
fn default_graph_rerank_seed_count() -> usize {
    1
}
fn default_graph_rerank_weight() -> f32 {
    0.15
}
fn default_graph_rerank_depth() -> u32 {
    2
}

// Metadata predicate matching now lives in valori-search.
pub use valori_search::matches_metadata_filter;

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct SearchHit {
    pub id: u32,
    pub score: f32,
    /// Phase C4.1 — applied decay factor in (0, 1]. Present only when decay is
    /// active. `score` stays the true (undecayed) L2 distance for honesty;
    /// ranking reflects `score / decay_factor`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decay_factor: Option<f32>,
    /// Age of the record in seconds at query time. Present only when decay is
    /// active and the record's creation time is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_secs: Option<u64>,
    /// G1.4.1 — hop distance to the nearest `graph_rerank` seed. Present
    /// only when `graph_rerank` was requested. `None` within that means
    /// the candidate has no graph node, or is unreachable within
    /// `max_depth` — never causes a candidate to be dropped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_distance: Option<u32>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchHit>,
    /// Present only for as-of searches: the log index of the replayed state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of_log_index: Option<u64>,
    /// Unix-second wall-clock timestamp of the `as_of_log_index` event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of_timestamp_unix: Option<u64>,
    /// ISO 8601 string of `as_of_timestamp_unix`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of_timestamp_iso: Option<String>,
    /// BLAKE3 hex hash of the kernel state at `as_of_log_index`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of_state_hash: Option<String>,
}

impl SearchResponse {
    pub fn simple(results: Vec<SearchHit>) -> Self {
        Self {
            results,
            as_of_log_index: None,
            as_of_timestamp_unix: None,
            as_of_timestamp_iso: None,
            as_of_state_hash: None,
        }
    }
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
/// A single entry in the timeline — one committed kernel event with its metadata.
#[derive(Serialize)]
pub struct TimelineEntry {
    /// Sequential index within this entry's shard log (0-based).
    /// Used as a tie-breaker when two shards share the same `timestamp_unix`.
    pub log_index: u64,
    /// Shard that committed this event. Always 0 in standalone mode.
    pub shard_id: u32,
    /// Unix-second wall-clock timestamp when this event was committed.
    pub timestamp_unix: u64,
    /// ISO 8601 UTC string for `timestamp_unix`.
    pub timestamp_iso: String,
    /// Human-readable event kind.
    pub event_type: &'static str,
    /// Record ID if this is a record-level event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_id: Option<u32>,
    /// Node ID if this is a graph-node event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<u32>,
    /// Edge ID if this is a graph-edge event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_id: Option<u32>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct TimelineResponse {
    pub events: Vec<TimelineEntry>,
    pub total: usize,
    /// Inclusive lower bound filter applied (unix seconds), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_unix: Option<u64>,
    /// Inclusive upper bound filter applied (unix seconds), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_unix: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct OperationSummary {
    /// Canonical v1 operation identity. Always a string (§13).
    pub id: String,
    #[serde(rename = "type")]
    pub op_type: String,
    pub status: String,
    pub timing: String,
    pub timestamp_unix: u64,
    pub collection: String,
    /// Addressing for the underlying committed event.
    #[cfg_attr(feature = "utoipa", schema(value_type = OperationDetails))]
    pub details: serde_json::Value,
}

/// The `details` block of [`OperationSummary`].
///
/// `shard_id` is populated on the cluster path only — standalone has no shard
/// dimension, so it is absent there rather than defaulted to a fictitious `0`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct OperationDetails {
    /// Position in the committed event log.
    pub log_index: Option<u64>,
    /// Cluster mode only — the shard whose log this event came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shard_id: Option<u8>,
    /// Set when the event touched a record.
    pub record_id: Option<u32>,
    /// Set when the event touched a graph node.
    pub node_id: Option<u32>,
    /// Set when the event touched a graph edge.
    pub edge_id: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct OperationsListResponse {
    pub operations: Vec<OperationSummary>,
    pub total: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct OperationDetailResponse {
    /// Canonical v1 operation identity. Always a string (§13).
    pub id: String,
    #[serde(rename = "type")]
    pub op_type: String,
    pub status: String,
    pub timing: String,
    pub timestamp_unix: u64,
    pub collection: String,
    /// Identity and addressing for the operation.
    #[cfg_attr(feature = "utoipa", schema(value_type = OperationOverview))]
    pub overview: serde_json::Value,
    /// What the operation changed.
    #[cfg_attr(feature = "utoipa", schema(value_type = OperationResults))]
    pub results: serde_json::Value,
    /// The proof of the state transition.
    ///
    /// When a receipt was assembled for this operation this is a full
    /// [`crate::openapi::ReceiptDto`]. When one was not — a receipt store is
    /// in-process and does not survive a restart — the node synthesises a
    /// reduced stand-in carrying `receipt_id`, `status`, `operation_hash`,
    /// `state_hash_before` and `state_hash_after`. Because the two shapes
    /// genuinely differ, this is documented as an open object rather than
    /// claiming a single schema that only sometimes holds.
    #[cfg_attr(feature = "utoipa", schema(value_type = std::collections::HashMap<String, serde_json::Value>))]
    pub proof: serde_json::Value,
    /// Resource cost of the operation.
    #[cfg_attr(feature = "utoipa", schema(value_type = OperationMetrics))]
    pub metrics: serde_json::Value,
}

/// The `overview` block of [`OperationDetailResponse`].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct OperationOverview {
    pub id: String,
    #[serde(rename = "type")]
    pub op_type: String,
    pub status: String,
    pub timing: String,
    pub collection: String,
    /// Position in the committed event log.
    pub log_index: Option<u64>,
    /// Set when the operation touched a record.
    pub record_id: Option<u32>,
    /// Set when the operation touched a graph node.
    pub node_id: Option<u32>,
    /// Set when the operation touched a graph edge.
    pub edge_id: Option<u32>,
}

/// The `results` block of [`OperationDetailResponse`].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct OperationResults {
    /// Commit outcome, e.g. `committed`.
    pub status: String,
    pub records_affected: u32,
    pub nodes_affected: u32,
    pub edges_affected: u32,
    /// Human-readable summary. Do not parse.
    pub message: String,
}

/// The `metrics` block of [`OperationDetailResponse`].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct OperationMetrics {
    pub duration_ms: f64,
    pub memory_bytes: u64,
    pub cpu_cycles: u64,
    /// Qualitative assessment, e.g. `optimal`.
    pub status: String,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Deserialize)]
pub struct CreateNodeRequest {
    pub record_id: Option<u32>,
    // NodeKind needs to be deserializable.
    // valori-kernel NodeKind derives Copy, Clone, Debug, PartialEq. Does it derive Serialize/Deserialize?
    // If not, we need a mirror enum or manual impl.
    // The user didn't ask to modify kernel.
    // So we must redefine or use `#[serde(remote = ...)]`?
    // Or just "kind": u8 ?
    // User request: "You can define NodeKind and EdgeKind via valori-kernel’s enums (they are #[repr(u8)] + serde)."
    // Ah, the user implied they *are* serde?
    // Or I should make them serde in kernel?
    // "Do NOT modify valori-kernel".
    // "You can define NodeKind ... via valori-kernel's enums (they are #[repr(u8)] + serde)" -> Maybe the user thinks they are serde?
    // Or maybe "You can define [your own API types] via ..."
    // I will redefine them here for serde support if kernel ones don't have it.
    // Let's assume for now I wrap them: kind: u8 in JSON, mapped to enum.
    pub kind: u8,
    #[serde(default)]
    pub collection: Option<String>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct CreateNodeResponse {
    pub node_id: u32,
    /// Raft log index of the committed write — cluster path only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_index: Option<u64>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Deserialize)]
pub struct CreateEdgeRequest {
    pub from: u32,
    pub to: u32,
    pub kind: u8,
    #[serde(default)]
    pub collection: Option<String>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct CreateEdgeResponse {
    pub edge_id: u32,
    /// Raft log index of the committed write — cluster path only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_index: Option<u64>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct GetNodeResponse {
    pub kind: u8,
    pub record_id: Option<u32>,
    pub namespace_id: u16,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct EdgeData {
    pub edge_id: u32,
    pub to_node: u32,
    pub kind: u8,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct GetEdgesResponse {
    pub edges: Vec<EdgeData>,
}

// ── G1.1 — deterministic graph query primitives ─────────────────────────────

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct GraphQueryHitDto {
    pub node_id: u32,
    pub kind: u8,
    pub record_id: Option<u32>,
    pub depth: u32,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct GraphQueryResponse {
    pub hits: Vec<GraphQueryHitDto>,
    pub count: usize,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Deserialize)]
pub struct MemoryUpsertVectorRequest {
    pub vector: Vec<f32>,
    #[serde(default)]
    pub collection: Option<String>,
    pub attach_to_document_node: Option<u32>,
    // Reserved for future use:
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<Object>))]
    pub metadata: Option<serde_json::Value>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct MemoryUpsertResponse {
    pub memory_id: String,
    pub record_id: u32,
    pub document_node_id: u32,
    pub chunk_node_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_index: Option<u64>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Deserialize)]
pub struct MemorySearchVectorRequest {
    pub query_vector: Vec<f32>,
    pub k: usize,
    #[serde(default)]
    pub collection: Option<String>,
    /// Phase C4.1 — recency half-life (seconds). When set (> 0), the agent-memory
    /// recall path re-ranks older memories down. See `SearchRequest`.
    #[serde(default)]
    pub decay_half_life_secs: Option<u64>,
    /// Phase S6 (cluster mode only; ignored standalone): `"local"` skips
    /// the read-index round trip (eventually consistent, faster). Absent
    /// or any other value defaults to linearizable, matching `/v1/search`.
    #[serde(default)]
    pub consistency: Option<String>,
    /// Phase I7 — restrict results to records whose stored metadata satisfies
    /// every key/value predicate. Same semantics as `SearchRequest::metadata_filter`.
    #[serde(default)]
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<Object>))]
    pub metadata_filter: Option<serde_json::Map<String, serde_json::Value>>,
    /// Phase C5 — when `true` (default) and `query_text` is provided, re-ranks
    /// candidates by hybrid BM25 + vector score before returning the top-k.
    #[serde(default = "crate::api::default_rerank")]
    pub rerank: bool,
    /// Phase C5 — raw query text for BM25 hybrid re-ranking. Required when
    /// `rerank=true`; ignored otherwise.
    #[serde(default)]
    pub query_text: Option<String>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct NodeInfo {
    pub node_id: u32,
    pub kind: u8,
    pub record_id: Option<u32>,
    pub namespace_id: u16,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct ListNodesResponse {
    pub nodes: Vec<NodeInfo>,
    pub count: usize,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct DeleteNodeResponse {
    pub success: bool,
    /// Raft log index of the committed write — cluster path only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_index: Option<u64>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize, Deserialize)]
pub struct MemorySearchHit {
    pub memory_id: String,
    pub record_id: u32,
    pub score: f32,
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<Object>))]
    pub metadata: Option<serde_json::Value>,
    /// Phase C4.1 — applied decay factor in (0, 1]; present only when decay is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decay_factor: Option<f32>,
    /// Phase C4.1 — record age in seconds; present only when decay is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_secs: Option<u64>,
}

// ... existing content ...

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize, Deserialize)]
pub struct MemorySearchResponse {
    pub results: Vec<MemorySearchHit>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Deserialize, Serialize, Debug)]
pub struct MetadataSetRequest {
    pub target_id: String,
    /// Arbitrary caller-supplied JSON object. Valori stores it verbatim and
    /// never interprets it, so this is genuinely open-ended — but
    /// `additionalProperties` makes that explicit, so a generator emits
    /// `Record<string, unknown>` / `Dict[str, Any]` rather than a bare
    /// property-less `object` that says nothing at all.
    #[cfg_attr(feature = "utoipa", schema(value_type = std::collections::HashMap<String, serde_json::Value>))]
    pub metadata: serde_json::Value,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize, Debug)]
pub struct MetadataSetResponse {
    pub success: bool,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Deserialize, Serialize, Debug)]
pub struct MetadataGetRequest {
    pub target_id: String,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize, Debug)]
pub struct MetadataGetResponse {
    pub target_id: String,
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<Object>))]
    pub metadata: Option<serde_json::Value>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Deserialize, Serialize, Debug)]
pub struct SnapshotSaveRequest {
    // Optional path override. If None, uses configured snapshot path.
    pub path: Option<String>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize, Debug)]
pub struct SnapshotSaveResponse {
    pub success: bool,
    pub path: String,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Deserialize, Serialize, Debug)]
pub struct SnapshotRestoreRequest {
    // Path to load from.
    pub path: String,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize, Debug)]
pub struct SnapshotRestoreResponse {
    pub success: bool,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
// Phase 26: Event log proof API
#[derive(Serialize, Debug)]
pub struct EventProofResponse {
    pub kernel_version: u32,
    pub event_log_hash: String,        // hex-encoded BLAKE3
    pub final_state_hash: String,      // hex-encoded BLAKE3
    pub snapshot_hash: Option<String>, // hex-encoded BLAKE3 (if snapshot exists)
    pub event_count: u64,
    pub committed_height: u64,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
// Phase 34: Batch Ingestion
#[derive(Deserialize, Serialize, Debug)]
pub struct BatchInsertRequest {
    pub batch: Vec<Vec<f32>>,
    #[serde(default)]
    pub collection: Option<String>,
    /// Optional per-vector metadata blobs (UTF-8 JSON strings).
    /// If present, must be the same length as `batch`.
    /// Each entry is committed inside the `InsertRecord` event and is
    /// therefore included in the BLAKE3 audit chain.
    #[serde(default)]
    pub metadata: Option<Vec<Option<String>>>,
    /// Per-item idempotency keys (32-hex strings = 16-byte UUIDs).
    /// If present, must be the same length as `batch`. A null entry means
    /// "no dedup key for this item". A repeated key causes that item to be
    /// skipped and the previously assigned ID is returned instead.
    #[serde(default)]
    pub request_ids: Option<Vec<Option<String>>>,
    /// Optional per-vector text strings for BM25 hybrid reranking.
    /// If present, must be the same length as `batch`. A null entry means
    /// no text is stored for that vector. Text is tokenised and indexed
    /// so that future /search calls with `rerank=true` can re-score results.
    #[serde(default)]
    pub texts: Option<Vec<Option<String>>>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize, Deserialize, Debug)]
pub struct BatchInsertResponse {
    pub ids: Vec<u32>,
}

// ── Collection (namespace) management ────────────────────────────────────────

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Deserialize, Debug)]
pub struct CreateCollectionRequest {
    pub name: String,
    /// Vector dimension for this collection. **Required for every name** —
    /// `"default"` included, with no exception (Phase 3.3). Valori does not
    /// infer a collection's dimension from its first insert or from any
    /// project/env-level default; `POST` without it is rejected with 400.
    /// Immutable after creation; a later request with a different value for
    /// the same collection is rejected, not silently applied.
    ///
    /// The Rust field stays `Option` so that a missing value reaches
    /// `parse_collection_config` and is answered with that explanatory 400
    /// rather than a generic deserialization failure. `required = true`
    /// records the contract-level truth the handler enforces, so a generated
    /// SDK makes the argument mandatory instead of silently omittable.
    #[serde(default)]
    #[cfg_attr(feature = "utoipa", schema(required = true))]
    pub dimension: Option<u32>,
    /// Distance metric. **Required for every name** — `"default"` included,
    /// with no exception (Phase 3.3). Sent explicitly over the wire, never
    /// assumed. Only `"squared_l2"` is supported today; present as a field
    /// (not hard-coded) so the wire contract does not need to change when a
    /// second metric is added.
    ///
    /// See `dimension` for why this is `Option` in Rust but `required` in the
    /// contract.
    #[serde(default)]
    #[cfg_attr(feature = "utoipa", schema(
        required = true,
        value_type = Option<crate::openapi::MetricInputSchema>,
        example = "squared_l2",
    ))]
    pub metric: Option<String>,
    /// Index algorithm: `"brute"`, `"hnsw"`, `"ivf"`, `"bq"`, or `"auto"`.
    /// Optional — omitting it means `index = NONE` (exact
    /// namespace-specific search, no dedicated ANN structure), a
    /// first-class supported state, not a missing feature. Requires
    /// `dimension` to also be set — a collection cannot have an explicit
    /// index without an explicit dimension, since the index is constructed
    /// for that dimension at creation time.
    #[serde(default)]
    #[cfg_attr(feature = "utoipa", schema(
        value_type = Option<crate::openapi::IndexKindInputSchema>,
        example = "hnsw",
    ))]
    pub index: Option<String>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize, Debug)]
pub struct CollectionInfo {
    pub name: String,
    pub id: u16,
    /// Present only for collections created with an explicit vector config.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimension: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<crate::openapi::MetricSchema>))]
    pub metric: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<crate::openapi::IndexKindSchema>))]
    pub index: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_records: Option<usize>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize, Debug)]
pub struct CreateCollectionResponse {
    pub name: String,
    pub id: u16,
    pub created: bool,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize, Debug)]
pub struct ListCollectionsResponse {
    pub collections: Vec<CollectionInfo>,
}

// ── C4.2: Memory consolidation ───────────────────────────────────────────────

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
/// Replace an existing memory record with a new vector, committing a
/// SoftDeleteRecord + AutoInsertRecord + AutoCreateEdge(Supersedes) to the
/// BLAKE3 audit chain in one logical operation.
#[derive(Deserialize)]
pub struct MemoryConsolidateRequest {
    /// Record id of the memory being replaced.
    pub old_record_id: u32,
    /// New vector that replaces the old memory.
    pub new_vector: Vec<f32>,
    #[serde(default)]
    pub collection: Option<String>,
    #[serde(default)]
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<Object>))]
    pub metadata: Option<serde_json::Value>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct MemoryConsolidateResponse {
    /// The old record id (now soft-deleted).
    pub old_record_id: u32,
    /// The new record id.
    pub new_record_id: u32,
    /// The Supersedes edge id linking new → old.
    pub supersedes_edge_id: u32,
    /// BLAKE3 state hash after all three events are applied.
    pub state_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_index: Option<u64>,
}

// ── C4.3: Contradiction detection ────────────────────────────────────────────

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
/// Check whether two records contradict each other (by cosine similarity
/// threshold) and, if so, commit a Contradicts edge to the audit chain.
#[derive(Deserialize)]
pub struct MemoryContradictRequest {
    pub record_a: u32,
    pub record_b: u32,
    /// Cosine similarity threshold above which the records are deemed to
    /// contradict. Default 0.85 — tuned for claim-level NLI in Q16.16 space.
    #[serde(default)]
    pub threshold: Option<f32>,
    #[serde(default)]
    pub collection: Option<String>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct MemoryContradictResponse {
    pub record_a: u32,
    pub record_b: u32,
    pub similarity: f32,
    pub contradicts: bool,
    /// Edge id of the Contradicts edge, present only when contradicts=true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_id: Option<u32>,
    pub state_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_index: Option<u64>,
}

// ── Phase API-3: Unified Additive Health DTO ─────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct HealthResponse {
    pub status: String,
    pub mode: String,
    pub version: String,

    // Standalone legacy fields (additive compatibility)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collections: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persistence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<PoolStatsSchema>))]
    pub records: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<PoolStatsSchema>))]
    pub nodes: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<PoolStatsSchema>))]
    pub edges: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_log_height: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_provider: Option<String>,
    pub shard_count: usize,

    // Cluster legacy fields (additive compatibility)
    //
    // `leader` and `dim` are the two top-level fields the pre-API-3 cluster
    // `/health` emitted that `ui/src/lib/hooks/useHealth.ts` still reads. They
    // are kept at the top level, not only inside `cluster`, because §14 of the
    // API contract phases forbids removing a field a live consumer depends on
    // for the sake of a tidier schema.
    /// Node id of the leader this node currently sees. Cluster mode only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leader: Option<u64>,
    /// The vector dimension the cluster has locked to, or the configured
    /// dimension when nothing has been inserted yet. Cluster mode only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dim: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leader_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub term: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raft_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub members: Option<usize>,

    // Sub-objects for clean OpenAPI model
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<EngineHealthStats>))]
    pub engine: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<ClusterHealthStats>))]
    pub cluster: Option<serde_json::Value>,
}

// ── Phase API-3.1: schema mirrors for the health sub-objects (§14) ───────────
//
// `HealthResponse` keeps `serde_json::Value` for the additive sub-objects so no
// legacy field can be dropped by a type change. These mirrors exist purely so
// the generated contract describes those objects instead of emitting a bare
// `object`. They are asserted against the runtime shape by
// `tests/api_contract.rs::health_subobjects_match_schema_mirrors`.

/// Slab occupancy for one kernel pool (records, graph nodes, graph edges).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct PoolStatsSchema {
    /// Live (non-tombstoned) entries.
    pub live: usize,
    /// Slab slots consumed, including tombstones.
    pub slots_used: usize,
    /// Configured slab capacity.
    pub capacity: usize,
    /// `live / capacity` as a percentage, rounded to one decimal.
    pub fill_pct: f64,
}

/// The `engine` sub-object of `GET /health`. Standalone mode only; absent in
/// cluster mode, where the node has no single in-process engine to describe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct EngineHealthStats {
    pub status: String,
    pub version: String,
    pub collections: usize,
    /// `event_log`, `wal`, `snapshot`, or `none`.
    pub persistence: String,
    pub records: PoolStatsSchema,
    pub nodes: PoolStatsSchema,
    pub edges: PoolStatsSchema,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_log_height: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_log_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_path: Option<String>,
    pub embed_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_provider: Option<String>,
    pub shard_count: usize,
}

/// The `cluster` sub-object of `GET /health`. Cluster mode only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ClusterHealthStats {
    /// `ok` when this node sees an elected leader, `no-leader` otherwise.
    pub status: String,
    /// Node id of the leader this node currently sees, if any.
    pub leader: Option<u64>,
    /// Vector dimension the cluster locked on first insert, if any.
    pub dim: Option<u32>,
    /// Raft role of this node (`Leader`, `Follower`, `Candidate`, `Learner`).
    pub role: String,
    pub term: u64,
}

// ── Phase API-3.1: public DTOs for endpoints that previously answered with an
//    untyped `serde_json::Value` body ─────────────────────────────────────────

/// `GET /v1/records/{id}` — one stored record, decoded back to `f32`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct RecordResponse {
    pub id: u32,
    /// The stored Q16.16 vector, converted back to `f32` for the wire.
    /// Round-tripping is lossy at the Q16.16 quantum, by design.
    pub vector: Vec<f32>,
    /// Whatever JSON was committed alongside the record, if any.
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<Object>))]
    pub metadata: Option<serde_json::Value>,
    pub tag: u64,
}

/// `PATCH /v1/records/{id}/metadata`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct UpdateMetadataResponse {
    pub ok: bool,
    pub id: u32,
}

/// `GET /v1/proof/state` — the running BLAKE3 Merkle root over all applied
/// events. Identical wire shape in standalone and cluster mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct StateProofResponse {
    /// 64 lowercase hex characters (32 bytes).
    pub final_state_hash: String,
}

/// The `storage` sub-object of `GET /v1/usage`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct UsageStorage {
    /// Live event-log segment plus every rotated archive segment.
    pub event_log_bytes: u64,
    pub snapshot_bytes: u64,
    pub total_bytes: u64,
}

/// `GET /v1/usage` — raw counters only. The node is plan-agnostic: it never
/// returns quota, plan, or billing context.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct UsageResponse {
    pub records: usize,
    pub collections: usize,
    pub storage: UsageStorage,
}

/// One shard's collection assignment in `GET /v1/shard/routing`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ShardRoutingEntry {
    pub shard: usize,
    pub collections: Vec<String>,
}

/// `GET /v1/shard/routing` — which collection lives on which logical shard.
/// Routing is `namespace_id % shard_count`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ShardRoutingResponse {
    /// `standalone` or `cluster`.
    pub mode: String,
    pub shard_count: usize,
    pub shards: Vec<ShardRoutingEntry>,
}

/// `POST /v1/index/rebuild` request body. Project-wide rebuild; per-collection
/// lifecycle lives at `/v1/namespaces/{name}/index`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct IndexRebuildRequest {
    /// Echoed back as `effective`. Present for forward compatibility; the
    /// standalone rebuild always rebuilds every per-collection index.
    #[serde(default)]
    #[cfg_attr(feature = "utoipa", schema(
        value_type = Option<crate::openapi::IndexKindInputSchema>,
    ))]
    pub index: Option<String>,
}

/// `POST /v1/index/rebuild` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct IndexRebuildResponse {
    pub ok: bool,
    /// The `index` value from the request, or `"rebuilt"` when it was absent.
    pub effective: String,
    /// Live record count after the rebuild.
    pub records: usize,
}

/// One node in an expanded subgraph.
///
/// Phase API-3.3: `SubgraphResponse.nodes` was `Vec<Object>` — an array of
/// property-less objects, which is `object[]` in TypeScript. The producer,
/// `valori_rag::graph::expand_subgraph`, emits a fixed three-key object; this
/// records it. Note the keys are `id`/`record`, not the `node_id`/`record_id`
/// that [`NodeInfo`] uses — the two shapes are genuinely different and must
/// not be conflated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct SubgraphNode {
    /// Graph node id.
    pub id: u32,
    /// `NodeKind` discriminant.
    pub kind: u8,
    /// The record this node represents, when it represents one.
    pub record: Option<u32>,
}

/// One edge in an expanded subgraph, as emitted by
/// `valori_rag::graph::expand_subgraph`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct SubgraphEdge {
    /// Graph edge id.
    pub id: u32,
    /// Source node id.
    pub from: u32,
    /// Target node id.
    pub to: u32,
    /// `EdgeKind` discriminant.
    pub kind: u8,
}

/// `GET /v1/graph/subgraph` — a BFS expansion around one root node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct SubgraphResponse {
    #[cfg_attr(feature = "utoipa", schema(value_type = Vec<SubgraphNode>))]
    pub nodes: serde_json::Value,
    #[cfg_attr(feature = "utoipa", schema(value_type = Vec<SubgraphEdge>))]
    pub edges: serde_json::Value,
}

/// One blended vector + graph hit from `POST /v1/graphrag`.
///
/// Phase API-3.3: `GraphRagResponse.hits` was `Vec<Object>`, so GraphRAG — a
/// headline retrieval feature — returned `object[]` to every generated SDK.
/// The producer is `capabilities.rs`, which builds a fixed ten-key object.
///
/// Several scores are nullable by design: a hit reached purely through graph
/// expansion has no vector distance, so `score` and `vector_score` are `null`
/// on it. `final_score` and `graph_score` are always present.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GraphRagHit {
    /// Stable memory identity, `rec:<record_id>`.
    pub memory_id: String,
    /// The underlying record.
    pub record_id: u32,
    /// Vector distance. `null` for a graph-only hit. Retained for backward
    /// compatibility; `vector_score` is the explicit spelling of the same value.
    pub score: Option<f32>,
    /// Vector distance. `null` for a graph-only hit.
    pub vector_score: Option<f32>,
    /// Normalised graph relevance in `[0, 1]`.
    pub graph_score: f32,
    /// Combined score in `[0, 1]`. Always present; rank on this.
    pub final_score: f32,
    /// Graph node for this record, when it has one.
    pub node_id: Option<u32>,
    /// Hop count from the nearest seed node, when reachable.
    pub graph_distance: Option<u32>,
    /// How this hit entered the result set — e.g. `vector`, `graph`.
    pub source: String,
    /// Caller-supplied metadata stored alongside the record, if any.
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<std::collections::HashMap<String, serde_json::Value>>))]
    pub metadata: Option<serde_json::Value>,
}

/// `POST /v1/graphrag` — K nearest vectors plus the connected subgraph around
/// them, read from one consistent kernel snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct GraphRagResponse {
    /// Blended vector + graph hits, best first.
    #[cfg_attr(feature = "utoipa", schema(value_type = Vec<GraphRagHit>))]
    pub hits: serde_json::Value,
    /// Graph node ids the vector hits seeded the expansion from.
    pub seed_nodes: Vec<u32>,
    pub subgraph: SubgraphResponse,
}

/// One community in `GET /v1/community/overview`, largest first.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct CommunityOverviewEntry {
    pub community_id: u32,
    pub member_count: usize,
    /// Mean vector of the community's members.
    pub centroid: Vec<f32>,
    /// Up to 10 member node ids, for a cheap preview.
    pub sample_node_ids: Vec<u32>,
}

/// `GET /v1/community/overview` — requires `POST /v1/community/detect` first.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct CommunityOverviewResponse {
    pub community_count: usize,
    pub node_count: usize,
    /// BLAKE3 receipt over the sorted community assignment.
    pub receipt: String,
    pub communities: Vec<CommunityOverviewEntry>,
}
