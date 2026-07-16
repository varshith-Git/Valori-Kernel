// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! BM25 Okapi hybrid reranker.
//!
//! The kernel returns top-K record IDs by vector similarity (L2 distance).
//! This module re-scores those candidates with BM25 and returns a list
//! re-ordered by a hybrid score so exact and rare-term matches rise above
//! semantically-similar-but-wrong results.
//!
//! # Pipeline
//!
//! ```text
//! /search handler
//!   └─ kernel.search_l2_ns(query_vec, k × POOL_FACTOR)   ← wider candidate pool
//!       └─ ValoriReranker::rerank(query_text, candidates) ← BM25 blend
//!           └─ return top k                               ← to client
//! ```
//!
//! # BM25 Okapi formula
//!
//! ```text
//! score(t,d) = IDF(t) × tf(t,d)×(k1+1) / (tf(t,d) + k1×(1−b + b×|d|/avgdl))
//! IDF(t)     = ln((N − df(t) + 0.5) / (df(t) + 0.5) + 1)
//!
//!   k1 = 1.5   — term-frequency saturation
//!   b  = 0.75  — document-length normalisation
//! ```
//!
//! # Scalability
//!
//! Document-frequency (`df`) is maintained in an **inverted index**
//! (`doc_freq: HashMap<String, usize>`), updated incrementally on every
//! [`ValoriReranker::insert`] and [`ValoriReranker::remove`]. IDF lookup is
//! therefore O(1) per query term regardless of corpus size, rather than the
//! naïve O(|corpus|) full scan.
//!
//! Insert / remove are O(|tokens in document|). Rerank is
//! O(|candidates| × |query_terms|) — negligible for k ≤ 1 000.

use std::collections::HashMap;

/// Standard BM25 Okapi parameters.
const K1: f32 = 1.5;
const B: f32 = 0.75;

/// How many vector candidates to fetch before BM25 reranking.
///
/// The search handler fetches `k × POOL_FACTOR` candidates from the kernel,
/// then BM25 re-ranks and returns the top `k`. A larger pool gives BM25 more
/// signal; set high so BM25 sees the full collection on small indices.
pub const POOL_FACTOR: usize = 20;

/// BM25 corpus and hybrid reranker.
///
/// One instance lives inside `Engine` / `DataPlaneState` alongside the kernel.
/// All operations are synchronous and allocation-minimal — no locks needed
/// because the caller serialises writes through the engine write lock.
#[derive(Default)]
pub struct ValoriReranker {
    /// record_id → tokenised document (lowercase alphanumeric words ≥ 2 chars)
    corpus: HashMap<u64, Vec<String>>,
    /// term → number of documents containing that term (inverted index for O(1) IDF)
    doc_freq: HashMap<String, usize>,
    /// Running total tokens across all documents (for avgdl).
    total_tokens: usize,
}

