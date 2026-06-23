// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! The six MCP tools and their dispatch. Each tool is a thin composition over
//! [`NodeClient`] primitives. The one with teeth is `memory_recall`, which
//! pairs the result set with a [`Receipt`] — verifiable agent memory.

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::backend::NodeClient;
use crate::receipt::{
    fingerprints_from_results, subgraph_fingerprint, Receipt, ReceiptBody,
};

/// Tool names — underscore form (some MCP clients reject dots in tool names).
pub const WRITE: &str = "memory_write";
pub const RECALL: &str = "memory_recall";
pub const GRAPH_RECALL: &str = "memory_graph_recall";
pub const WHY: &str = "memory_why";
pub const TIMELINE: &str = "memory_timeline";
pub const FORGET: &str = "memory_forget";
pub const FORK: &str = "memory_fork";

/// The `tools/list` payload: schema-bearing definitions for all six tools.
pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": WRITE,
            "description": "Store a memory. Provide the embedding `vector`; optional `text` and \
                            `metadata` are kept alongside it and returned on recall. Every write is \
                            BLAKE3-chained into the audit log.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "vector": { "type": "array", "items": { "type": "number" },
                                "description": "Embedding of the memory." },
                    "text": { "type": "string", "description": "Original text, stored as metadata." },
                    "collection": { "type": "string", "description": "Namespace (default: \"default\")." },
                    "metadata": { "type": "object", "description": "Arbitrary JSON kept with the memory." }
                },
                "required": ["vector"]
            }
        }),
        json!({
            "name": RECALL,
            "description": "Recall the k nearest memories to a query embedding AND a verifiable \
                            receipt: a BLAKE3 digest binding the exact result set to the committed \
                            state hash at recall time. Lets you prove later what the agent recalled. \
                            Optionally pass decay_half_life_secs for recency-aware recall: older \
                            memories are ranked down (a memory one half-life old has its distance \
                            doubled), so fresh context surfaces over stale.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query_vector": { "type": "array", "items": { "type": "number" } },
                    "k": { "type": "integer", "minimum": 1, "default": 5 },
                    "collection": { "type": "string" },
                    "decay_half_life_secs": { "type": "integer", "minimum": 0,
                        "description": "Recency half-life in seconds; 0/absent = no decay." }
                },
                "required": ["query_vector"]
            }
        }),
        json!({
            "name": GRAPH_RECALL,
            "description": "GraphRAG in one call: recall the k nearest memories AND the connected \
                            knowledge subgraph around them (sources, related entities, citations) up \
                            to `depth` hops — from a single consistent snapshot. Returns a receipt \
                            binding BOTH the hits and the subgraph. Replaces the Neo4j+vector-DB \
                            two-system dance.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query_vector": { "type": "array", "items": { "type": "number" } },
                    "k": { "type": "integer", "minimum": 1, "default": 5 },
                    "depth": { "type": "integer", "minimum": 1, "maximum": 4, "default": 2 },
                    "collection": { "type": "string" }
                },
                "required": ["query_vector"]
            }
        }),
        json!({
            "name": WHY,
            "description": "Explain why a memory is held: returns the provenance subgraph around a \
                            graph node (sources, derivations, citations) up to `depth` hops.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "root": { "type": "integer", "description": "Graph node id to explain." },
                    "depth": { "type": "integer", "minimum": 1, "maximum": 4, "default": 2 }
                },
                "required": ["root"]
            }
        }),
        json!({
            "name": TIMELINE,
            "description": "Return the committed event history (optionally bounded by ISO-8601 `from`/`to`). \
                            The agent's memory, as an auditable timeline.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "ISO-8601 lower bound (inclusive)." },
                    "to": { "type": "string", "description": "ISO-8601 upper bound (inclusive)." }
                }
            }
        }),
        json!({
            "name": FORGET,
            "description": "Certified erasure: destroy the encryption key for a memory key-id, rendering \
                            its ciphertext unrecoverable (GDPR-grade). Returns the shred result as a \
                            deletion certificate.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key_id": { "type": "string", "description": "32-hex DEK id to shred." }
                },
                "required": ["key_id"]
            }
        }),
        json!({
            "name": FORK,
            "description": "Take a deterministic snapshot of the current memory state — a fork point you \
                            can branch from or restore to. Returns the snapshot metadata and state hash.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
    ]
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn parse_vector(args: &Value, field: &str) -> Result<Vec<f32>> {
    let arr = args
        .get(field)
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("`{field}` is required and must be an array of numbers"))?;
    arr.iter()
        .map(|n| n.as_f64().map(|f| f as f32).ok_or_else(|| anyhow!("`{field}` must contain only numbers")))
        .collect()
}

