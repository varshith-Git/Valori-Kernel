// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Retrieval receipts — the feature no other agent-memory store has.
//!
//! Every `memory_recall` returns a [`Receipt`]: a tamper-evident binding of
//! *which* memories were returned, *against which committed state*, *at what
//! time*. The `receipt_digest` is `BLAKE3(canonical_json(receipt_body))`, so a
//! caller (or an auditor, months later) can recompute it offline and prove the
//! recall set was not altered after the fact.
//!
//! This rides directly on the kernel's existing guarantees: `state_hash` is the
//! BLAKE3 Merkle root over every applied event, and `event_log_hash` is the
//! BLAKE3 of the on-disk event log. Both come from the node's `/v1/proof/*`
//! endpoints — the receipt just packages them with the result set.

use serde::Serialize;
use serde_json::Value;

/// The body that gets hashed. Field order here is the canonical order: the
/// digest is computed over `serde_json::to_vec` of this struct, and serde
/// preserves declaration order, so the digest is reproducible by any client
/// that serializes the same fields in the same order.
#[derive(Debug, Clone, Serialize)]
pub struct ReceiptBody {
    /// BLAKE3 Merkle root of all applied events at recall time (64 hex chars).
    pub state_hash: String,
    /// BLAKE3 of the on-disk event log, if the node has one enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_log_hash: Option<String>,
    /// Number of committed events backing this state, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub committed_height: Option<u64>,
    /// The query vector dimensionality (lets a verifier sanity-check the query).
    pub query_dim: usize,
    /// k requested.
    pub k: usize,
    /// The ordered result identities: `[memory_id, record_id, score_bits]`.
    /// `score_bits` is the raw IEEE-754 bit pattern of the score as a string,
    /// so the digest is exact and platform-independent.
    ///
    /// # Known limitation — state hash is captured after the search
    ///
    /// `state_hash` is fetched in a separate HTTP call after `memory_search`
    /// completes. If a concurrent write commits between the two calls, `state_hash`
    /// reflects a strictly newer state `S'` than the state `S` at which the search
    /// ran. Because the kernel is append-only, the results are still valid members
    /// of `S'` — no result was removed — but you cannot replay `S'` and prove that
    /// these exact k-nearest results would be returned. A strict proof would require
    /// an atomic "search + proof" endpoint on the node side (not yet implemented).
    pub results: Vec<ResultFingerprint>,
    /// For GraphRAG recalls: the connected subgraph that was returned alongside
    /// the hits, so the receipt binds the *entire* retrieved context — not just
    /// the vector neighbours. Omitted (and absent from the digest) for plain
    /// vector recalls, keeping their receipts byte-identical to before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subgraph: Option<SubgraphFingerprint>,
}

