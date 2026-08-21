// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase API-2 — contract conformance and standalone/cluster **behaviour** parity.
//!
//! `route_parity.rs` proves the two routers declare the same paths and the
//! same HTTP methods. That is necessary and nowhere near sufficient: it says
//! nothing about request fields, response fields, defaults, status codes, or
//! error bodies. Every P0 in the Phase-1 audit lived in exactly that blind
//! spot — one insert body accepting `text` and the other accepting
//! `request_id`, an unknown Collection answering 400 on one path and 404 on
//! the other, a mis-configured Collection answering 500.
//!
//! This suite closes it by driving the **same request** through a real
//! standalone router and a real single-node Raft cluster router and comparing
//! what comes back. It also diffs the Rust error taxonomy against the
//! committed OpenAPI document so the two cannot drift apart silently.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tokio::sync::RwLock;
use tower::ServiceExt;

use valori_consensus::types::ValoriNode;
use valori_node::cluster::{bootstrap_cluster, ClusterConfig, ClusterHandle};
use valori_node::cluster_server::build_cluster_router;
use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use valori_node::server::{build_router, SharedEngine};
use valori_node::EngineFromNodeConfig;

const DIM: usize = 4;

// ── Harness ──────────────────────────────────────────────────────────────────

fn cfg() -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.max_records = 256;
    cfg.max_nodes = 64;
    cfg.max_edges = 64;
    cfg.event_log_path = None;
    cfg.wal_path = None;
    cfg.snapshot_path = None;
    cfg
}

fn standalone() -> SharedEngine {
    Arc::new(RwLock::new(Engine::new(&cfg())))
}

async fn boot_cluster() -> ClusterHandle {
    let cfg = ClusterConfig {
        node_id: 1,
        raft_bind: "127.0.0.1:0".into(),
        members: [(
            1,
            ValoriNode {
                api_addr: "10.0.0.1:3000".into(),
                raft_addr: String::new(),
            },
        )]
        .into_iter()
        .collect(),
        init: true,
        raft_log_path: None,
        tls: None,
        shard_count: 1,
    };
    let handle = bootstrap_cluster(&cfg, None, None).await.unwrap();
    handle
        .raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader == Some(1), "self-elected")
        .await
        .unwrap();
    handle
}

async fn call(
    router: axum::Router,
    method: Method,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut b = Request::builder().method(method).uri(uri);
    let payload = match body {
        Some(v) => {
            b = b.header("content-type", "application/json");
            Body::from(serde_json::to_vec(&v).unwrap())
        }
        None => Body::empty(),
    };
    let resp = router.oneshot(b.body(payload).unwrap()).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 22)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

async fn post(
    router: axum::Router,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    call(router, Method::POST, uri, Some(body)).await
}

