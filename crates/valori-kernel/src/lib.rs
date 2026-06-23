//! # valori-kernel (root crate) — **LEGACY**
//!
//! This crate is the original prototype implementation of the Valori vector
//! kernel.  It is **not** the production path.
//!
//! ## Production path
//! All new code lives in `node/` (`valori-node` crate):
//! - [`valori_node::engine::Engine`] — the non-generic, heap-allocated engine
//! - [`valori_node::config::NodeConfig`] — runtime configuration
//! - WAL, event log, replication, graph — all in `node/src/`
//!
//! ## What this crate still provides
//! Shared types used by `valori-node` as a dependency:
//! - `KernelState`, `Command`, `RecordId` (via `state/`, `types/`)
//! - Snapshot encode/decode (`snapshot/`)
//! - The `#[deprecated]` `ValoriKernel` struct (old HNSW prototype)
//!
//! The `ValoriKernel` struct will be removed in a future release.
//! CLI bench binaries in `crates/cli/src/bin/` that still reference it
//! will be migrated to `Engine` before the 1.0 release.

// ARCHITECTURAL INVARIANT: valori-kernel must remain no_std.
// Never add `use std::` to any file in this crate. Use `core::` or `alloc::` instead.
// Anything requiring std must be gated behind `#[cfg(feature = "std")]`.
// Verify after every change: `cargo build -p valori-kernel --target wasm32-unknown-unknown`
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;
pub mod config;
pub mod crypto;
pub mod error;
pub mod event;
pub mod fxp;
pub mod graph;
#[cfg(feature = "std")]
pub mod kernel;
pub mod types;
pub mod snapshot;
#[cfg(feature = "std")]
pub mod adapters;
#[cfg(feature = "std")]
pub mod hnsw;
pub mod index;
pub mod math;
pub mod proof;
pub mod replay;
pub mod state;
pub mod storage;
pub mod verify;
pub mod dist;



#[cfg(feature = "std")]
#[allow(deprecated)]
pub use kernel::ValoriKernel;
