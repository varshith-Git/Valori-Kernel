// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Retrieval-Augmented Generation primitives for the Valori platform.
//!
//! Three RAG modalities are exposed:
//! - **GraphRAG** — vector KNN + BFS subgraph expansion over `KernelState`.
//! - **Tree-RAG** — hierarchical markdown indexing with BLAKE3 receipt chaining.
//! - **Community** — Label Propagation detection + cosine community search.
//! - **LLM** — minimal HTTP wrapper for entity extraction (uses community provider creds).

pub mod community;
pub mod graph;
pub mod llm;
pub mod tree;

// Flat re-exports for the most commonly used items.
pub use community::{
    build_community_store, label_propagation, rank_communities, CommunityHit, CommunityStore,
    CommunitySummary, DetectRequest, DetectResponse, ExtractEntitiesRequest,
    ExtractEntitiesResponse, ExtractedEntity, ExtractedRelationship, InsertedEntity,
    InsertedRelationship, LlmExtractionOutput, SearchRequest, SearchResponse, DEFAULT_MAX_ITER,
};
pub use graph::{expand_subgraph, resolve_seed_nodes, MAX_DEPTH};
pub use llm::{extract_entities_via_llm, LlmConfig};
pub use tree::{Receipt, TreeIndex, TreeNode, GENESIS};
