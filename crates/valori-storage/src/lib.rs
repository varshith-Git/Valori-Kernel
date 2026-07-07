// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Durable storage layer for the Valori platform.
//!
//! This crate owns everything that touches disk:
//! - WAL (write-ahead log): `wal_writer`, `wal_reader`
//! - Event log + journal: `events`
//! - Crash recovery: `recovery`
//! - Object store (S3/file): `object_store`

pub mod wal_writer;
pub mod wal_reader;
pub mod events;
pub mod recovery;
pub mod object_store;

pub use recovery::StorageError;