impl ValoriReranker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register text for a newly inserted record.
    ///
    /// Call immediately after the kernel assigns a record ID. Duplicate calls
    /// for the same `record_id` overwrite the previous entry and update `doc_freq`
    /// correctly.
    pub fn insert(&mut self, record_id: u64, text: &str) {
        // If a previous entry exists, undo its contribution first.
        self.remove(record_id);

        let tokens = tokenise(text);
        self.total_tokens += tokens.len();

        // Update inverted index: increment df for each distinct term.
        let unique_terms: std::collections::HashSet<&str> =
            tokens.iter().map(String::as_str).collect();
        for term in &unique_terms {
            *self.doc_freq.entry(term.to_string()).or_insert(0) += 1;
        }

        self.corpus.insert(record_id, tokens);
    }

    /// Remove a record's text (soft-delete or drop-collection path).
    pub fn remove(&mut self, record_id: u64) {
        if let Some(tokens) = self.corpus.remove(&record_id) {
            self.total_tokens = self.total_tokens.saturating_sub(tokens.len());

            // Decrement df for each distinct term in the removed document.
            let unique_terms: std::collections::HashSet<&str> =
                tokens.iter().map(String::as_str).collect();
            for term in unique_terms {
                if let Some(count) = self.doc_freq.get_mut(term) {
                    if *count <= 1 {
                        self.doc_freq.remove(term);
                    } else {
                        *count -= 1;
                    }
                }
            }
        }
    }

    /// Remove all records belonging to a collection (drop-collection path).
    pub fn remove_batch(&mut self, record_ids: &[u64]) {
        for &id in record_ids {
            self.remove(id);
        }
    }

    /// Re-rank `candidates` (record_id, vector_distance) by a hybrid BM25 + vector score.
    ///
    /// Returns the same candidates sorted descending by:
    /// ```text
    /// hybrid = 0.5 × norm_vector_score + 0.5 × norm_bm25_score
    /// ```
    /// where both components are min-max normalised to `[0, 1]`.
    /// Vector distances are L2 (lower = better), so they are flipped before blending.
    ///
    /// Records with no stored text score 0 on the BM25 component and retain
    /// their relative vector rank.
    pub fn rerank(&self, query: &str, candidates: Vec<(u64, f32)>) -> Vec<(u64, f32)> {
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

        // IDF per query term — O(1) via inverted index.
        let idf: HashMap<&str, f32> = q_terms
            .iter()
            .map(|t| {
                let df = *self.doc_freq.get(t.as_str()).unwrap_or(&0) as f32;
                let score = ((n_docs - df + 0.5) / (df + 0.5) + 1.0).ln().max(0.0);
                (t.as_str(), score)
            })
            .collect();

        // BM25 score per candidate.
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

        let v_scores: Vec<f32> = candidates.iter().map(|(_, s)| *s).collect();
        // Vector scores are L2 distances — lower = better. Flip after normalising.
        let v_norm: Vec<f32> = normalise(&v_scores).iter().map(|x| 1.0 - x).collect();
        let b_norm = normalise(&bm25_raw);

        const ALPHA: f32 = 0.5;
        let mut scored: Vec<(u64, f32)> = candidates
            .iter()
            .enumerate()
            .map(|(i, (rid, _))| (*rid, ALPHA * v_norm[i] + (1.0 - ALPHA) * b_norm[i]))
            .collect();

        // Descending by hybrid score (higher = better).
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }

    /// Number of records with stored text.
    pub fn len(&self) -> usize {
        self.corpus.len()
    }

    pub fn is_empty(&self) -> bool {
        self.corpus.is_empty()
    }

    // ── snapshot persistence ──────────────────────────────────────────────────

    /// Borrow the raw corpus for snapshot serialization.
    pub fn snapshot_corpus(&self) -> (&HashMap<u64, Vec<String>>, usize) {
        (&self.corpus, self.total_tokens)
    }

    /// Replace the entire corpus from a deserialized snapshot.
    ///
    /// Rebuilds the inverted index from scratch to maintain consistency.
    pub fn restore_corpus(&mut self, corpus: HashMap<u64, Vec<String>>, total_tokens: usize) {
        self.corpus.clear();
        self.doc_freq.clear();
        self.total_tokens = 0;

        // Re-insert every document so the inverted index stays consistent.
        for (id, tokens) in corpus {
            let unique_terms: std::collections::HashSet<&str> =
                tokens.iter().map(String::as_str).collect();
            for term in &unique_terms {
                *self.doc_freq.entry(term.to_string()).or_insert(0) += 1;
            }
            self.total_tokens += tokens.len();
            self.corpus.insert(id, tokens);
        }
        // Override token count with the snapshotted value only if the rebuild matches;
        // if not (e.g. snapshot was from a version with different tokenisation) trust
        // the rebuild.
        let _ = total_tokens; // rebuild is authoritative
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Tokenise text into lowercase alphanumeric words with ≥ 2 characters.
///
/// Consistent with the Python-side tokeniser in `tree_rag.py`.
pub fn tokenise(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 2)
        .map(|w| w.to_lowercase())
        .collect()
}

