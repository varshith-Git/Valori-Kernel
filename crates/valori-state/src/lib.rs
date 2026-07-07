// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! State lifecycle for the Valori platform.
//!
//! `valori-state` owns everything that orchestrates *how* `KernelState` moves
//! between durable storage and in-memory operation:
//!
//! - [`bootstrap`] — crash recovery via event log replay, WAL replay, or snapshot.
//! - [`manifest`]  — `StateManifest`: which files make up the current durable state.
//! - [`lifecycle`] — `StateLifecycle`: Recovering / Ready / Snapshotting.
//! - [`shutdown`]  — graceful snapshot-on-close before process exit.
//!
//! Raw byte movement (WAL append, event log format) remains in `valori-storage`.
//! The divide: `valori-storage` = bytes on disk; `valori-state` = state lifecycle.

pub mod bootstrap;
pub mod error;
pub mod lifecycle;
pub mod manifest;
pub mod shutdown;

pub use error::{StateError, StateResult};
pub use lifecycle::StateLifecycle;
pub use manifest::StateManifest;
