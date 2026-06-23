// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! MCP method handling on top of the JSON-RPC envelope. Translates the three
//! methods we care about — `initialize`, `tools/list`, `tools/call` — plus
//! `ping`, into [`NodeClient`] calls. Transport (stdio) lives elsewhere.

use serde_json::{json, Value};

use crate::backend::NodeClient;
use crate::protocol::{codes, Request, Response};
use crate::tools;

/// MCP protocol revision we implement. Stable, widely supported by current
/// clients (Claude Desktop, the reference SDKs).
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// The MCP server: owns a [`NodeClient`] and answers protocol messages.
pub struct McpServer<C: NodeClient> {
    client: C,
}

impl<C: NodeClient> McpServer<C> {
    pub fn new(client: C) -> Self {
        Self { client }
    }

    /// Handle one parsed request. Returns `None` for notifications (which must
    /// not be answered per JSON-RPC 2.0), otherwise the response to write back.
    pub async fn handle(&self, req: Request) -> Option<Response> {
        if req.is_notification() {
            // e.g. `notifications/initialized` — acknowledge by doing nothing.
            return None;
        }
        // Safe: not a notification ⇒ id is present.
        let id = req.id.clone().unwrap_or(Value::Null);

        let resp = match req.method.as_str() {
            "initialize" => Response::success(id, self.initialize_result()),
            "ping" => Response::success(id, json!({})),
            "tools/list" => {
                Response::success(id, json!({ "tools": tools::tool_definitions() }))
            }
            "tools/call" => self.handle_tools_call(id, &req.params).await,
            other => Response::error(
                id,
                codes::METHOD_NOT_FOUND,
                format!("method not found: {other}"),
            ),
        };
        Some(resp)
    }

    fn initialize_result(&self) -> Value {
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "valori-mcp",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions":
                "Verifiable, deterministic long-term memory for agents. \
                 memory_recall returns a BLAKE3 receipt proving what was retrieved \
                 against the committed state."
        })
    }

    async fn handle_tools_call(&self, id: Value, params: &Value) -> Response {
        let name = match params.get("name").and_then(|n| n.as_str()) {
            Some(n) => n,
            None => {
                return Response::error(id, codes::INVALID_PARAMS, "tools/call missing `name`");
            }
        };
        let args = params.get("arguments").cloned().unwrap_or(json!({}));

        match tools::call_tool(&self.client, name, &args).await {
            Ok(payload) => {
                let text = serde_json::to_string_pretty(&payload)
                    .unwrap_or_else(|_| payload.to_string());
                // MCP tool results are a content array. We return one text block
                // carrying the JSON payload.
                Response::success(
                    id,
                    json!({
                        "content": [ { "type": "text", "text": text } ],
                        "isError": false
                    }),
                )
            }
            // Per MCP, tool-execution failures are reported as a result with
            // isError=true (so the model sees them) rather than a JSON-RPC error.
            Err(e) => Response::success(
                id,
                json!({
                    "content": [ { "type": "text", "text": format!("error: {e:#}") } ],
                    "isError": true
                }),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Request;
    use anyhow::Result;
    use async_trait::async_trait;

    struct OkNode;

    #[async_trait]
    impl NodeClient for OkNode {
        async fn memory_upsert(&self, _: Vec<f32>, _: Option<String>, _: Option<Value>) -> Result<Value> {
            Ok(json!({ "memory_id": "m", "record_id": 1 }))
        }
        async fn memory_search(&self, _: Vec<f32>, _: usize, _: Option<String>, _: Option<u64>) -> Result<Value> {
            Ok(json!({ "results": [] }))
        }
        async fn proof_state(&self) -> Result<String> { Ok("aa".repeat(32)) }
        async fn proof_event_log(&self) -> Result<Option<(String, u64)>> { Ok(None) }
        async fn subgraph(&self, _: u32, _: u32) -> Result<Value> { Ok(json!({})) }
        async fn graphrag(&self, _: Vec<f32>, _: usize, _: u32, _: Option<String>) -> Result<Value> {
            Ok(json!({ "hits": [], "subgraph": { "nodes": [], "edges": [] } }))
        }
        async fn timeline(&self, _: Option<String>, _: Option<String>) -> Result<Value> { Ok(json!({})) }
        async fn crypto_shred(&self, _: String) -> Result<Value> { Ok(json!({})) }
        async fn snapshot_save(&self) -> Result<Value> { Ok(json!({})) }
    }

    fn req(method: &str, params: Value, id: Option<i64>) -> Request {
        serde_json::from_value(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn initialize_advertises_tools_capability() {
        let s = McpServer::new(OkNode);
        let r = s.handle(req("initialize", json!({}), Some(1))).await.unwrap();
        let result = r.result.unwrap();
        assert_eq!(result["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(result["serverInfo"]["name"], "valori-mcp");
        assert!(result["capabilities"]["tools"].is_object());
    }

    #[tokio::test]
    async fn tools_list_returns_seven() {
        let s = McpServer::new(OkNode);
        let r = s.handle(req("tools/list", json!({}), Some(2))).await.unwrap();
        let tools = r.result.unwrap();
        assert_eq!(tools["tools"].as_array().unwrap().len(), 7);
    }

    #[tokio::test]
    async fn notification_gets_no_response() {
        let s = McpServer::new(OkNode);
        let r = s.handle(req("notifications/initialized", json!({}), None)).await;
        assert!(r.is_none());
    }

    #[tokio::test]
    async fn unknown_method_is_jsonrpc_error() {
        let s = McpServer::new(OkNode);
        let r = s.handle(req("foo/bar", json!({}), Some(3))).await.unwrap();
        assert_eq!(r.error.unwrap().code, codes::METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn tool_error_is_reported_as_iserror_result() {
        let s = McpServer::new(OkNode);
        // Unknown tool → tool-level error, surfaced as isError result not RPC error.
        let r = s
            .handle(req("tools/call", json!({ "name": "nope", "arguments": {} }), Some(4)))
            .await
            .unwrap();
        let result = r.result.unwrap();
        assert_eq!(result["isError"], true);
        assert!(result["content"][0]["text"].as_str().unwrap().contains("unknown tool"));
    }

    #[tokio::test]
    async fn tools_call_without_name_is_invalid_params() {
        let s = McpServer::new(OkNode);
        let r = s.handle(req("tools/call", json!({ "arguments": {} }), Some(5))).await.unwrap();
        assert_eq!(r.error.unwrap().code, codes::INVALID_PARAMS);
    }
}
