// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Durable storage layer for the Valori platform.
//!
//! This crate owns everything that persists bytes:
//! - WAL (write-ahead log): `wal_writer`, `wal_reader`
//! - Event log + journal: `events`
//! - Object store (S3/file): `object_store`
//!
//! Recovery orchestration (which files to load, in what order) lives in
//! `valori-state::bootstrap`. This crate provides the raw primitives that
//! bootstrap uses.

pub mod collection_manifest;
pub mod collection_snapshot;
pub mod error;
pub mod events;
pub mod object_store;
pub mod project_manifest;
pub mod provider;
mod wal_compat;
pub mod wal_reader;
pub mod wal_writer;

pub use error::StorageError;

// Note: `provider::StorageError` is a deliberately separate, more granular
// error type for the new logical-artifact storage abstraction (Phase 2,
// collection-storage-foundation) — NOT re-exported here under the same bare
// name as the pre-existing `error::StorageError` (the WAL/event-log layer's
// error type). Reach it as `valori_storage::provider::StorageError`.
