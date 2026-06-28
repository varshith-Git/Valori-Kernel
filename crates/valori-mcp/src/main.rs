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

    // M-4: warn when the auth token will be sent over a plaintext connection to
    // a remote host. http://localhost and http://127.0.0.1 are acceptable for
    // local dev; anything else leaks the bearer token on the wire.
    if args.auth_token.is_some() {
        let lower = args.url.to_lowercase();
        let is_plaintext = lower.starts_with("http://");
        let is_local = lower.contains("://localhost") || lower.contains("://127.0.0.1");
        if is_plaintext && !is_local {
            eprintln!(
                "WARNING: VALORI_AUTH_TOKEN is set but VALORI_URL uses http:// to a \
                 non-localhost host. The bearer token will be sent in PLAINTEXT over \
                 the network. Use https:// or restrict to localhost."
            );
        }
    }

    let backend = HttpBackend::new(args.url, args.auth_token);
    let server = McpServer::new(backend);
    stdio::serve(server).await
}
