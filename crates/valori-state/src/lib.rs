// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! State lifecycle orchestration for the Valori platform.
//!
//! `valori-state` owns everything that orchestrates *how* `KernelState` moves
//! between durable storage and in-memory operation:
//!
//! - [`bootstrap`] — crash recovery via event log replay, WAL replay, or snapshot.
//! - [`error`]     — `StateError` / `StateResult`.
//!
//! Raw byte movement (WAL append, event log format) remains in `valori-storage`.
//! The divide: `valori-storage` = bytes on disk; `valori-state` = state lifecycle.

pub mod bootstrap;
pub mod error;

pub use error::{StateError, StateResult};
