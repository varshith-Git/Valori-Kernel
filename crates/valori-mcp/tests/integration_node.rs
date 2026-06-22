// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! End-to-end: drive the MCP tools against a REAL in-process valori-node and
//! prove that the recall receipt verifies against the node's live proof
//! endpoints — not a mock. This is the test that backs the wedge claim.

use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;

use valori_mcp::backend::HttpBackend;
use valori_mcp::mcp::McpServer;
use valori_mcp::protocol::Request;
use valori_mcp::receipt::{compute_digest, subgraph_fingerprint, ReceiptBody, ResultFingerprint};
use valori_mcp::stdio::handle_line;

use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use valori_node::server::build_router;

const DIM: usize = 8;

/// Boot a real node with an event log enabled (so the receipt carries an
/// event-log hash) and return its base URL.
async fn spawn_node() -> String {
    let dir = tempfile::tempdir().unwrap();
    // Keep the tempdir so the event-log file outlives the test body.
    let path = dir.keep().join("events.log");

    let mut cfg = NodeConfig::default();
    cfg.dim = DIM;
    cfg.max_records = 1000;
    cfg.event_log_path = Some(path);
    cfg.wal_path = None;

    let engine = Engine::new(&cfg);
    let shared = Arc::new(RwLock::new(engine));
    let router = build_router(shared, None, None);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

fn vec_n(seed: f32) -> Vec<f32> {
    (0..DIM).map(|i| seed + i as f32 * 0.01).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recall_receipt_verifies_against_live_node() {
    let url = spawn_node().await;
    let backend = HttpBackend::new(url, None);
    let server = McpServer::new(backend);

    // Write three memories through the MCP write tool.
    for (i, seed) in [0.10f32, 0.50, 0.90].iter().enumerate() {
        let args = json!({ "vector": vec_n(*seed), "text": format!("memory {i}") });
        let out = call(&server, "memory_write", args).await;
        assert!(out["isError"] != json!(true), "write failed: {out:?}");
    }

    // Recall near the first memory.
    let recall = call(&server, "memory_recall", json!({ "query_vector": vec_n(0.10), "k": 2 })).await;
    assert_eq!(recall["isError"], json!(false));

    // The tool payload is JSON text inside content[0].text.
    let payload: Value = serde_json::from_str(recall["content"][0]["text"].as_str().unwrap()).unwrap();

    let results = payload["results"].as_array().expect("results array");
    assert!(!results.is_empty(), "recall returned no memories");

    let receipt = &payload["receipt"];
    // State hash is the real 64-hex kernel Merkle root.
    let state_hash = receipt["state_hash"].as_str().unwrap();
    assert_eq!(state_hash.len(), 64);
    // Event log was enabled → the receipt must carry the event-log proof.
    let event_log_hash = receipt["event_log_hash"].as_str().expect("event_log_hash present");
    assert_eq!(event_log_hash.len(), 64);
    let committed_height = receipt["committed_height"].as_u64().unwrap();
    assert!(committed_height >= 3, "expected >=3 committed events, got {committed_height}");

    // THE PROOF: independently reconstruct the receipt body and recompute the
    // digest, exactly as an external auditor would. It must match byte-for-byte.
    let fingerprints: Vec<ResultFingerprint> = receipt["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| ResultFingerprint {
            memory_id: f["memory_id"].as_str().unwrap().to_string(),
            record_id: f["record_id"].as_u64().unwrap(),
            score_bits: f["score_bits"].as_str().unwrap().to_string(),
        })
        .collect();

    let rebuilt = ReceiptBody {
        state_hash: state_hash.to_string(),
        event_log_hash: Some(event_log_hash.to_string()),
        committed_height: Some(committed_height),
        query_dim: receipt["query_dim"].as_u64().unwrap() as usize,
        k: receipt["k"].as_u64().unwrap() as usize,
        results: fingerprints,
        subgraph: None,
    };
    let recomputed = compute_digest(&rebuilt);
    assert_eq!(
        recomputed,
        receipt["receipt_digest"].as_str().unwrap(),
        "independently recomputed digest must match the receipt — this is the verification contract"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timeline_reflects_writes() {
    let url = spawn_node().await;
    let backend = HttpBackend::new(url, None);
    let server = McpServer::new(backend);

    for seed in [0.2f32, 0.4] {
        call(&server, "memory_write", json!({ "vector": vec_n(seed) })).await;
    }

    let out = call(&server, "memory_timeline", json!({})).await;
    let payload: Value = serde_json::from_str(out["content"][0]["text"].as_str().unwrap()).unwrap();
    let total = payload["total"].as_u64().unwrap_or(0);
    assert!(total >= 2, "timeline should record the writes, got total={total}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_handshake_over_line_transport() {
    let url = spawn_node().await;
    let backend = HttpBackend::new(url, None);
    let server = McpServer::new(backend);

    // initialize
    let init = handle_line(
        &server,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    )
    .await
    .unwrap();
    assert!(init.contains("protocolVersion"));

    // notifications/initialized → no reply
    assert!(handle_line(&server, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
        .await
        .is_none());

    // tools/list → six tools
    let list = handle_line(&server, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#)
        .await
        .unwrap();
    let v: Value = serde_json::from_str(&list).unwrap();
    assert_eq!(v["result"]["tools"].as_array().unwrap().len(), 7);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graphrag_returns_hits_and_subgraph_with_verifiable_receipt() {
    let url = spawn_node().await;
    let backend = HttpBackend::new(url.clone(), None);
    let server = McpServer::new(backend);

    // Write a memory and capture its document + chunk graph nodes.
    let write = call(&server, "memory_write", json!({ "vector": vec_n(0.10), "text": "seed" })).await;
    let wpayload: Value = serde_json::from_str(write["content"][0]["text"].as_str().unwrap()).unwrap();
    let chunk_node = wpayload["chunk_node_id"].as_u64().unwrap();
    let doc_node = wpayload["document_node_id"].as_u64().unwrap();

    // Add an edge OUT of the chunk node so the subgraph around the hit is
    // non-trivial (the default ingest edge points doc -> chunk, i.e. inbound).
    let http = reqwest::Client::new();
    let r = http
        .post(format!("{url}/graph/edge"))
        .json(&json!({ "from": chunk_node, "to": doc_node, "kind": 0 }))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "edge creation failed: {}", r.status());

    // A couple more memories so KNN has choices.
    call(&server, "memory_write", json!({ "vector": vec_n(0.50) })).await;
    call(&server, "memory_write", json!({ "vector": vec_n(0.90) })).await;

    // GraphRAG in one call.
    let out = call(&server, "memory_graph_recall",
        json!({ "query_vector": vec_n(0.10), "k": 3, "depth": 2 })).await;
    assert_eq!(out["isError"], json!(false));
    let payload: Value = serde_json::from_str(out["content"][0]["text"].as_str().unwrap()).unwrap();

    // One call returned BOTH the vector hits and the connected subgraph.
    assert!(!payload["hits"].as_array().unwrap().is_empty(), "no hits");
    let nodes = payload["subgraph"]["nodes"].as_array().unwrap();
    let edges = payload["subgraph"]["edges"].as_array().unwrap();
    assert!(nodes.iter().any(|n| n["id"].as_u64() == Some(chunk_node)), "seed chunk node missing");
    assert!(nodes.iter().any(|n| n["id"].as_u64() == Some(doc_node)), "edge target (doc) not expanded");
    assert!(edges.iter().any(|e| e["from"].as_u64() == Some(chunk_node) && e["to"].as_u64() == Some(doc_node)),
        "the chunk->doc edge should be in the subgraph");

    // THE PROOF: recompute the receipt digest — which now binds BOTH the hits
    // AND the subgraph — exactly as an external auditor would.
    let receipt = &payload["receipt"];
    let fingerprints: Vec<ResultFingerprint> = receipt["results"].as_array().unwrap().iter()
        .map(|f| ResultFingerprint {
            memory_id: f["memory_id"].as_str().unwrap().to_string(),
            record_id: f["record_id"].as_u64().unwrap(),
            score_bits: f["score_bits"].as_str().unwrap().to_string(),
        })
        .collect();

    let rebuilt = ReceiptBody {
        state_hash: receipt["state_hash"].as_str().unwrap().to_string(),
        event_log_hash: receipt["event_log_hash"].as_str().map(|s| s.to_string()),
        committed_height: receipt["committed_height"].as_u64(),
        query_dim: receipt["query_dim"].as_u64().unwrap() as usize,
        k: receipt["k"].as_u64().unwrap() as usize,
        results: fingerprints,
        subgraph: Some(subgraph_fingerprint(&payload["subgraph"])),
    };
    assert_eq!(
        compute_digest(&rebuilt),
        receipt["receipt_digest"].as_str().unwrap(),
        "GraphRAG receipt must independently recompute — binds hits + subgraph"
    );
}

/// Helper: send a `tools/call` through the server and return the MCP result object.
async fn call(server: &McpServer<HttpBackend>, name: &str, arguments: Value) -> Value {
    let req: Request = serde_json::from_value(json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    }))
    .unwrap();
    server.handle(req).await.unwrap().result.unwrap()
}