/// Order-independent fingerprint of a returned subgraph: node and edge ids,
/// each sorted ascending so the digest does not depend on traversal order.
#[derive(Debug, Clone, Serialize)]
pub struct SubgraphFingerprint {
    pub node_ids: Vec<u64>,
    pub edge_ids: Vec<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResultFingerprint {
    pub memory_id: String,
    pub record_id: u64,
    /// Score as raw f64 bits (decimal string) — exact, no float formatting drift.
    pub score_bits: String,
}

/// A full receipt: the hashed body plus the digest over it.
#[derive(Debug, Clone, Serialize)]
pub struct Receipt {
    #[serde(flatten)]
    pub body: ReceiptBody,
    /// ISO-8601-ish wall clock when the recall was served. NOT part of the
    /// digest — wall clock is advisory; the cryptographic anchor is the state.
    pub recalled_at_unix: u64,
    /// BLAKE3 hex digest over the canonical JSON of `body`.
    pub receipt_digest: String,
}

impl Receipt {
    /// Build a receipt from a recall result set and the node's proof fields.
    pub fn build(body: ReceiptBody, recalled_at_unix: u64) -> Self {
        let digest = compute_digest(&body);
        Self {
            body,
            recalled_at_unix,
            receipt_digest: digest,
        }
    }
}

/// Compute `BLAKE3(canonical_json(body))` as lowercase hex.
///
/// Canonical JSON here = `serde_json::to_vec` of [`ReceiptBody`] with its
/// fields in declaration order. Any client that reconstructs the same body and
/// serializes it the same way gets the same digest — that is the verification
/// contract.
pub fn compute_digest(body: &ReceiptBody) -> String {
    let bytes = serde_json::to_vec(body).expect("ReceiptBody is always serializable");
    blake3::hash(&bytes).to_hex().to_string()
}

/// Extract the ordered result fingerprints from a node `memory_search_vector`
/// JSON response (`{ "results": [ { "memory_id", "record_id", "score", ... } ] }`).
pub fn fingerprints_from_results(results: &Value) -> Vec<ResultFingerprint> {
    results
        .get("results")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .map(|hit| {
                    let memory_id = hit
                        .get("memory_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let record_id = hit.get("record_id").and_then(|v| v.as_u64()).unwrap_or(0);
                    let score = hit.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    ResultFingerprint {
                        memory_id,
                        record_id,
                        score_bits: score.to_bits().to_string(),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Build a [`SubgraphFingerprint`] from a GraphRAG `subgraph` JSON object
/// (`{ "nodes": [ { "id" } ], "edges": [ { "id" } ] }`). Ids are sorted so the
/// fingerprint is independent of the traversal/emit order.
pub fn subgraph_fingerprint(subgraph: &Value) -> SubgraphFingerprint {
    let ids = |key: &str| -> Vec<u64> {
        let mut v: Vec<u64> = subgraph
            .get(key)
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.get("id").and_then(|i| i.as_u64()))
                    .collect()
            })
            .unwrap_or_default();
        v.sort_unstable();
        v
    };
    SubgraphFingerprint {
        node_ids: ids("nodes"),
        edge_ids: ids("edges"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_body() -> ReceiptBody {
        ReceiptBody {
            state_hash: "ab".repeat(32),
            event_log_hash: Some("cd".repeat(32)),
            committed_height: Some(3),
            query_dim: 4,
            k: 2,
            results: vec![
                ResultFingerprint {
                    memory_id: "m1".into(),
                    record_id: 1,
                    score_bits: "0".into(),
                },
                ResultFingerprint {
                    memory_id: "m2".into(),
                    record_id: 2,
                    score_bits: "1".into(),
                },
            ],
            subgraph: None,
        }
    }

    #[test]
    fn digest_is_deterministic() {
        let a = compute_digest(&sample_body());
        let b = compute_digest(&sample_body());
        assert_eq!(a, b);
        assert_eq!(a.len(), 64); // BLAKE3 hex
    }

    #[test]
    fn digest_changes_when_results_change() {
        let base = compute_digest(&sample_body());
        let mut tampered = sample_body();
        tampered.results[0].record_id = 999; // swap a returned memory
        let after = compute_digest(&tampered);
        assert_ne!(
            base, after,
            "tampering with the result set must change the digest"
        );
    }

    #[test]
    fn digest_changes_when_state_hash_changes() {
        let base = compute_digest(&sample_body());
        let mut other = sample_body();
        other.state_hash = "ff".repeat(32);
        assert_ne!(base, compute_digest(&other));
    }

    #[test]
    fn wall_clock_does_not_affect_digest() {
        // The digest is over `body` only; recalled_at is advisory.
        let r1 = Receipt::build(sample_body(), 1000);
        let r2 = Receipt::build(sample_body(), 2000);
        assert_eq!(r1.receipt_digest, r2.receipt_digest);
    }

    #[test]
    fn fingerprints_parse_from_node_json() {
        let resp = serde_json::json!({
            "results": [
                {"memory_id": "m1", "record_id": 5, "score": 0.5, "metadata": null},
                {"memory_id": "m2", "record_id": 6, "score": 0.25}
            ]
        });
        let fps = fingerprints_from_results(&resp);
        assert_eq!(fps.len(), 2);
        assert_eq!(fps[0].memory_id, "m1");
        assert_eq!(fps[0].record_id, 5);
        assert_eq!(fps[1].record_id, 6);
    }

    #[test]
    fn subgraph_fingerprint_sorts_ids_for_order_independence() {
        let a = serde_json::json!({
            "nodes": [{"id": 3}, {"id": 1}, {"id": 2}],
            "edges": [{"id": 20}, {"id": 10}]
        });
        let b = serde_json::json!({
            "nodes": [{"id": 1}, {"id": 2}, {"id": 3}],
            "edges": [{"id": 10}, {"id": 20}]
        });
        let fa = subgraph_fingerprint(&a);
        assert_eq!(fa.node_ids, vec![1, 2, 3]);
        assert_eq!(fa.edge_ids, vec![10, 20]);
        // Same ids in a different order produce an identical fingerprint.
        let fb = subgraph_fingerprint(&b);
        assert_eq!(fa.node_ids, fb.node_ids);
        assert_eq!(fa.edge_ids, fb.edge_ids);
    }

    #[test]
    fn subgraph_presence_changes_the_digest() {
        let plain = compute_digest(&sample_body());
        let mut with_graph = sample_body();
        with_graph.subgraph = Some(SubgraphFingerprint {
            node_ids: vec![1, 2],
            edge_ids: vec![9],
        });
        assert_ne!(
            plain,
            compute_digest(&with_graph),
            "binding a subgraph must change the receipt digest"
        );
    }

    #[test]
    fn plain_recall_digest_unchanged_by_new_optional_field() {
        // The new `subgraph: None` field is skipped in serialization, so a plain
        // recall receipt hashes exactly as it did before the field existed.
        let body = sample_body(); // subgraph: None
        let bytes = serde_json::to_vec(&body).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("subgraph"));
    }
}
