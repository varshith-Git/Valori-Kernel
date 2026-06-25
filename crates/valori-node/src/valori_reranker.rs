/// Valori BM25 Reranker
///
/// Post-retrieval reranker: the kernel returns top-K record IDs by vector
/// similarity (L2); this module re-scores those candidates with BM25 and
/// returns a reordered list so exact and rare-term matches rise above
/// semantically-similar-but-wrong results.
///
/// # Where it fits
///
///   /search handler
///       └─ engine.search_l2_ns(query_vec, k * POOL_FACTOR)  ← wider candidate pool
///           └─ ValoriReranker::rerank(query_text, candidates) ← BM25 re-order
///               └─ return top k                               ← to client
///
/// # BM25 Okapi formula (per query term t, document d)
///
///   score(t,d) = IDF(t) × tf(t,d)×(k1+1) / (tf(t,d) + k1×(1−b + b×|d|/avgdl))
///   IDF(t)     = ln((N − df(t) + 0.5) / (df(t) + 0.5) + 1)
///
///   k1 = 1.5   — term-frequency saturation
///   b  = 0.75  — document-length normalisation
///
/// # Text storage
///
/// Text is stored in a `HashMap<u64, Vec<String>>` (record_id → tokens).
/// This lives in valori-node RAM alongside the kernel state.
/// Insert cost: tokenise once at ingest time.
/// Search cost: O(|candidates| × |query_terms|) — negligible for k ≤ 100.
///
/// # no_std note
///
/// This module is in valori-node (std). It must NEVER be moved to
/// valori-kernel; the kernel is no_std and must stay that way.

use std::collections::HashMap;

/// Standard BM25 Okapi parameters — well-established defaults.
const K1: f32 = 1.5;
const B: f32 = 0.75;
/// How many extra candidates to fetch from the kernel before reranking.
/// E.g. user asks for k=5 → we fetch k×POOL = 20, rerank, return top 5.
/// How many vector candidates to fetch before BM25 reranking.
/// Set high so BM25 sees the entire collection on small indices
/// (e.g. 15 tree nodes) — BM25 needs the full pool to win over
/// the wrong-but-semantically-close vector hit.
pub const POOL_FACTOR: usize = 20;

/// Holds the tokenised text corpus for BM25 scoring.
///
/// One instance lives inside `Engine` / `ClusterHandle` alongside the kernel.
#[derive(Default)]
pub struct ValoriReranker {
    /// record_id → tokenised document (lowercase words, ≥ 2 chars)
    corpus: HashMap<u64, Vec<String>>,
    /// running total of document lengths (for avgdl)
    total_tokens: usize,
}

impl ValoriReranker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register text for a newly inserted record.
    /// Call this immediately after the kernel assigns a record ID.
    pub fn insert(&mut self, record_id: u64, text: &str) {
        let tokens = tokenise(text);
        self.total_tokens += tokens.len();
        self.corpus.insert(record_id, tokens);
    }

    /// Remove a record's text (called on soft-delete or drop-collection).
    pub fn remove(&mut self, record_id: u64) {
        if let Some(tokens) = self.corpus.remove(&record_id) {
            self.total_tokens = self.total_tokens.saturating_sub(tokens.len());
        }
    }

    /// Remove all records belonging to a collection (drop-collection path).
    pub fn remove_batch(&mut self, record_ids: &[u64]) {
        for id in record_ids {
            self.remove(*id);
        }
    }

    /// Re-rank `candidates` (record_id, vector_score) by BM25 against `query`.
    ///
    /// Returns the same candidates sorted by a combined hybrid score:
    ///   hybrid = α × normalised_vector_score + (1−α) × normalised_bm25_score
    ///   α = 0.5 (equal weight; tune later via config if needed)
    ///
    /// Records with no stored text are kept at their original vector rank.
    pub fn rerank(
        &self,
        query: &str,
        candidates: Vec<(u64, f32)>,
    ) -> Vec<(u64, f32)> {
        if candidates.is_empty() {
            return candidates;
        }

        let q_terms = tokenise(query);
        if q_terms.is_empty() {
            return candidates;
        }

        let n_docs = self.corpus.len() as f32;
        let avgdl = if n_docs > 0.0 {
            self.total_tokens as f32 / n_docs
        } else {
            1.0
        };

        // IDF computed over the whole corpus (standard BM25)
        let idf: HashMap<&str, f32> = q_terms
            .iter()
            .map(|t| {
                let df = self.corpus.values().filter(|doc| doc.contains(t)).count() as f32;
                let score = ((n_docs - df + 0.5) / (df + 0.5) + 1.0).ln();
                (t.as_str(), score.max(0.0))
            })
            .collect();

        // BM25 score per candidate
        let bm25_raw: Vec<f32> = candidates
            .iter()
            .map(|(rid, _)| {
                let doc = match self.corpus.get(rid) {
                    Some(d) => d,
                    None => return 0.0,
                };
                let doc_len = doc.len() as f32;
                let mut tf_map: HashMap<&str, f32> = HashMap::new();
                for tok in doc {
                    *tf_map.entry(tok.as_str()).or_insert(0.0) += 1.0;
                }
                q_terms.iter().fold(0.0_f32, |acc, t| {
                    let tf = *tf_map.get(t.as_str()).unwrap_or(&0.0);
                    let idf_t = *idf.get(t.as_str()).unwrap_or(&0.0);
                    let num = tf * (K1 + 1.0);
                    let den = tf + K1 * (1.0 - B + B * doc_len / avgdl);
                    acc + idf_t * num / den.max(1e-9)
                })
            })
            .collect();

        // Min-max normalise both score arrays to [0, 1] for fair combination.
        let v_scores: Vec<f32> = candidates.iter().map(|(_, s)| *s).collect();
        let v_norm = normalise(&v_scores);
        // Vector scores are L2 distances — lower is better. Flip for normalisation.
        let v_norm: Vec<f32> = v_norm.iter().map(|x| 1.0 - x).collect();
        let b_norm = normalise(&bm25_raw);

        // Hybrid score: equal-weight blend
        let alpha = 0.5_f32;
        let mut scored: Vec<(u64, f32)> = candidates
            .iter()
            .enumerate()
            .map(|(i, (rid, _))| {
                let hybrid = alpha * v_norm[i] + (1.0 - alpha) * b_norm[i];
                (*rid, hybrid)
            })
            .collect();

        // Sort descending by hybrid score
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }

    /// Total number of records with stored text.
    pub fn len(&self) -> usize {
        self.corpus.len()
    }

    pub fn is_empty(&self) -> bool {
        self.corpus.is_empty()
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Tokenise text into lowercase words ≥ 2 chars.
/// Consistent with the Python-side tokeniser in tree_rag.py.
pub fn tokenise(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 2)
        .map(|w| w.to_lowercase())
        .collect()
}