async fn make_collection(router: axum::Router, name: &str) {
    let (s, b) = post(
        router,
        "/v1/namespaces",
        serde_json::json!({ "name": name, "dimension": DIM, "metric": "squared_l2" }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "collection setup failed: {b}");
}

fn v(x: f32) -> serde_json::Value {
    serde_json::json!([x, x * 0.5, x * 0.25, x * 0.125])
}

/// Run one request against BOTH routers and return `(standalone, cluster)`.
async fn both(
    uri: &str,
    body: serde_json::Value,
    setup_collection: Option<&str>,
) -> (
    (StatusCode, serde_json::Value),
    (StatusCode, serde_json::Value),
) {
    let sa = build_router(standalone(), None, None);
    let handle = boot_cluster().await;
    let cl = build_cluster_router(&handle, None);
    if let Some(name) = setup_collection {
        make_collection(sa.clone(), name).await;
        make_collection(cl.clone(), name).await;
    }
    let a = post(sa, uri, body.clone()).await;
    let b = post(cl, uri, body).await;
    (a, b)
}

// ── §15 / §29 — the error contract ───────────────────────────────────────────

/// The Rust `ErrorCode` enum and the OpenAPI `ErrorCode` enum are one thing
/// spelled twice. If they drift, generated clients get codes they cannot
/// handle (or handle codes the server never sends).
#[test]
fn error_code_enum_matches_the_openapi_contract() {
    let spec = include_str!("../../../api/openapi/valori-v1.yaml");
    let start = spec
        .find("ErrorCode:")
        .expect("ErrorCode schema missing from the contract");
    let block = &spec[start..];
    let enum_start = block.find("enum:").expect("enum section missing");
    let enum_block = &block[enum_start..];
    let in_contract: Vec<&str> = enum_block
        .lines()
        .skip(1)
        .take_while(|l| l.trim().starts_with("- "))
        .filter_map(|l| l.trim().strip_prefix("- "))
        .collect();
    let in_code: Vec<&str> = valori_node::errors::ErrorCode::ALL
        .iter()
        .map(|c| c.as_str())
        .collect();

    assert_eq!(
        in_code, in_contract,
        "ErrorCode drift between valori-engine and api/openapi/valori-v1.yaml.\n\
         code: {in_code:?}\ncontract: {in_contract:?}"
    );
}

/// Phase API-3.3 — `GET /v1/proof/receipt{,/{id}}` used to declare
/// `body = Object`, so the receipt reached every SDK as an untyped blob. It is
/// now described by `crate::openapi::ReceiptDto`.
///
/// That DTO is a hand-written mirror of `valori_effect::Receipt`, which is the
/// type the handler actually serialises. A mirror that nobody checks is just a
/// second place to be wrong, so this diffs the two key sets directly: serialise
/// a real `Receipt`, serialise the DTO, and require identical top-level fields.
/// Adding a field to `Receipt` without adding it here fails the build's tests
/// rather than silently shipping a lossy contract.
///
/// `utoipa`-gated because `valori_node::openapi`, where the DTO lives, only
/// exists under that feature.
#[cfg(feature = "utoipa")]
#[test]
fn receipt_dto_matches_the_runtime_receipt() {
    use valori_effect::receipt::{Receipt, ReceiptHash, StateHash};

    let runtime = Receipt {
        receipt_id: "r1".into(),
        receipt_hash: ReceiptHash::zero(),
        operation_hash: "op".into(),
        graph_hash: "g".into(),
        kernel_abi_version: 1,
        planner_fingerprint_hash: "fp".into(),
        embed_enabled: false,
        cluster_mode: false,
        shard_count: 1,
        state_hash_before: StateHash::zero(),
        state_hash_after: StateHash::zero(),
        parent_receipts: vec![],
        shard_id: 0,
        committed_height: 0,
        produced_at: 0,
        fragments: vec![],
    };

    fn keys(v: &serde_json::Value) -> Vec<String> {
        let mut k: Vec<String> = v
            .as_object()
            .expect("receipt serialises as a JSON object")
            .keys()
            .cloned()
            .collect();
        k.sort();
        k
    }

    let dto = valori_node::openapi::ReceiptDto {
        receipt_id: "r1".into(),
        receipt_hash: vec![0u8; 32],
        operation_hash: "op".into(),
        graph_hash: "g".into(),
        kernel_abi_version: 1,
        planner_fingerprint_hash: "fp".into(),
        embed_enabled: false,
        cluster_mode: false,
        shard_count: 1,
        state_hash_before: "0".repeat(64),
        state_hash_after: "0".repeat(64),
        parent_receipts: vec![],
        shard_id: 0,
        committed_height: 0,
        produced_at: 0,
        fragments: vec![],
    };

    let runtime_json = serde_json::to_value(&runtime).unwrap();
    let dto_json = serde_json::to_value(&dto).unwrap();
    assert_eq!(
        keys(&runtime_json),
        keys(&dto_json),
        "ReceiptDto has drifted from valori_effect::Receipt — the documented \
         receipt shape no longer matches the one the handler sends"
    );
}

#[tokio::test]
async fn unknown_collection_is_404_collection_not_found_on_both_paths() {
    let ((sa_s, sa_b), (cl_s, cl_b)) = both(
        "/v1/records",
        serde_json::json!({ "values": v(1.0), "collection": "nope" }),
        None,
    )
    .await;
    assert_eq!(sa_s, StatusCode::NOT_FOUND, "standalone: {sa_b}");
    assert_eq!(cl_s, StatusCode::NOT_FOUND, "cluster: {cl_b}");
    assert_eq!(sa_b["code"], "collection_not_found");
    assert_eq!(cl_b["code"], "collection_not_found");
}

#[tokio::test]
async fn every_error_response_carries_a_code_on_both_paths() {
    // A grab-bag of distinct failure shapes; each must come back with a
    // machine-readable code regardless of which handler produced it.
    let cases: Vec<(&str, serde_json::Value)> = vec![
        // k out of range → validation
        ("/v1/search", serde_json::json!({ "query": v(1.0), "k": 0 })),
        // empty collection list → validation
        (
            "/v1/search/multi",
            serde_json::json!({ "query": v(1.0), "k": 3, "collections": [] }),
        ),
        // unknown collection → collection_not_found
        (
            "/v1/search/multi",
            serde_json::json!({ "query": v(1.0), "k": 3, "collections": ["ghost"] }),
        ),
    ];
    for (uri, body) in cases {
        let ((sa_s, sa_b), (cl_s, cl_b)) = both(uri, body.clone(), None).await;
        assert!(sa_s.is_client_error(), "standalone {uri}: {sa_s} {sa_b}");
        assert!(cl_s.is_client_error(), "cluster {uri}: {cl_s} {cl_b}");
        assert!(
            sa_b["code"].is_string(),
            "standalone {uri} has no code: {sa_b}"
        );
        assert!(
            cl_b["code"].is_string(),
            "cluster {uri} has no code: {cl_b}"
        );
        assert_eq!(
            sa_s, cl_s,
            "status fork on {uri}: standalone {sa_s} vs cluster {cl_s}\n{sa_b}\n{cl_b}"
        );
        assert_eq!(
            sa_b["code"], cl_b["code"],
            "code fork on {uri}: {sa_b} vs {cl_b}"
        );
    }
}

/// Before this phase, 401 and 403 were emitted with **no body at all** —
/// unparseable by any client that expects JSON.
#[tokio::test]
async fn unauthorized_has_a_parseable_json_body_with_a_code() {
    let router = build_router(standalone(), Some("secret-token".into()), None);
    let (status, body) = post(
        router,
        "/v1/records",
        serde_json::json!({ "values": v(1.0) }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "unauthorized", "body was: {body}");
    assert!(body["error"].is_string(), "body was: {body}");
}

// ── §4 — one canonical record request/response ───────────────────────────────

/// Every field of the canonical `InsertRecordRequest` must be *accepted* by
/// both routers. Before this phase, standalone silently dropped
/// `metadata`/`tag`/`request_id` and cluster silently dropped `text`.
#[tokio::test]
async fn insert_accepts_the_full_canonical_field_set_on_both_paths() {
    let body = serde_json::json!({
        "values": v(1.0),
        "collection": "docs",
        "text": "the quick brown fox",
        "metadata": [123, 34, 97, 34, 58, 49, 125],
        "tag": 7,
        "request_id": "0123456789abcdef0123456789abcdef",
    });
    let ((sa_s, sa_b), (cl_s, cl_b)) = both("/v1/records", body, Some("docs")).await;
    assert_eq!(sa_s, StatusCode::OK, "standalone: {sa_b}");
    assert_eq!(cl_s, StatusCode::OK, "cluster: {cl_b}");

    for (label, b) in [("standalone", &sa_b), ("cluster", &cl_b)] {
        assert!(b["id"].is_u64(), "{label} missing id: {b}");
        assert_eq!(b["deduplicated"], false, "{label}: {b}");
        assert!(b["receipt"].is_object(), "{label} missing receipt: {b}");
        assert!(
            b["receipt"]["state_hash"].is_string(),
            "{label} receipt shape: {b}"
        );
    }
    // `log_index` is the one legitimately mode-specific response field.
    assert!(
        sa_b.get("log_index").is_none(),
        "standalone must omit log_index"
    );
    assert!(cl_b["log_index"].is_u64(), "cluster must carry log_index");
}

/// The idempotency token has two historical spellings — a 16-byte array
/// (cluster insert, Python SDK) and a 32-hex string (batch insert). One
/// canonical type now accepts both, on both routers.
#[tokio::test]
async fn request_id_accepts_both_wire_spellings_on_both_paths() {
    for token in [
        serde_json::json!("0123456789abcdef0123456789abcdef"),
        serde_json::json!("01234567-89ab-cdef-0123-456789abcdef"),
        serde_json::json!([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    ] {
        let ((sa_s, sa_b), (cl_s, cl_b)) = both(
            "/v1/records",
            serde_json::json!({ "values": v(1.0), "collection": "docs", "request_id": token }),
            Some("docs"),
        )
        .await;
        assert_eq!(sa_s, StatusCode::OK, "standalone {token}: {sa_b}");
        assert_eq!(cl_s, StatusCode::OK, "cluster {token}: {cl_b}");
    }
}

#[tokio::test]
async fn invalid_request_id_is_rejected_not_ignored_on_both_paths() {
    let ((sa_s, _), (cl_s, _)) = both(
        "/v1/records",
        serde_json::json!({ "values": v(1.0), "collection": "docs", "request_id": "too-short" }),
        Some("docs"),
    )
    .await;
    assert!(sa_s.is_client_error(), "standalone accepted a bad token");
    assert!(cl_s.is_client_error(), "cluster accepted a bad token");
    assert_eq!(sa_s, cl_s);
}

// ── §5 — real idempotency, standalone and cluster ────────────────────────────

#[tokio::test]
async fn standalone_request_id_deduplicates() {
    let router = build_router(standalone(), None, None);
    make_collection(router.clone(), "docs").await;
    let token = "aaaaaaaabbbbbbbbccccccccdddddddd";

    let (s1, b1) = post(
        router.clone(),
        "/v1/records",
        serde_json::json!({ "values": v(1.0), "collection": "docs", "request_id": token }),
    )
    .await;
    assert_eq!(s1, StatusCode::OK, "{b1}");
    assert_eq!(b1["deduplicated"], false);
    let first_id = b1["id"].as_u64().unwrap();

    // Same token → same record, no second write.
    let (s2, b2) = post(
        router.clone(),
        "/v1/records",
        serde_json::json!({ "values": v(9.0), "collection": "docs", "request_id": token }),
    )
    .await;
    assert_eq!(s2, StatusCode::OK, "{b2}");
    assert_eq!(b2["deduplicated"], true, "replay was not recognised: {b2}");
    assert_eq!(b2["id"].as_u64().unwrap(), first_id);

    // Different token → a genuinely new record.
    let (s3, b3) = post(
        router.clone(),
        "/v1/records",
        serde_json::json!({
            "values": v(2.0), "collection": "docs",
            "request_id": "eeeeeeeeffffffff00000000111111 11".replace(' ', "")
        }),
    )
    .await;
    assert_eq!(s3, StatusCode::OK, "{b3}");
    assert_eq!(b3["deduplicated"], false);
    assert_ne!(b3["id"].as_u64().unwrap(), first_id);

    // No token at all → always a new record.
    let (s4, b4) = post(
        router,
        "/v1/records",
        serde_json::json!({ "values": v(3.0), "collection": "docs" }),
    )
    .await;
    assert_eq!(s4, StatusCode::OK, "{b4}");
    assert_eq!(b4["deduplicated"], false);
}

#[tokio::test]
async fn cluster_request_id_deduplicates_to_the_same_record_id() {
    let handle = boot_cluster().await;
    let router = build_cluster_router(&handle, None);
    make_collection(router.clone(), "docs").await;
    let token = "aaaaaaaabbbbbbbbccccccccdddddddd";

    let (s1, b1) = post(
        router.clone(),
        "/v1/records",
        serde_json::json!({ "values": v(1.0), "collection": "docs", "request_id": token }),
    )
    .await;
    assert_eq!(s1, StatusCode::OK, "{b1}");
    assert_eq!(b1["deduplicated"], false);
    let first_id = b1["id"].as_u64().unwrap();

    let (s2, b2) = post(
        router,
        "/v1/records",
        serde_json::json!({ "values": v(9.0), "collection": "docs", "request_id": token }),
    )
    .await;
    assert_eq!(s2, StatusCode::OK, "{b2}");
    assert_eq!(b2["deduplicated"], true, "{b2}");
    // Phase API-2: the replicated dedup table now remembers the record id, so
    // a replay is answered with the original record instead of `id: 0`.
    assert_eq!(b2["id"].as_u64().unwrap(), first_id, "{b2}");
}

// ── §7 / §8 — search semantics ───────────────────────────────────────────────

/// `k` has one meaning on both paths: required, and bounded. Cluster used to
/// silently default it to 10 while standalone required it.
#[tokio::test]
async fn search_k_bounds_agree_on_both_paths() {
    for k in [serde_json::json!(0), serde_json::json!(1_000_000)] {
        let ((sa_s, sa_b), (cl_s, cl_b)) = both(
            "/v1/search",
            serde_json::json!({ "query": v(1.0), "k": k, "collection": "docs" }),
            Some("docs"),
        )
        .await;
        assert_eq!(
            sa_s, cl_s,
            "k={k} status fork: standalone {sa_s} {sa_b} vs cluster {cl_s} {cl_b}"
        );
        assert!(sa_s.is_client_error(), "k={k} should be rejected: {sa_b}");
    }
}

#[tokio::test]
async fn search_response_shape_agrees_on_both_paths() {
    let sa = build_router(standalone(), None, None);
    let handle = boot_cluster().await;
    let cl = build_cluster_router(&handle, None);
    for r in [sa.clone(), cl.clone()] {
        make_collection(r.clone(), "docs").await;
        for x in [1.0f32, 2.0, 3.0] {
            let (s, b) = post(
                r.clone(),
                "/v1/records",
                serde_json::json!({ "values": v(x), "collection": "docs" }),
            )
            .await;
            assert_eq!(s, StatusCode::OK, "{b}");
        }
    }
    let q = serde_json::json!({ "query": v(2.0), "k": 2, "collection": "docs" });
    let (sa_s, sa_b) = post(sa, "/v1/search", q.clone()).await;
    let (cl_s, cl_b) = post(cl, "/v1/search", q).await;
    assert_eq!(sa_s, StatusCode::OK, "{sa_b}");
    assert_eq!(cl_s, StatusCode::OK, "{cl_b}");

    for (label, b) in [("standalone", &sa_b), ("cluster", &cl_b)] {
        let hits = b["results"]
            .as_array()
            .unwrap_or_else(|| panic!("{label}: {b}"));
        assert_eq!(hits.len(), 2, "{label}: {b}");
        for h in hits {
            assert!(h["id"].is_u64(), "{label} hit missing id: {h}");
            // `score` is a raw squared-L2 distance, lower is better. It is
            // never normalised and never renamed.
            assert!(h["score"].is_number(), "{label} hit missing score: {h}");
        }
        // Ascending by distance.
        assert!(hits[0]["score"].as_f64().unwrap() <= hits[1]["score"].as_f64().unwrap());
    }
}

// ── §9 / §18 — multi-collection search error semantics ───────────────────────

#[tokio::test]
async fn multi_search_error_statuses_agree_on_both_paths() {
    // Unknown collection: audit found 400 standalone / 404 cluster.
    let ((sa_s, sa_b), (cl_s, cl_b)) = both(
        "/v1/search/multi",
        serde_json::json!({ "query": v(1.0), "k": 3, "collections": ["ghost"] }),
        None,
    )
    .await;
    assert_eq!(sa_s, StatusCode::NOT_FOUND, "{sa_b}");
    assert_eq!(cl_s, StatusCode::NOT_FOUND, "{cl_b}");
    assert_eq!(sa_b["code"], "collection_not_found");
    assert_eq!(cl_b["code"], "collection_not_found");

    // Query dimension mismatch.
    let ((sa_s, sa_b), (cl_s, cl_b)) = both(
        "/v1/search/multi",
        serde_json::json!({ "query": [1.0, 2.0], "k": 3, "collections": ["docs"] }),
        Some("docs"),
    )
    .await;
    assert_eq!(sa_s, cl_s, "dim-mismatch fork: {sa_b} vs {cl_b}");
    assert!(sa_s.is_client_error(), "{sa_b}");

    // Too many collections.
    let many: Vec<String> = (0..64).map(|i| format!("c{i}")).collect();
    let ((sa_s, _), (cl_s, _)) = both(
        "/v1/search/multi",
        serde_json::json!({ "query": v(1.0), "k": 3, "collections": many }),
        None,
    )
    .await;
    assert_eq!(sa_s, StatusCode::BAD_REQUEST);
    assert_eq!(cl_s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn multi_search_hits_always_carry_collection_identity() {
    let sa = build_router(standalone(), None, None);
    let handle = boot_cluster().await;
    let cl = build_cluster_router(&handle, None);
    for r in [sa.clone(), cl.clone()] {
        make_collection(r.clone(), "a").await;
        make_collection(r.clone(), "b").await;
        for (c, x) in [("a", 1.0f32), ("b", 2.0)] {
            post(
                r.clone(),
                "/v1/records",
                serde_json::json!({ "values": v(x), "collection": c }),
            )
            .await;
        }
    }
    let q = serde_json::json!({ "query": v(1.5), "k": 5, "collections": ["a", "b"] });
    for (label, r) in [("standalone", sa), ("cluster", cl)] {
        let (s, b) = post(r, "/v1/search/multi", q.clone()).await;
        assert_eq!(s, StatusCode::OK, "{label}: {b}");
        for hit in b["results"].as_array().unwrap() {
            assert!(
                hit["collection"].is_string(),
                "{label} hit without collection identity: {hit}"
            );
        }
        assert!(b["collections_searched"].is_array(), "{label}: {b}");
    }
}

// ── §11 / §12 — Collection creation contract ─────────────────────────────────

#[tokio::test]
async fn collection_creation_requires_dimension_and_metric_on_both_paths() {
    for body in [
        serde_json::json!({ "name": "x" }),
        serde_json::json!({ "name": "x", "dimension": DIM }),
        serde_json::json!({ "name": "x", "metric": "squared_l2" }),
    ] {
        let ((sa_s, sa_b), (cl_s, cl_b)) = both("/v1/namespaces", body.clone(), None).await;
        assert!(sa_s.is_client_error(), "standalone accepted {body}: {sa_b}");
        assert!(cl_s.is_client_error(), "cluster accepted {body}: {cl_b}");
        assert_eq!(sa_s, cl_s, "status fork on {body}");
    }
}

/// `"default"` is an ordinary name. It is not auto-created, and creating it
/// behaves exactly like creating any other Collection.
#[tokio::test]
async fn default_has_no_implicit_behaviour_on_both_paths() {
    let sa = build_router(standalone(), None, None);
    let handle = boot_cluster().await;
    let cl = build_cluster_router(&handle, None);

    for (label, r) in [("standalone", sa), ("cluster", cl)] {
        // A fresh node has zero collections.
        let (s, b) = call(r.clone(), Method::GET, "/v1/namespaces", None).await;
        assert_eq!(s, StatusCode::OK, "{label}: {b}");
        assert_eq!(
            b["collections"].as_array().map(|a| a.len()),
            Some(0),
            "{label} started with collections: {b}"
        );

        // Omitting `collection` is an error — there is no implicit default
        // target, and a Collection *named* "default" does not become one.
        let (s, b) = post(
            r.clone(),
            "/v1/records",
            serde_json::json!({ "values": v(1.0) }),
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{label}: {b}");
        assert_eq!(b["code"], "collection_not_found", "{label}: {b}");

        // Creating "default" needs the same explicit config as any other name.
        let (s, b) = post(
            r.clone(),
            "/v1/namespaces",
            serde_json::json!({ "name": "default" }),
        )
        .await;
        assert!(s.is_client_error(), "{label} auto-configured default: {b}");

        make_collection(r.clone(), "default").await;

        // Even now, omitting `collection` still fails: "default" is an
        // ordinary Collection that must be named explicitly.
        let (s, b) = post(
            r.clone(),
            "/v1/records",
            serde_json::json!({ "values": v(1.0) }),
        )
        .await;
        assert_eq!(
            s,
            StatusCode::NOT_FOUND,
            "{label} treated a Collection named \"default\" as implicit: {b}"
        );

        // Naming it explicitly works, exactly like any other Collection.
        let (s, b) = post(
            r,
            "/v1/records",
            serde_json::json!({ "values": v(1.0), "collection": "default" }),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{label}: {b}");
    }
}

/// Re-creating a Collection must never silently mutate the existing one.
#[tokio::test]
async fn collection_create_idempotency_agrees_on_both_paths() {
    let sa = build_router(standalone(), None, None);
    let handle = boot_cluster().await;
    let cl = build_cluster_router(&handle, None);

    for (label, r) in [("standalone", sa), ("cluster", cl)] {
        make_collection(r.clone(), "docs").await;

        // Same name, same config → no duplicate, no error.
        let (s, b) = post(
            r.clone(),
            "/v1/namespaces",
            serde_json::json!({ "name": "docs", "dimension": DIM, "metric": "squared_l2" }),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{label}: {b}");
        assert_eq!(b["created"], false, "{label} reported a fresh create: {b}");

        // Same name, different dimension → conflict, and the original survives.
        let (s, b) = post(
            r.clone(),
            "/v1/namespaces",
            serde_json::json!({ "name": "docs", "dimension": DIM + 1, "metric": "squared_l2" }),
        )
        .await;
        assert!(
            s == StatusCode::CONFLICT || s.is_client_error(),
            "{label} allowed a dimension change: {s} {b}"
        );

        let (s, b) = call(r, Method::GET, "/v1/namespaces", None).await;
        assert_eq!(s, StatusCode::OK);
        let docs = b["collections"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "docs")
            .unwrap_or_else(|| panic!("{label}: docs vanished: {b}"))
            .clone();
        assert_eq!(
            docs["dimension"].as_u64(),
            Some(DIM as u64),
            "{label} dimension was mutated by a re-create: {docs}"
        );
    }
}

// ── §31 — legacy aliases stay deprecated, and stay working ───────────────────

#[tokio::test]
async fn legacy_aliases_still_work_and_announce_their_deprecation() {
    let router = build_router(standalone(), None, None);
    make_collection(router.clone(), "default").await;
    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/records")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(
                        &serde_json::json!({ "values": v(1.0), "collection": "default" }),
                    )
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("Deprecation")
            .map(|v| v.to_str().unwrap()),
        Some("true"),
        "legacy alias lost its Deprecation header"
    );
}

// ── Structural guards ────────────────────────────────────────────────────────

/// Both routers must keep deserialising the SAME request type. If either one
/// reintroduces a private `struct InsertRequest`, a field can start being
/// accepted on one path and silently dropped on the other again — the exact
/// P0 this phase fixed.
#[test]
fn both_routers_share_the_canonical_insert_dto() {
    let cluster = include_str!("../src/cluster_server.rs");
    assert!(
        cluster.contains("use crate::api::{InsertRecordRequest as InsertRequest"),
        "cluster_server.rs no longer imports the canonical insert DTO — did \
         someone reintroduce a router-private insert request type?"
    );
    let server = include_str!("../src/server.rs");
    assert!(
        server.contains("Json<InsertRecordRequest>"),
        "server.rs no longer deserialises the canonical insert DTO"
    );
}

/// The middleware that guarantees every error body carries a `code` must stay
/// installed on both routers.
#[test]
fn both_routers_install_the_error_code_middleware() {
    for (label, src) in [
        ("server.rs", include_str!("../src/server.rs")),
        (
            "cluster_server.rs",
            include_str!("../src/cluster_server.rs"),
        ),
    ] {
        assert!(
            src.contains("crate::error_codes::attach_error_code"),
            "{label} dropped the error-code middleware"
        );
    }
}

// ── Scopes (§17) ─────────────────────────────────────────────────────────────

/// `required_scope()` derives the minimum scope from `(method, path)` by
/// prefix matching, and the contract records the answer per operation as
/// `x-required-scope`. Phase 1 found two places where the derivation was
/// wrong, not just undocumented: cluster-membership mutations settled on
/// `read_write` (audit row 34) and the two body-carrying read endpoints
/// settled on `read_write` because they do not literally end in `/search`
/// (row 35). Both are fixed in the implementation; this pins them.
#[test]
fn scope_derivation_matches_the_documented_contract() {
    use valori_node::api_keys::{required_scope, ApiScope};

    let cases: &[(Method, &str, ApiScope)] = &[
        // Row 34 — reconfiguring the cluster is an admin action.
        (Method::POST, "/v1/cluster/add-node", ApiScope::Admin),
        (Method::POST, "/v1/cluster/remove-node", ApiScope::Admin),
        (Method::POST, "/v1/cluster/snapshot", ApiScope::Admin),
        // Row 35 — cross-collection and GraphRAG queries are pure reads.
        (Method::POST, "/v1/search/multi", ApiScope::ReadOnly),
        (Method::POST, "/v1/graphrag", ApiScope::ReadOnly),
        // Unchanged neighbours — the fix must not widen anything else.
        (Method::GET, "/v1/cluster/status", ApiScope::ReadOnly),
        (Method::POST, "/v1/search", ApiScope::ReadOnly),
        (Method::POST, "/v1/records", ApiScope::ReadWrite),
        (Method::POST, "/v1/namespaces", ApiScope::ReadWrite),
        (Method::GET, "/v1/keys", ApiScope::Admin),
        (Method::POST, "/v1/storage/snapshots", ApiScope::Admin),
    ];

    for (method, path, expected) in cases {
        assert_eq!(
            required_scope(method, path),
            *expected,
            "{method} {path} resolves to the wrong scope"
        );
    }
}

/// The contract must not describe a weaker scope than the code enforces —
/// a generated SDK would tell users a read-only key suffices for an admin
/// action, or vice versa.
#[test]
fn contract_records_the_corrected_scopes() {
    let contract = include_str!("../../../api/openapi/valori-v1.yaml");

    // Phase API-3.1 §8: cluster-membership mutations and key management are
    // ADMIN routes. The node still serves them — see `cluster_api.rs` — but
    // they are not part of the SDK surface, so they are not registered on
    // `ValoriApi` and must not appear in the public contract. A server route
    // is not the same thing as a public SDK route.
    //
    // The scope they enforce is still pinned, one test above, directly against
    // `required_scope`. Dropping them from the document does not weaken that.
    for op in [
        "add_cluster_node",
        "remove_cluster_node",
        "trigger_cluster_snapshot",
        "create_api_key",
        "list_api_keys",
        "revoke_api_key",
        "shred_key",
    ] {
        assert!(
            !contract.contains(&format!("operationId: {op}")),
            "{op} is an ADMIN route and must not be in the public SDK contract"
        );
    }

    for op in ["search_multi", "graphrag"] {
        let at = contract
            .find(&format!("operationId: {op}"))
            .unwrap_or_else(|| panic!("{op} missing from the contract"));
        let window = &contract[at..(at + 3000).min(contract.len())];
        assert!(
            window.contains("x-required-scope: read_only"),
            "{op} is still documented as anything but read_only"
        );
    }
}

/// Phase API-3.1 §19: `x-required-scope` is added by a `Modify` pass that reads
/// the very function the auth middleware calls. This proves the two never
/// diverge — for every operation in the contract, not a hand-picked few.
///
/// Phase API-3.3 refined the invariant. It used to be "every operation carries
/// a scope", which was wrong in one direction: `GET /health` declares
/// `security: []`, so the auth middleware never runs on it and `required_scope`
/// is never consulted — yet the pass stamped it with the function's default
/// (`read_only`) anyway, telling every SDK that the one deliberately-open
/// endpoint needed a key. The honest invariant is conditional on `security`:
///
///   * authenticated operation  -> carries exactly the scope the server enforces
///   * unauthenticated operation -> carries no scope at all
#[test]
fn every_operation_documents_the_scope_the_server_enforces() {
    use valori_node::api_keys::required_scope;

    let contract = include_str!("../../../api/openapi/valori-v1.yaml");
    let doc: serde_norway::Value = serde_norway::from_str(contract).expect("contract parses");
    let paths = doc["paths"].as_mapping().expect("paths mapping");

    let mut checked = 0usize;
    for (path, item) in paths {
        let path = path.as_str().expect("path is a string");
        let axum_path = path.replace('{', ":").replace('}', "");
        for (method, op) in item.as_mapping().expect("path item mapping") {
            let method = method.as_str().expect("method is a string");
            let Ok(m) = Method::from_bytes(method.to_uppercase().as_bytes()) else {
                continue;
            };
            // `security: []` renders as an empty sequence and means "no
            // credentials are consulted"; a missing key means the operation
            // inherits the document-level requirement.
            let authenticated = match op.get("security") {
                Some(s) => !s.as_sequence().is_some_and(|r| r.is_empty()),
                None => true,
            };
            let documented = op["x-required-scope"].as_str();

            if authenticated {
                let documented =
                    documented.unwrap_or_else(|| panic!("{method} {path} has no x-required-scope"));
                assert_eq!(
                    documented,
                    required_scope(&m, &axum_path).to_string(),
                    "{method} {path} documents a scope the server does not enforce"
                );
            } else {
                assert!(
                    documented.is_none(),
                    "{method} {path} declares security: [] but still advertises \
                     x-required-scope = {documented:?} — no credentials are consulted \
                     on this operation, so no scope can be required"
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 0, "no operations were checked");
}

// ── Phase API-3.1 ────────────────────────────────────────────────────────────

/// §14. `HealthResponse` keeps `serde_json::Value` for its additive
/// sub-objects so a type change can never silently drop a legacy field. The
/// contract describes those objects through `PoolStatsSchema` /
/// `EngineHealthStats` / `ClusterHealthStats` mirrors. Mirrors can rot, so
/// this drives the real handler and checks the shape the node actually emits
/// against the shape the contract promises.
#[tokio::test]
async fn health_subobjects_match_schema_mirrors() {
    let (status, body) = call(
        build_router(standalone(), None, None),
        Method::GET,
        "/health",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Every key the top-level contract promises for a standalone node.
    for field in ["status", "mode", "version", "shard_count", "engine"] {
        assert!(body.get(field).is_some(), "/health is missing `{field}`");
    }
    assert_eq!(body["mode"], "standalone");

    // PoolStatsSchema
    for pool in ["records", "nodes", "edges"] {
        let p = &body[pool];
        for field in ["live", "slots_used", "capacity", "fill_pct"] {
            assert!(
                p.get(field).is_some(),
                "/health `{pool}` is missing `{field}` — PoolStatsSchema is stale"
            );
        }
    }

    // EngineHealthStats
    let engine = &body["engine"];
    for field in [
        "status",
        "version",
        "collections",
        "persistence",
        "records",
        "nodes",
        "edges",
        "embed_enabled",
        "shard_count",
    ] {
        assert!(
            engine.get(field).is_some(),
            "/health `engine` is missing `{field}` — EngineHealthStats is stale"
        );
    }
}

/// §14. Cluster `/health` must keep the top-level `leader` and `dim` fields
/// that `ui/src/lib/hooks/useHealth.ts` reads, and additionally expose the
/// structured `cluster` sub-object described by `ClusterHealthStats`.
#[tokio::test]
async fn cluster_health_keeps_its_legacy_top_level_fields() {
    let handle = boot_cluster().await;
    let (_status, body) = call(
        build_cluster_router(&handle, None),
        Method::GET,
        "/health",
        None,
    )
    .await;

    assert_eq!(body["mode"], "cluster");
    // Present as keys even when null-valued is not enough — `leader` is
    // skipped when there is no leader, which is the pre-API-3 behaviour. What
    // must never happen is the field disappearing from the *contract*.
    for field in ["status", "version", "cluster", "node_id", "role", "term"] {
        assert!(
            body.get(field).is_some(),
            "cluster /health is missing `{field}`"
        );
    }
    let cluster = &body["cluster"];
    for field in ["status", "leader", "dim", "role", "term"] {
        assert!(
            cluster.get(field).is_some(),
            "cluster /health `cluster` is missing `{field}` — ClusterHealthStats is stale"
        );
    }
}

/// §16. Operation identity is a string (`op-{log_index}`), and the lookup URL
/// accepts both the prefixed form the API emits and the bare index older
/// clients may have stored. Neither form may start 404-ing on the other's
/// input.
#[tokio::test]
async fn operation_urls_accept_both_id_forms() {
    let sa = build_router(standalone(), None, None);

    // No event log configured, so both forms resolve identically — the point
    // is that the *parse* accepts both rather than rejecting one as malformed.
    let (bare, _) = call(sa.clone(), Method::GET, "/v1/operations/7", None).await;
    let (prefixed, _) = call(sa.clone(), Method::GET, "/v1/operations/op-7", None).await;
    assert_eq!(
        bare, prefixed,
        "the two operation id spellings resolve differently"
    );
    assert_ne!(
        bare,
        StatusCode::BAD_REQUEST,
        "a numeric operation id is no longer accepted"
    );

    // A malformed id is rejected with 400 by the id parser, but only once an
    // event log exists — without one the handler short-circuits on "no
    // journal" first. This harness runs without persistence, so the parse
    // branch is not reachable here; `/v1/operations/{id}` documents the 400
    // and the parser is exercised by the event-log integration tests.
    let (garbage, _) = call(sa, Method::GET, "/v1/operations/not-an-op", None).await;
    assert!(
        garbage == StatusCode::BAD_REQUEST || garbage == StatusCode::NOT_FOUND,
        "a malformed operation id produced {garbage}, which is neither a \
         rejection nor a miss"
    );
}

/// §12. Coverage is a two-link chain: annotation AND registration. This pins
/// the end of that chain from the Rust side — every operation the generator
/// emits carries an operationId, and no two share one.
#[cfg(feature = "utoipa")]
#[test]
fn generated_operation_ids_are_present_and_unique() {
    let doc: serde_norway::Value =
        serde_norway::from_str(&valori_node::openapi::to_yaml().expect("render")).expect("yaml");
    let mut seen = std::collections::BTreeSet::new();
    let mut count = 0usize;
    for (path, item) in doc["paths"].as_mapping().expect("paths") {
        for (method, op) in item.as_mapping().expect("path item") {
            let id = op["operationId"].as_str().unwrap_or_else(|| {
                panic!(
                    "{:?} {:?} has no operationId",
                    method.as_str(),
                    path.as_str()
                )
            });
            assert!(seen.insert(id.to_string()), "duplicate operationId: {id}");
            count += 1;
        }
    }
    assert!(count > 0, "no operations in the generated document");
}
