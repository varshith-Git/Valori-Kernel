// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `valori-mcp` binary entry point. Connects to a running Valori node over HTTP
//! and serves the Model Context Protocol over stdio.
//!
//! Wire it into Claude Desktop (or any MCP client) with:
//! ```json
//! { "mcpServers": { "valori": { "command": "valori-mcp",
//!   "env": { "VALORI_URL": "http://localhost:3000" } } } }
//! ```

use clap::Parser;
use valori_mcp::backend::HttpBackend;
use valori_mcp::mcp::McpServer;
use valori_mcp::stdio;

#[derive(Parser, Debug)]
#[command(name = "valori-mcp", version, about = "MCP server for verifiable Valori agent memory")]
struct Args {
    /// Base URL of the Valori node to talk to.
    #[arg(long, env = "VALORI_URL", default_value = "http://localhost:3000")]
    url: String,

    /// Bearer token, if the node has auth enabled.
    #[arg(long, env = "VALORI_AUTH_TOKEN")]
    auth_token: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    eprintln!("valori-mcp: backend = {}", args.url);

    let backend = HttpBackend::new(args.url, args.auth_token);
    let server = McpServer::new(backend);
    stdio::serve(server).await
}