fn opt_str(args: &Value, field: &str) -> Option<String> {
    args.get(field).and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Dispatch a `tools/call`. Returns the tool's JSON payload (the caller wraps it
/// in MCP `content`). Errors here become MCP tool errors, not transport errors.
pub async fn call_tool(client: &dyn NodeClient, name: &str, args: &Value) -> Result<Value> {
    match name {
        WRITE => {
            let vector = parse_vector(args, "vector")?;
            let collection = opt_str(args, "collection");
            // Fold `text` into metadata so recall can return it.
            let mut metadata = args.get("metadata").cloned();
            if let Some(text) = opt_str(args, "text") {
                let m = metadata.get_or_insert_with(|| json!({}));
                if let Some(obj) = m.as_object_mut() {
                    obj.insert("text".to_string(), json!(text));
                }
            }
            client.memory_upsert(vector, collection, metadata).await
        }

        RECALL => {
            let query = parse_vector(args, "query_vector")?;
            let k = args.get("k").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
            let collection = opt_str(args, "collection");
            let decay = args.get("decay_half_life_secs").and_then(|v| v.as_u64());
            let query_dim = query.len();

            let results = client
                .memory_search(query.clone(), k, collection, decay)
                .await
                .context("memory search failed")?;

            let state_hash = client.proof_state().await.context("fetching state proof")?;
            let event_log = client.proof_event_log().await.unwrap_or(None);
            let (event_log_hash, committed_height) = match event_log {
                Some((h, n)) => (Some(h), Some(n)),
                None => (None, None),
            };

            let body = ReceiptBody {
                state_hash,
                event_log_hash,
                committed_height,
                query_dim,
                k,
                results: fingerprints_from_results(&results),
                subgraph: None,
            };
            let receipt = Receipt::build(body, now_unix());

            Ok(json!({
                "results": results.get("results").cloned().unwrap_or(json!([])),
                "receipt": receipt,
            }))
        }

        GRAPH_RECALL => {
            let query = parse_vector(args, "query_vector")?;
            let k = args.get("k").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
            let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(2) as u32;
            let collection = opt_str(args, "collection");
            let query_dim = query.len();

            let resp = client
                .graphrag(query.clone(), k, depth, collection)
                .await
                .context("graphrag failed")?;

            let state_hash = client.proof_state().await.context("fetching state proof")?;
            let (event_log_hash, committed_height) = match client.proof_event_log().await.unwrap_or(None) {
                Some((h, n)) => (Some(h), Some(n)),
                None => (None, None),
            };

            let hits = resp.get("hits").cloned().unwrap_or(json!([]));
            let subgraph = resp.get("subgraph").cloned().unwrap_or(json!({ "nodes": [], "edges": [] }));

            let body = ReceiptBody {
                state_hash,
                event_log_hash,
                committed_height,
                query_dim,
                k,
                // GraphRAG hits use the same {memory_id, record_id, score} shape.
                results: fingerprints_from_results(&json!({ "results": hits })),
                subgraph: Some(subgraph_fingerprint(&subgraph)),
            };
            let receipt = Receipt::build(body, now_unix());

            Ok(json!({
                "hits": hits,
                "subgraph": subgraph,
                "seed_nodes": resp.get("seed_nodes").cloned().unwrap_or(json!([])),
                "receipt": receipt,
            }))
        }

        WHY => {
            let root = args
                .get("root")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow!("`root` (graph node id) is required"))? as u32;
            let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(2) as u32;
            client.subgraph(root, depth).await
        }

        TIMELINE => {
            let from = opt_str(args, "from");
            let to = opt_str(args, "to");
            client.timeline(from, to).await
        }

        FORGET => {
            let key_id = opt_str(args, "key_id")
                .ok_or_else(|| anyhow!("`key_id` (32-hex DEK id) is required"))?;
            let res = client.crypto_shred(key_id).await?;
            Ok(json!({ "deletion_certificate": res }))
        }

        FORK => {
            let res = client.snapshot_save().await?;
            Ok(json!({ "fork": res }))
        }

        other => Err(anyhow!("unknown tool: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::NodeClient;
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// In-memory fake: records calls and returns canned node responses, so we can
    /// prove the tool-composition logic (especially receipt assembly) with no node.
    #[derive(Default)]
    struct FakeNode {
        last_upsert_meta: Mutex<Option<Value>>,
    }

    #[async_trait]
    impl NodeClient for FakeNode {
        async fn memory_upsert(
            &self,
            _vector: Vec<f32>,
            _collection: Option<String>,
            metadata: Option<Value>,
        ) -> Result<Value> {
            *self.last_upsert_meta.lock().unwrap() = metadata;
            Ok(json!({ "memory_id": "mem-1", "record_id": 1,
                       "document_node_id": 10, "chunk_node_id": 11 }))
        }
        async fn memory_search(&self, _q: Vec<f32>, _k: usize, _c: Option<String>, _d: Option<u64>) -> Result<Value> {
            Ok(json!({ "results": [
                { "memory_id": "mem-1", "record_id": 1, "score": 0.9, "metadata": {"text": "hi"} },
                { "memory_id": "mem-2", "record_id": 2, "score": 0.5 }
            ]}))
        }
        async fn proof_state(&self) -> Result<String> {
            Ok("aa".repeat(32))
        }
        async fn proof_event_log(&self) -> Result<Option<(String, u64)>> {
            Ok(Some(("bb".repeat(32), 7)))
        }
        async fn subgraph(&self, root: u32, depth: u32) -> Result<Value> {
            Ok(json!({ "root": root, "depth": depth, "nodes": [], "edges": [] }))
        }
        async fn graphrag(&self, _q: Vec<f32>, _k: usize, _d: u32, _c: Option<String>) -> Result<Value> {
            Ok(json!({
                "hits": [
                    { "memory_id": "mem-1", "record_id": 1, "score": 0.9, "node_id": 11 }
                ],
                "seed_nodes": [11],
                "subgraph": {
                    "nodes": [ { "id": 10, "kind": 0, "record": null },
                               { "id": 11, "kind": 1, "record": 1 } ],
                    "edges": [ { "id": 100, "from": 10, "to": 11, "kind": 0 } ]
                }
            }))
        }
        async fn timeline(&self, _f: Option<String>, _t: Option<String>) -> Result<Value> {
            Ok(json!({ "events": [], "total": 0 }))
        }
        async fn crypto_shred(&self, key_id: String) -> Result<Value> {
            Ok(json!({ "key_id": key_id, "shredded": true }))
        }
        async fn snapshot_save(&self) -> Result<Value> {
            Ok(json!({ "path": "/tmp/snap", "state_hash": "cc" }))
        }
    }

    #[tokio::test]
    async fn lists_all_seven_tools() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 7);
        let names: Vec<&str> = defs.iter().filter_map(|d| d["name"].as_str()).collect();
        for expected in [WRITE, RECALL, GRAPH_RECALL, WHY, TIMELINE, FORGET, FORK] {
            assert!(names.contains(&expected), "missing tool {expected}");
        }
    }

    #[tokio::test]
    async fn graph_recall_binds_hits_and_subgraph_in_receipt() {
        let node = FakeNode::default();
        let args = json!({ "query_vector": [0.1, 0.2, 0.3, 0.4], "k": 1, "depth": 2 });
        let out = call_tool(&node, GRAPH_RECALL, &args).await.unwrap();

        // The composed call returns hits AND the subgraph.
        assert_eq!(out["hits"].as_array().unwrap().len(), 1);
        assert_eq!(out["subgraph"]["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(out["subgraph"]["edges"].as_array().unwrap().len(), 1);

        let receipt = &out["receipt"];
        // Hits are fingerprinted...
        assert_eq!(receipt["results"].as_array().unwrap().len(), 1);
        // ...and so is the subgraph (sorted ids).
        assert_eq!(receipt["subgraph"]["node_ids"], json!([10, 11]));
        assert_eq!(receipt["subgraph"]["edge_ids"], json!([100]));
        assert_eq!(receipt["receipt_digest"].as_str().unwrap().len(), 64);
    }

    #[tokio::test]
    async fn write_folds_text_into_metadata() {
        let node = FakeNode::default();
        let args = json!({ "vector": [0.1, 0.2], "text": "remember me" });
        call_tool(&node, WRITE, &args).await.unwrap();
        let meta = node.last_upsert_meta.lock().unwrap().clone().unwrap();
        assert_eq!(meta["text"], "remember me");
    }

    #[tokio::test]
    async fn recall_attaches_a_receipt() {
        let node = FakeNode::default();
        let args = json!({ "query_vector": [0.1, 0.2, 0.3, 0.4], "k": 2 });
        let out = call_tool(&node, RECALL, &args).await.unwrap();

        let receipt = &out["receipt"];
        assert_eq!(receipt["state_hash"], "aa".repeat(32));
        assert_eq!(receipt["event_log_hash"], "bb".repeat(32));
        assert_eq!(receipt["committed_height"], 7);
        assert_eq!(receipt["query_dim"], 4);
        assert_eq!(receipt["k"], 2);
        let digest = receipt["receipt_digest"].as_str().unwrap();
        assert_eq!(digest.len(), 64);
        // Two returned memories → two fingerprints in the receipt.
        assert_eq!(receipt["results"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn forget_returns_a_deletion_certificate() {
        let node = FakeNode::default();
        let args = json!({ "key_id": "ab".repeat(16) });
        let out = call_tool(&node, FORGET, &args).await.unwrap();
        assert_eq!(out["deletion_certificate"]["shredded"], true);
    }

    #[tokio::test]
    async fn unknown_tool_errors() {
        let node = FakeNode::default();
        let err = call_tool(&node, "memory_nope", &json!({})).await.unwrap_err();
        assert!(err.to_string().contains("unknown tool"));
    }

    #[tokio::test]
    async fn recall_without_query_vector_errors() {
        let node = FakeNode::default();
        let err = call_tool(&node, RECALL, &json!({ "k": 3 })).await.unwrap_err();
        assert!(err.to_string().contains("query_vector"));
    }
}
