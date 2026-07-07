// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
pub mod config;
pub mod errors;
/// BM25 post-retrieval reranker — hybrid vector + term-frequency scoring.
pub mod valori_reranker;
pub mod api;
pub mod engine;
pub mod graph_rag;
pub mod decay;
pub mod server;
/// Server-side document ingestion with built-in chunking strategies.
pub mod ingest;
/// Tree-RAG — hierarchical retrieval with breadcrumb citations + replayable receipts.
pub mod tree_rag;
/// HTTP embedding client — drives VALORI_EMBED_PROVIDER for on-node embedding.
pub mod embedder;
pub mod structure;
pub mod metadata;
pub mod persistence;
pub mod telemetry;
// Storage layer now lives in valori-storage; re-export here so all existing
// `crate::wal_writer::*`, `crate::events::*`, etc. imports still compile.
pub use valori_storage::wal_writer;
pub use valori_storage::wal_reader;
pub use valori_storage::events;
pub use valori_storage::object_store;
// State lifecycle layer lives in valori-state; re-export bootstrap as `recovery`
// so existing `crate::recovery::recover_from_events` call sites still compile.
pub use valori_state::bootstrap as recovery;
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
/// Cluster-mode HTTP server: data plane over Raft (insert/search/health).
pub mod cluster_server;
// object_store is re-exported from valori_storage above.
/// Phase 3.5: Per-tenant API keys + RBAC.
pub mod api_keys;
/// Phase 3.6: AES-256-GCM vault for crypto-shredding (GDPR erasure).
pub mod crypto_vault;
pub mod community;
/// Phase A7: Concrete capability implementations (EngineKernelCapability, HttpEmbedCapability).
pub mod capabilities;
/// Phase A10: Receipt bridge — emits ReceiptAssembler receipts from existing HTTP handlers.
pub mod receipt_bridge;
/// Phase A7: TaskRunner drives ExecutionGraph → Task::run in topological order.
pub mod runner;
