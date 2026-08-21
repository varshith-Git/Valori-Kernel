// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! G1.4.1 — graph-aware reranking.
//!
//! A **read-time re-rank**, in the same family as [`crate::decay`]: never
//! mutates kernel state, never emits a committed event, never affects the
//! BLAKE3 state hash. It only perturbs the order of an already-fetched
//! candidate pool, using a per-candidate graph-distance signal the caller
//! has already computed (`valori_rag::graph::graph_distances_from_seeds`,
//! reduced per record to the minimum distance across that record's live
//! graph nodes — see docs/reviews/graph-g1.4.1-graph-aware-reranking-design.md
//! §3/§4 for why hop-count + minimum was chosen over the alternatives).
//!
//! # Model
//!
//! Candidate scores are ascending — lower is better (the same convention
//! every index and [`crate::decay`] already use). Graph distance is
//! "badness" (more hops = worse), so it inflates the score exactly the way
//! [`crate::decay`] inflates an old record's distance, but as a multiplier
//! rather than a division:
//!
//! ```text
//! adjusted = score × (1 + weight × distance)
//! ```
//!
//! `distance == 0` (the candidate's own node IS a seed) → multiplier `1.0`,
//! no change. A candidate with no graph distance at all (no graph node, or
//! unreachable within the configured `max_depth`) is never penalized —
//! `None` also multiplies by `1.0` — it simply keeps its pre-rerank order
//! relative to other `None` candidates. This is a deliberate design
//! decision (§8 of the design doc): missing graph data must never cause a
//! vector hit to be dropped or artificially demoted.

/// A scored candidate entering the graph re-ranker. `score` is whatever the
/// upstream pipeline already produced — raw L2 distance, BM25-blended, or
/// decay-adjusted (G1.4.1 composes with either, see design doc §8).
#[derive(Clone, Copy, Debug)]
pub struct GraphRerankHit {
    pub id: u32,
    pub score: f32,
    /// Hop distance to the nearest seed, already reduced to the minimum
    /// across the record's live graph nodes. `None` = no graph node, or
    /// unreachable within `max_depth`.
    pub graph_distance: Option<u32>,
}

/// The result of applying graph rerank to one hit.
#[derive(Clone, Copy, Debug)]
pub struct GraphRerankedHit {
    pub id: u32,
    /// Original, unperturbed score — preserved for auditability, same
    /// convention as `DecayedHit::distance`.
    pub score: f32,
    pub graph_distance: Option<u32>,
    /// Internal ordering key: `score × (1 + weight × distance)`. Lower
    /// ranks first. `None` distance ⇒ equal to `score` (no penalty).
    adjusted: f64,
}

/// Multiplicative graph-distance penalty in `[1.0, ∞)`.
///
/// `distance = None` or `distance = 0` → `1.0` (no penalty). Otherwise
/// `1.0 + weight × distance`. `weight` is expected in `[0.0, 1.0]` (callers
/// clamp before this call — this function does not re-clamp, so it stays a
/// pure, total function of its inputs, matching `decay::decay_factor`'s
/// shape).
#[inline]
pub fn graph_penalty(distance: Option<u32>, weight: f32) -> f64 {
    match distance {
        None | Some(0) => 1.0,
        Some(d) => 1.0 + (weight as f64) * (d as f64),
    }
}

