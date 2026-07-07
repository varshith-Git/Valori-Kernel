// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Shared HTTP handler logic — written once, served by BOTH routers.
//!
//! CLAUDE.md documents the dual-path trap: every endpoint must exist in
//! `server.rs` (standalone) AND `cluster_server.rs` (Raft), and historically
//! each file carried its own copy of the handler body. The copies drift:
//! validation applied on one path but not the other, wire formats that
//! silently diverge (see the S12 note in `cluster_server.rs::get_graph_node`),
//! error codes that disagree (drop-unknown-collection was 400 standalone /
//! 404 cluster before this module).
//!
//! The pattern here kills that class of bug:
//!
//! 1. Each domain module (e.g. [`collections`]) defines a small `*Ops` trait —
//!    ONLY the state-touching commit/read primitives, nothing else.
//! 2. The handler body — request validation, special cases, response shaping —
//!    is a shared generic function over that trait. One copy. Any behavior
//!    change automatically applies to both paths.
//! 3. `server.rs` implements the trait on `SharedEngine` (direct engine
//!    locks); `cluster_server.rs` implements it on its data-plane state
//!    (`raft.client_write()` for writes, state-machine reads for reads).
//!    The axum handlers in both files become 3-line wrappers.
//!
//! Adding an endpoint to only one router still fails
//! `tests/route_parity.rs`; this module is how you satisfy it without
//! writing the logic twice.
//!
//! Error convention: shared handlers return `Result<_, axum::response::Response>`
//! — the error side is a fully-formed HTTP response. Validation errors are
//! built through [`crate::errors::EngineError`] so both paths emit the same
//! canonical `{"error": …}` body shape.

pub mod collections;
pub mod graph;
pub mod memory;
pub mod meta;
pub mod records;

/// `GET /v1/version` — stateless, literally the same function on both routers.
pub async fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
