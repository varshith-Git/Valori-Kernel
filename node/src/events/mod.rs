// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Event-Sourced Persistence Layer
//!
//! This module provides the canonical event log infrastructure for Valori Node.
//!
//! # Architecture
//! - Event Log = Primary truth (append-only, durable)
//! - Snapshots = Performance optimization (disposable)
//! - Journal = Runtime state (buffer + committed)
//!
//! # Guarantees
//! - Events are fsync'd before application
//! - Crash-symmetric recovery via replay
//! - No partial commits
//! - Deterministic across architectures

pub mod event_log;
pub mod event_journal;
pub mod event_replay;
pub mod event_commit;
pub mod event_proof;

pub use event_log::EventLogWriter;
pub use event_journal::EventJournal;
pub use event_replay::recover_from_event_log;
pub use event_commit::{CommitResult, EventCommitter};
pub use event_proof::EventProof;