/// Re-rank `hits` by graph-penalized score and return the top `k`.
///
/// Ordering is ascending by `adjusted`, ties broken by `id` ascending —
/// identical convention to [`crate::decay::rerank`] and every vector index's
/// own comparator. Deterministic: `adjusted` is a pure function of
/// `(score, graph_distance, weight)`, so identical inputs always produce
/// identical output regardless of input `Vec` order.
pub fn rerank(hits: Vec<GraphRerankHit>, weight: f32, k: usize) -> Vec<GraphRerankedHit> {
    let mut out: Vec<GraphRerankedHit> = hits
        .into_iter()
        .map(|h| {
            let penalty = graph_penalty(h.graph_distance, weight);
            GraphRerankedHit {
                id: h.id,
                score: h.score,
                graph_distance: h.graph_distance,
                adjusted: h.score as f64 * penalty,
            }
        })
        .collect();

    out.sort_by(|a, b| {
        a.adjusted
            .partial_cmp(&b.adjusted)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.id.cmp(&b.id))
    });
    out.truncate(k);
    out
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn penalty_endpoints() {
        assert_eq!(graph_penalty(None, 0.5), 1.0);
        assert_eq!(graph_penalty(Some(0), 0.5), 1.0);
        assert_eq!(graph_penalty(Some(1), 0.5), 1.5);
        assert!((graph_penalty(Some(2), 0.15) - 1.3).abs() < 1e-6);
    }

    #[test]
    fn closer_candidate_overtakes_a_slightly_better_but_farther_one() {
        let hits = vec![
            GraphRerankHit {
                id: 1,
                score: 1.0,
                graph_distance: Some(2), // adjusted = 1.0 * 1.3 = 1.3
            },
            GraphRerankHit {
                id: 2,
                score: 1.2,
                graph_distance: Some(0), // adjusted = 1.2 * 1.0 = 1.2
            },
        ];
        let ranked = rerank(hits, 0.15, 2);
        assert_eq!(ranked[0].id, 2, "graph-adjacent candidate overtakes");
        assert_eq!(ranked[1].id, 1);
        assert_eq!(ranked[0].score, 1.2, "original score preserved");
    }

    #[test]
    fn missing_graph_distance_is_neutral_never_penalized() {
        let hits = vec![
            GraphRerankHit {
                id: 1,
                score: 1.0,
                graph_distance: None,
            },
            GraphRerankHit {
                id: 2,
                score: 2.0,
                graph_distance: None,
            },
        ];
        let ranked = rerank(hits, 0.5, 2);
        assert_eq!(
            ranked[0].id, 1,
            "no graph data keeps pure vector-score order"
        );
    }

    #[test]
    fn zero_weight_never_changes_order() {
        let hits = vec![
            GraphRerankHit {
                id: 1,
                score: 1.0,
                graph_distance: Some(4),
            },
            GraphRerankHit {
                id: 2,
                score: 2.0,
                graph_distance: Some(0),
            },
        ];
        let ranked = rerank(hits, 0.0, 2);
        assert_eq!(
            ranked.iter().map(|h| h.id).collect::<Vec<_>>(),
            vec![1, 2],
            "weight=0 must be a strict no-op on ordering"
        );
    }

    #[test]
    fn ties_break_by_id_ascending() {
        let hits = vec![
            GraphRerankHit {
                id: 9,
                score: 1.0,
                graph_distance: None,
            },
            GraphRerankHit {
                id: 2,
                score: 1.0,
                graph_distance: None,
            },
            GraphRerankHit {
                id: 5,
                score: 1.0,
                graph_distance: None,
            },
        ];
        let ranked = rerank(hits, 0.5, 3);
        assert_eq!(
            ranked.iter().map(|h| h.id).collect::<Vec<_>>(),
            vec![2, 5, 9]
        );
    }

    #[test]
    fn truncates_to_k() {
        let hits = (0..5)
            .map(|i| GraphRerankHit {
                id: i,
                score: i as f32,
                graph_distance: None,
            })
            .collect();
        let ranked = rerank(hits, 0.1, 2);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].id, 0);
        assert_eq!(ranked[1].id, 1);
    }

    #[test]
    fn exact_match_at_distance_zero_stays_best() {
        let hits = vec![
            GraphRerankHit {
                id: 1,
                score: 0.0,
                graph_distance: Some(4),
            },
            GraphRerankHit {
                id: 2,
                score: 0.1,
                graph_distance: Some(0),
            },
        ];
        let ranked = rerank(hits, 1.0, 2);
        assert_eq!(ranked[0].id, 1, "score 0 * anything == 0, always best");
    }
}