/// Min-max normalise a slice to [0, 1].
/// If all values are equal, returns a zero vector (no preference).
fn normalise(values: &[f32]) -> Vec<f32> {
    let min = values.iter().cloned().fold(f32::INFINITY, f32::min);
    let max = values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let range = max - min;
    if range < 1e-9 {
        return vec![0.0; values.len()];
    }
    values.iter().map(|v| (v - min) / range).collect()
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_reranker() -> ValoriReranker {
        let mut r = ValoriReranker::new();
        r.insert(1, "The optimizer used is AdamW with weight decay during continued pretraining");
        r.insert(2, "Reinforcement learning uses Adam optimizer in the single epoch regime");
        r.insert(3, "Context Parallelism replaced Tensor Parallelism for long context scaling");
        r.insert(4, "Agent behavior includes penalties for leaving to-do lists unfinished");
        r.insert(5, "Public benchmarks like SWE-bench measure software engineering performance");
        r
    }

    #[test]
    fn reranks_exact_term_above_semantic_neighbour() {
        let r = make_reranker();
        // Vector search hypothetically returns [2, 1, 3] (record 2 ranked first by vector)
        // but query mentions AdamW which only appears in record 1
        let candidates = vec![(2, 1.0), (1, 1.5), (3, 2.0)];
        let reranked = r.rerank("AdamW optimizer pretraining", candidates);
        // record 1 should rise to top after BM25 rerank
        assert_eq!(reranked[0].0, 1, "AdamW should rank first");
    }

    #[test]
    fn exact_phrase_wins() {
        let r = make_reranker();
        let candidates = vec![(1, 1.0), (2, 1.1), (3, 1.2), (4, 1.3), (5, 1.4)];
        let reranked = r.rerank("Context Parallelism long context", candidates);
        assert_eq!(reranked[0].0, 3, "Context Parallelism section should win");
    }

    #[test]
    fn empty_candidates_returns_empty() {
        let r = make_reranker();
        assert!(r.rerank("anything", vec![]).is_empty());
    }

    #[test]
    fn unknown_record_id_does_not_panic() {
        let r = make_reranker();
        // record 99 has no stored text
        let candidates = vec![(99, 1.0), (1, 1.5)];
        let reranked = r.rerank("AdamW optimizer", candidates);
        assert_eq!(reranked.len(), 2);
    }

    #[test]
    fn remove_cleans_up_correctly() {
        let mut r = make_reranker();
        let before = r.len();
        r.remove(1);
        assert_eq!(r.len(), before - 1);
        // removed record should not panic in rerank
        let candidates = vec![(1, 1.0), (2, 1.5)];
        let reranked = r.rerank("AdamW", candidates);
        assert_eq!(reranked.len(), 2);
    }

    #[test]
    fn tokenise_lowercases_and_filters_short() {
        let toks = tokenise("Hello, World! a I");
        assert!(toks.contains(&"hello".to_string()));
        assert!(toks.contains(&"world".to_string()));
        // single-char words filtered
        assert!(!toks.contains(&"a".to_string()));
        assert!(!toks.contains(&"i".to_string()));
    }

    #[test]
    fn normalise_all_equal_returns_zeros() {
        let v = normalise(&[3.0, 3.0, 3.0]);
        assert!(v.iter().all(|x| *x == 0.0));
    }
}
