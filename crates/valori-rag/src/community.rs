// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase I6 — Community Layer
//!
//! Runs Label Propagation on the existing `KernelState` graph, computes a
//! centroid vector per community (average of member record vectors), and
//! produces a BLAKE3 receipt that proves the assignment at a point in time.
//!
//! ## Why Label Propagation?
//!
//! - O(n + e) per iteration, typically converges in < 10 passes.
//! - Deterministic with tie-breaking (min label wins).
//! - Zero allocation of a dense adjacency matrix.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::id::NodeId;

/// Default maximum Label Propagation iterations before stopping.
pub const DEFAULT_MAX_ITER: u32 = 20;

// ── Store ─────────────────────────────────────────────────────────────────────

/// A community assignment snapshot produced by `/v1/community/detect`.
#[derive(Clone, Debug, Serialize)]
pub struct CommunityStore {
    /// node_id → community_id
    pub assignments: HashMap<u32, u32>,
    /// community_id → centroid (f32, same dimensionality as record vectors)
    pub centroids: HashMap<u32, Vec<f32>>,
    /// community_id → sorted list of member node_ids
    pub members: HashMap<u32, Vec<u32>>,
    /// BLAKE3 hex of the sorted (node_id, community_id) assignment map.
    pub receipt: String,
    pub community_count: usize,
    pub node_count: usize,
}

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
pub struct CommunitySummary {
    pub community_id: u32,
    pub member_count: usize,
    pub centroid_record_id: Option<u32>,
}

#[derive(Serialize, Deserialize)]
pub struct DetectResponse {
    pub community_count: usize,
    pub node_count: usize,
    pub communities: Vec<CommunitySummary>,
    /// BLAKE3 hex receipt over sorted assignments.
    pub receipt: String,
}

#[derive(Deserialize)]
pub struct DetectRequest {
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub max_iter: Option<u32>,
}

#[derive(Deserialize)]
pub struct SearchRequest {
    pub vector: Vec<f32>,
    #[serde(default = "default_k")]
    pub k: usize,
    #[serde(default = "default_depth")]
    pub depth: u32,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub drill_in: bool,
}

fn default_k() -> usize {
    5
}
fn default_depth() -> u32 {
    1
}

#[derive(Serialize, Deserialize)]
pub struct CommunityHit {
    pub community_id: u32,
    pub score: f32,
    pub member_count: usize,
    pub sample_node_ids: Vec<u32>,
}

#[derive(Serialize, Deserialize)]
pub struct SearchResponse {
    pub communities: Vec<CommunityHit>,
    pub total_communities_searched: usize,
}

