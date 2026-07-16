// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! # valori-search
//!
//! Post-retrieval search primitives used by every Valori execution path
//! (standalone, cluster, FFI, MCP). Three independent, pure modules:
//!
//! | Module | Responsibility |
//! |--------|---------------|
//! | [`decay`] | Time-decay re-ranking — penalise old records by inflating their L2 distance |
//! | [`reranker`] | BM25 hybrid reranker — blend vector similarity with term-frequency scoring |
//! | [`filter`] | Metadata predicate matching — exact equality and numeric range operators |
//!
//! ## Design invariants
//!
//! - **No kernel dependency.** This crate operates on raw numeric types and
//!   `serde_json` values. It knows nothing about `KernelState`, `Engine`, or
//!   Raft. Adding such a dependency here is a bug.
//! - **No I/O, no async.** Every function is synchronous and pure. Callers own
//!   all state; this crate only transforms it.
//! - **Deterministic output.** Tie-breaking in sort operations uses record ID
//!   ascending so the same input always produces the same ranked output.
//! - **No mutation of audit state.** None of these operations emit kernel
//!   events or affect the BLAKE3 state hash.

pub mod decay;
pub mod filter;
pub mod reranker;

// ── Convenient re-exports ─────────────────────────────────────────────────────

pub use decay::{decay_factor, rerank as decay_rerank, DecayHit, DecayedHit};
pub use filter::{matches_metadata_filter, MetadataFilter};
pub use reranker::{tokenise, ValoriReranker, POOL_FACTOR};
