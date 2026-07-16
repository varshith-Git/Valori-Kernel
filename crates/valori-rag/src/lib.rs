// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Retrieval-Augmented Generation primitives for the Valori platform.
//!
//! Three RAG modalities are exposed:
//! - **GraphRAG** — vector KNN + BFS subgraph expansion over `KernelState`.
//! - **Tree-RAG** — hierarchical markdown indexing with BLAKE3 receipt chaining.
//! - **Community** — Label Propagation detection + cosine community search.
//! - **LLM** — minimal HTTP wrapper for entity extraction (uses community provider creds).

pub mod graph;
pub mod tree;
pub mod community;
pub mod llm;

// Flat re-exports for the most commonly used items.
pub use graph::{expand_subgraph, resolve_seed_nodes, MAX_DEPTH};
pub use tree::{TreeIndex, TreeNode, Receipt, GENESIS};
pub use community::{
    CommunityStore, CommunitySummary, CommunityHit,
    DetectRequest, DetectResponse, SearchRequest, SearchResponse,
    ExtractEntitiesRequest, ExtractEntitiesResponse,
    InsertedEntity, InsertedRelationship,
    ExtractedEntity, ExtractedRelationship, LlmExtractionOutput,
    label_propagation, build_community_store, rank_communities,
    DEFAULT_MAX_ITER,
};
pub use llm::{LlmConfig, extract_entities_via_llm};
