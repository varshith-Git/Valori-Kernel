// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Pure orchestration helpers for Phase 5 cross-collection (multi) search.
//!
//! This module is deliberately free of axum / tokio / engine references so the
//! two call sites (`server.rs` and `cluster_server.rs`) can test it in isolation.
//!
//! # Design invariants
//! - Score semantics: Squared L2, smaller = better.  No normalization.
//! - BM25 reranking is NOT applied: hybrid scores from different Collection
//!   corpora are incomparable and would corrupt the cross-collection merge.
//! - Graph reranking is NOT applied: graph edges are Collection-scoped.
//! - `merge_top_k` is a pure sort + truncate — no score mutation.

use valori_domain::Metric;
use valori_metadata::collection::CollectionVectorConfig;

use crate::api::{MultiSearchHit, PartialSearchFailure};

/// Hard ceiling on the number of collections in one multi-search request.
pub const MAX_MULTI_COLLECTIONS: usize = 32;

/// Hard ceiling on `k` in a multi-search request.
/// Mirrors `server.rs::MAX_SEARCH_K` and `cluster_server.rs::MAX_SEARCH_K`.
pub const MAX_MULTI_SEARCH_K: usize = 5000;

/// Check that every collection in `configs` shares the same `dim` and `metric`.
///
/// `configs` must be non-empty (caller's responsibility).
/// Returns `(dim, metric)` on success.
pub fn check_compatibility(
    configs: &[(String, CollectionVectorConfig)],
) -> Result<(u32, Metric), String> {
    let (first_name, first_cfg) = &configs[0];
    for (name, cfg) in configs.iter().skip(1) {
        if cfg.dim != first_cfg.dim {
            return Err(format!(
                "dimension mismatch: '{}' has dim={} but '{}' has dim={}; \
                 all collections in a multi-search must share the same dimension",
                first_name, first_cfg.dim, name, cfg.dim
            ));
        }
        if cfg.metric != first_cfg.metric {
            return Err(format!(
                "metric mismatch: '{}' uses {:?} but '{}' uses {:?}; \
                 all collections in a multi-search must share the same metric",
                first_name, first_cfg.metric, name, cfg.metric
            ));
        }
    }
    Ok((first_cfg.dim, first_cfg.metric))
}

/// A per-collection search result before global merge.
pub struct CollectionHits {
    pub collection: String,
    pub hits: Vec<MultiSearchHit>,
}

/// Merge per-collection hits into a single global top-k list.
///
/// Sort criterion: `score` ascending (Squared L2, smaller = better).
/// No score transformation is applied.
pub fn merge_top_k(
    per_collection: Vec<CollectionHits>,
    partial_failures: Vec<PartialSearchFailure>,
    k: usize,
) -> crate::api::MultiSearchResponse {
    let collections_searched: Vec<String> = per_collection
        .iter()
        .map(|c| c.collection.clone())
        .collect();

    let mut all: Vec<MultiSearchHit> = per_collection.into_iter().flat_map(|c| c.hits).collect();

    all.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    all.truncate(k);

    crate::api::MultiSearchResponse {
        results: all,
        collections_searched,
        partial_failures: if partial_failures.is_empty() {
            None
        } else {
            Some(partial_failures)
        },
    }
}