// ── Entity extraction types ───────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ExtractEntitiesRequest {
    pub text: String,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub entity_types: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ExtractedEntity {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub description: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ExtractedRelationship {
    pub source: String,
    pub target: String,
    pub description: String,
    #[serde(default)]
    pub strength: f32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LlmExtractionOutput {
    pub entities: Vec<ExtractedEntity>,
    pub relationships: Vec<ExtractedRelationship>,
}

#[derive(Serialize)]
pub struct InsertedEntity {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub description: String,
    pub node_id: u32,
    pub record_id: Option<u32>,
}

#[derive(Serialize)]
pub struct InsertedRelationship {
    pub source_name: String,
    pub target_name: String,
    pub description: String,
    pub edge_id: u32,
}

#[derive(Serialize)]
pub struct ExtractEntitiesResponse {
    pub entities: Vec<InsertedEntity>,
    pub relationships: Vec<InsertedRelationship>,
    pub entity_count: usize,
    pub relationship_count: usize,
    pub skipped_relationships: usize,
}

// ── Label Propagation ─────────────────────────────────────────────────────────

/// Run Label Propagation on the graph in `state`, optionally filtered to
/// nodes in `namespace_id`. Returns `node_id → community_id` assignments.
///
/// Tie-breaking rule: the **minimum label** wins, making the output
/// deterministic given a fixed graph structure.
pub fn label_propagation(
    state: &KernelState,
    namespace_filter: Option<u16>,
    max_iter: u32,
) -> HashMap<u32, u32> {
    let node_ids: Vec<u32> = state
        .iter_nodes()
        .filter(|n| namespace_filter.map_or(true, |ns| n.namespace_id == ns))
        .map(|n| n.id.0)
        .collect();

    if node_ids.is_empty() {
        return HashMap::new();
    }

    let mut labels: HashMap<u32, u32> = node_ids.iter().map(|&id| (id, id)).collect();

    for _ in 0..max_iter {
        let mut changed = false;
        let mut sorted_ids = node_ids.clone();
        sorted_ids.sort_unstable();

        for &nid in &sorted_ids {
            let mut freq: HashMap<u32, u32> = HashMap::new();

            if let Some(out_iter) = state.outgoing_edges(NodeId(nid)) {
                for edge in out_iter {
                    let nbr = edge.to.0;
                    if let Some(&lbl) = labels.get(&nbr) {
                        *freq.entry(lbl).or_insert(0) += 1;
                    }
                }
            }
            if let Some(in_iter) = state.incoming_edges(NodeId(nid)) {
                for edge in in_iter {
                    let nbr = edge.from.0;
                    if let Some(&lbl) = labels.get(&nbr) {
                        *freq.entry(lbl).or_insert(0) += 1;
                    }
                }
            }

            if freq.is_empty() {
                continue;
            }

            let max_count = *freq.values().max().unwrap();
            let best = freq
                .into_iter()
                .filter(|(_, c)| *c == max_count)
                .map(|(lbl, _)| lbl)
                .min()
                .unwrap();

            if labels[&nid] != best {
                labels.insert(nid, best);
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }

    labels
}

/// Build the full `CommunityStore` from an assignment map and kernel state.
///
/// Computes centroid vectors (average of member record FxpVectors) in f32
/// and produces a BLAKE3 receipt over the sorted assignments.
pub fn build_community_store(
    state: &KernelState,
    assignments: HashMap<u32, u32>,
) -> CommunityStore {
    let mut members: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&nid, &cid) in &assignments {
        members.entry(cid).or_default().push(nid);
    }
    for v in members.values_mut() {
        v.sort_unstable();
    }

    let dim = state.dim.unwrap_or(0);
    let mut centroids: HashMap<u32, Vec<f32>> = HashMap::new();

    if dim > 0 {
        use valori_kernel::fxp::qformat::SCALE;
        for (&cid, nids) in &members {
            let mut sum = vec![0f64; dim];
            let mut count = 0usize;
            for &nid in nids {
                if let Some(node) = state.get_node(NodeId(nid)) {
                    if let Some(rid) = node.record {
                        if let Some(rec) = state.get_record(rid) {
                            if rec.is_searchable() && rec.vector.data.len() == dim {
                                for (i, s) in rec.vector.data.iter().enumerate() {
                                    sum[i] += s.0 as f64 / SCALE as f64;
                                }
                                count += 1;
                            }
                        }
                    }
                }
            }
            if count > 0 {
                let centroid: Vec<f32> = sum.iter().map(|&s| (s / count as f64) as f32).collect();
                centroids.insert(cid, centroid);
            }
        }
    }

    let mut hasher = blake3::Hasher::new();
    let sorted: BTreeMap<u32, u32> = assignments.iter().map(|(&k, &v)| (k, v)).collect();
    for (nid, cid) in &sorted {
        hasher.update(&nid.to_le_bytes());
        hasher.update(&cid.to_le_bytes());
    }
    let receipt = hasher
        .finalize()
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();

    let community_count = members.len();
    let node_count = assignments.len();

    CommunityStore {
        assignments,
        centroids,
        members,
        receipt,
        community_count,
        node_count,
    }
}

/// Score a query vector against all community centroids using cosine similarity.
/// Returns `(community_id, score)` pairs sorted best-first, truncated to `k`.
pub fn rank_communities(store: &CommunityStore, query: &[f32], k: usize) -> Vec<(u32, f32)> {
    let mut scores: Vec<(u32, f32)> = store
        .centroids
        .iter()
        .filter(|(_, c)| c.len() == query.len())
        .map(|(&cid, centroid)| {
            let dot: f32 = query.iter().zip(centroid.iter()).map(|(a, b)| a * b).sum();
            let q_norm: f32 = query.iter().map(|a| a * a).sum::<f32>().sqrt().max(1e-9);
            let c_norm: f32 = centroid.iter().map(|a| a * a).sum::<f32>().sqrt().max(1e-9);
            (cid, dot / (q_norm * c_norm))
        })
        .collect();

    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scores.truncate(k);
    scores
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_propagation_empty_graph() {
        let state = KernelState::new();
        let result = label_propagation(&state, None, DEFAULT_MAX_ITER);
        assert!(result.is_empty());
    }

    #[test]
    fn rank_communities_empty_store() {
        let store = CommunityStore {
            assignments: HashMap::new(),
            centroids: HashMap::new(),
            members: HashMap::new(),
            receipt: String::new(),
            community_count: 0,
            node_count: 0,
        };
        let scores = rank_communities(&store, &[0.1, 0.2], 5);
        assert!(scores.is_empty());
    }

    #[test]
    fn build_community_store_produces_receipt() {
        let state = KernelState::new();
        let mut assignments = HashMap::new();
        assignments.insert(1u32, 1u32);
        assignments.insert(2u32, 1u32);
        let store = build_community_store(&state, assignments);
        assert_eq!(store.community_count, 1);
        assert_eq!(store.node_count, 2);
        assert_eq!(store.receipt.len(), 64); // BLAKE3 hex = 64 chars
    }
}