/// Min-max normalise a slice to `[0, 1]`.
///
/// Returns all-zeros when every value is equal (no preference between candidates).
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
        r.insert(
            1,
            "The optimizer used is AdamW with weight decay during continued pretraining",
        );
        r.insert(
            2,
            "Reinforcement learning uses Adam optimizer in the single epoch regime",
        );
        r.insert(
            3,
            "Context Parallelism replaced Tensor Parallelism for long context scaling",
        );
        r.insert(
            4,
            "Agent behavior includes penalties for leaving to-do lists unfinished",
        );
        r.insert(
            5,
            "Public benchmarks like SWE-bench measure software engineering performance",
        );
        r
    }

    #[test]
    fn exact_term_rises_above_semantic_neighbour() {
        let r = make_reranker();
        // Vector search hypothetically returns [2, 1, 3] but query mentions AdamW
        // which only appears in record 1.
        let candidates = vec![(2, 1.0), (1, 1.5), (3, 2.0)];
        let reranked = r.rerank("AdamW optimizer pretraining", candidates);
        assert_eq!(reranked[0].0, 1, "AdamW should rank first after BM25");
    }

    #[test]
    fn exact_phrase_wins() {
        let r = make_reranker();
        let candidates = vec![(1, 1.0), (2, 1.1), (3, 1.2), (4, 1.3), (5, 1.4)];
        let reranked = r.rerank("Context Parallelism long context", candidates);
        assert_eq!(reranked[0].0, 3);
    }

    #[test]
    fn empty_candidates_returns_empty() {
        let r = make_reranker();
        assert!(r.rerank("anything", vec![]).is_empty());
    }

    #[test]
    fn unknown_record_does_not_panic() {
        let r = make_reranker();
        let candidates = vec![(99, 1.0), (1, 1.5)];
        let reranked = r.rerank("AdamW optimizer", candidates);
        assert_eq!(reranked.len(), 2);
    }

    #[test]
    fn remove_keeps_doc_freq_consistent() {
        let mut r = make_reranker();
        // record 1 contains "adamw". After remove it must not appear in doc_freq.
        r.remove(1);
        assert_eq!(r.len(), 4);
        assert!(
            !r.doc_freq.contains_key("adamw"),
            "doc_freq must be cleaned up"
        );

        // Reranking must not panic with the removed record.
        let candidates = vec![(1, 1.0), (2, 1.5)];
        let reranked = r.rerank("AdamW", candidates);
        assert_eq!(reranked.len(), 2);
    }

    #[test]
    fn insert_overwrites_and_doc_freq_stays_consistent() {
        let mut r = ValoriReranker::new();
        r.insert(1, "hello world");
        assert_eq!(*r.doc_freq.get("hello").unwrap(), 1);
        // Overwrite with different text.
        r.insert(1, "goodbye world");
        assert!(
            !r.doc_freq.contains_key("hello"),
            "old term must be removed"
        );
        assert_eq!(*r.doc_freq.get("goodbye").unwrap(), 1);
        assert_eq!(*r.doc_freq.get("world").unwrap(), 1);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn restore_corpus_rebuilds_doc_freq() {
        let r = make_reranker();
        let (corpus, total) = r.snapshot_corpus();
        let corpus_clone: HashMap<u64, Vec<String>> = corpus.clone();

        let mut r2 = ValoriReranker::new();
        r2.restore_corpus(corpus_clone, total);

        // After restore, doc_freq must be populated.
        assert!(
            r2.doc_freq.contains_key("adamw"),
            "doc_freq rebuilt from corpus"
        );
        assert_eq!(r2.len(), r.len());
    }

    #[test]
    fn tokenise_lowercases_and_filters_short() {
        let toks = tokenise("Hello, World! a I");
        assert!(toks.contains(&"hello".to_string()));
        assert!(toks.contains(&"world".to_string()));
        assert!(!toks.contains(&"a".to_string()));
        assert!(!toks.contains(&"i".to_string()));
    }

    #[test]
    fn normalise_all_equal_returns_zeros() {
        let v = normalise(&[3.0, 3.0, 3.0]);
        assert!(v.iter().all(|x| *x == 0.0));
    }
}
