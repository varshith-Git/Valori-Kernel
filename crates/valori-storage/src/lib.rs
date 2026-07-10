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

pub mod error;
pub mod wal_writer;
pub mod wal_reader;
mod wal_compat;
pub mod events;
pub mod object_store;

pub use error::StorageError;
