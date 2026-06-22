// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Newline-delimited JSON-RPC transport over stdio — the MCP stdio binding.
//! stdout carries the protocol; everything diagnostic goes to stderr (writing
//! logs to stdout would corrupt the channel and break the client).

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::backend::NodeClient;
use crate::mcp::McpServer;
use crate::protocol::{codes, Request, Response};

/// Parse one inbound line, dispatch it, and return the line to write back
/// (`None` for notifications, which get no reply). Pure except for the backend
/// call inside `handle`, so it can be driven directly in tests.
pub async fn handle_line<C: NodeClient>(server: &McpServer<C>, line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let req: Request = match serde_json::from_str(trimmed) {
        Ok(r) => r,
        Err(e) => {
            // Couldn't parse — we have no id, so reply with a null-id parse error.
            let resp = Response::error(Value::Null, codes::PARSE_ERROR, format!("parse error: {e}"));
            return Some(serialize_line(&resp));
        }
    };
    server.handle(req).await.map(|resp| serialize_line(&resp))
}

fn serialize_line(resp: &Response) -> String {
    // Responses never contain newlines (serde_json compact output), satisfying
    // the stdio framing rule of one message per line.
    serde_json::to_string(resp).unwrap_or_else(|_| {
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"serialize error"}}"#
            .to_string()
    })
}

/// Run the stdio read→dispatch→write loop until stdin closes.
pub async fn serve<C: NodeClient>(server: McpServer<C>) -> anyhow::Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    eprintln!("valori-mcp: ready on stdio (protocol {})", crate::mcp::PROTOCOL_VERSION);

    while let Some(line) = lines.next_line().await? {
        if let Some(out) = handle_line(&server, &line).await {
            stdout.write_all(out.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use async_trait::async_trait;
    use serde_json::json;

    struct OkNode;

    #[async_trait]
    impl NodeClient for OkNode {
        async fn memory_upsert(&self, _: Vec<f32>, _: Option<String>, _: Option<Value>) -> Result<Value> {
            Ok(json!({}))
        }
        async fn memory_search(&self, _: Vec<f32>, _: usize, _: Option<String>) -> Result<Value> {
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

    #[tokio::test]
    async fn blank_line_is_ignored() {
        let s = McpServer::new(OkNode);
        assert!(handle_line(&s, "   ").await.is_none());
    }

    #[tokio::test]
    async fn garbage_line_yields_parse_error() {
        let s = McpServer::new(OkNode);
        let out = handle_line(&s, "{not json").await.unwrap();
        assert!(out.contains("-32700"));
        // Response must be a single line (no embedded newline).
        assert!(!out.contains('\n'));
    }

    #[tokio::test]
    async fn notification_produces_no_output_line() {
        let s = McpServer::new(OkNode);
        let line = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        assert!(handle_line(&s, line).await.is_none());
    }

    #[tokio::test]
    async fn initialize_roundtrips_through_the_line_handler() {
        let s = McpServer::new(OkNode);
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let out = handle_line(&s, line).await.unwrap();
        assert!(out.contains("valori-mcp"));
        assert!(out.contains("protocolVersion"));
        assert!(!out.contains('\n'));
    }
}
