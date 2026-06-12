// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
pub mod config;
pub mod errors;
pub mod api;
pub mod engine;
pub mod server;
pub mod structure;
pub mod metadata;
pub mod persistence;
pub mod wal_writer;
pub mod wal_reader;
pub mod recovery;
pub mod telemetry;
pub mod events;
pub mod replication;
pub mod network;
/// Phase 1.9: Committer trait seam (skeleton present; Engine wiring in Phase 1.9).
/// See docs/phases/phase-1.9-committer-trait.md
pub mod commit;
/// Phase 2.5: cluster bootstrap — standalone vs cluster is a boot-time decision.
/// See docs/phases/phase-2.5-raft-committer.md
pub mod cluster;
/// Phase 2.6: cluster management HTTP API (status, health, membership).
/// See docs/phases/phase-2.6-cluster-api.md
pub mod cluster_api;
