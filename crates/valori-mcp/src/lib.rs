// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! valori-mcp — a Model Context Protocol server that exposes a Valori node as
//! verifiable, deterministic long-term memory for agents.
//!
//! Layers (bottom-up):
//! - [`protocol`] — JSON-RPC 2.0 envelope (transport-agnostic).
//! - [`receipt`] — retrieval receipts: the BLAKE3 binding of a recall to state.
//! - [`backend`] — [`backend::NodeClient`] trait + HTTP impl over node endpoints.
//! - [`tools`]   — the six MCP tools, composed from `NodeClient` primitives.
//! - [`mcp`]     — MCP method dispatch (`initialize`/`tools.list`/`tools.call`).
//! - [`stdio`]   — newline-delimited JSON-RPC over stdin/stdout.

pub mod backend;
pub mod mcp;
pub mod protocol;
pub mod receipt;
pub mod stdio;
pub mod tools;
