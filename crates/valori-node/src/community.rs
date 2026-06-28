// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase I6 — Community Layer
//!
//! Runs Label Propagation on the existing `KernelState` graph, computes a
//! centroid vector per community (average of member record vectors), and
//! produces a BLAKE3 receipt that proves the assignment at a point in time.
//!
//! This module is **std-only** and lives entirely in `valori-node`.
//! Nothing in `valori-kernel` is modified — the kernel stays `no_std`.
//!
//! ## Why Label Propagation?
//!
//! It is the lightest community-detection algorithm that works directly on an
//! adjacency list with no external dependencies:
//! - O(n + e) per iteration, typically converges in < 10 passes.
//! - Deterministic with tie-breaking (min label wins) so the BLAKE3 receipt is
//!   reproducible given the same graph state.
//! - Zero allocation of a dense adjacency matrix — we walk the linked-list edges
//!   that already exist in `EdgePool`.

use std::collections::{HashMap, BTreeMap};

use serde::{Deserialize, Serialize};
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::id::NodeId;

/// Default maximum Label Propagation iterations before stopping.
pub const DEFAULT_MAX_ITER: u32 = 20;

/// A community assignment snapshot produced by `/v1/community/detect`.
/// Stored in the engine between calls so `/v1/community/search` can use it
/// without re-running detection on every query.
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
    /// Number of detected communities.
    pub community_count: usize,
    /// Total nodes that were assigned.
    pub node_count: usize,
}

/// Per-community summary returned by `/v1/community/detect`.
#[derive(Serialize)]
pub struct CommunitySummary {
    pub community_id: u32,
    pub member_count: usize,
    pub centroid_record_id: Option<u32>,
}

#[derive(Serialize)]
pub struct DetectResponse {
    pub community_count: usize,
    pub node_count: usize,
    pub communities: Vec<CommunitySummary>,
    /// BLAKE3 hex receipt over sorted assignments — tamper-evident proof of
    /// the community structure at this point in time.
    pub receipt: String,
}

#[derive(Deserialize)]
pub struct DetectRequest {
    /// Limit detection to nodes in a specific collection (namespace).
    /// If omitted, all nodes across all namespaces are included.
    #[serde(default)]
    pub namespace: Option<String>,
    /// Maximum Label Propagation iterations. Default: 20.
    #[serde(default)]
    pub max_iter: Option<u32>,
}

// ── Search ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SearchRequest {
    /// Query vector (f32). Must match the dimension of stored vectors.
    pub vector: Vec<f32>,
    /// How many top communities to return. Default: 5.
    #[serde(default = "default_k")]
    pub k: usize,
    /// BFS depth into each community's subgraph. Default: 1.
    #[serde(default = "default_depth")]
    pub depth: u32,
    /// Restrict to a specific collection.
    #[serde(default)]
    pub namespace: Option<String>,
    /// If true, also return the top-k individual records within each matched
    /// community (second-level drill-in). Default: false.
    #[serde(default)]
    pub drill_in: bool,
}

fn default_k() -> usize { 5 }
fn default_depth() -> u32 { 1 }

#[derive(Serialize)]
pub struct CommunityHit {
    pub community_id: u32,
    /// Cosine-like similarity score (higher = more relevant).
    pub score: f32,
    pub member_count: usize,
    /// Seed node_ids that belong to this community (up to 20).
    pub sample_node_ids: Vec<u32>,
}

#[derive(Serialize)]
pub struct SearchResponse {
    pub communities: Vec<CommunityHit>,
    pub total_communities_searched: usize,
}

// ── Entity extraction ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ExtractEntitiesRequest {
    pub text: String,
    #[serde(default)]
    pub namespace: Option<String>,
    /// Entity types to focus on. Defaults to ["PERSON","ORGANIZATION","CONCEPT","LOCATION","EVENT"].
    #[serde(default)]
    pub entity_types: Vec<String>,
    /// Model to use for extraction (uses VALORI_EMBED_MODEL / provider default if omitted).
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
/// deterministic given a fixed graph structure and iteration order.
pub fn label_propagation(
    state: &KernelState,
    namespace_filter: Option<u16>,
    max_iter: u32,
) -> HashMap<u32, u32> {
    // Collect relevant node IDs.
    let node_ids: Vec<u32> = state.iter_nodes()
        .filter(|n| namespace_filter.map_or(true, |ns| n.namespace_id == ns))
        .map(|n| n.id.0)
        .collect();

    if node_ids.is_empty() {
        return HashMap::new();
    }

    // Initialise: each node is its own community.
    let mut labels: HashMap<u32, u32> = node_ids.iter().map(|&id| (id, id)).collect();

    for _ in 0..max_iter {
        let mut changed = false;

        // Iterate in a fixed order (sorted) for determinism.
        let mut sorted_ids = node_ids.clone();
        sorted_ids.sort_unstable();

        for &nid in &sorted_ids {
            // Collect neighbour labels from both outgoing and incoming edges.
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
                // Isolated node: stays in its own community.
                continue;
            }

            // Pick the most frequent label; break ties by choosing the minimum label.
            let max_count = *freq.values().max().unwrap();
            let best = freq.into_iter()
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
    // Group node_ids by community.
    let mut members: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&nid, &cid) in &assignments {
        members.entry(cid).or_default().push(nid);
    }
    for v in members.values_mut() {
        v.sort_unstable();
    }

    // Compute centroids: average of the FxpVectors of all records in the community.
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

    // BLAKE3 receipt over sorted (node_id, community_id) pairs.
    let mut hasher = blake3::Hasher::new();
    let sorted: BTreeMap<u32, u32> = assignments.iter().map(|(&k, &v)| (k, v)).collect();
    for (nid, cid) in &sorted {
        hasher.update(&nid.to_le_bytes());
        hasher.update(&cid.to_le_bytes());
    }
    let receipt = hasher.finalize().as_bytes().iter()
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

/// Score a query vector against all community centroids using dot-product
/// similarity (faster than L2 for this comparison since centroids are few).
/// Returns `(community_id, score)` pairs sorted best-first, truncated to `k`.
pub fn rank_communities(
    store: &CommunityStore,
    query: &[f32],
    k: usize,
) -> Vec<(u32, f32)> {
    let mut scores: Vec<(u32, f32)> = store.centroids.iter()
        .filter(|(_, c)| c.len() == query.len())
        .map(|(&cid, centroid)| {
            // Cosine similarity: dot(q, c) / (|q| * |c|)
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

// ── LLM entity extraction helper ─────────────────────────────────────────────

/// Call the configured embed provider's chat/completion endpoint to extract
/// entities and relationships from `text`. Reuses existing credentials so no
/// new env vars are needed.
///
/// Returns `None` if no embed provider is configured.
pub async fn extract_entities_via_llm(
    text: &str,
    entity_types: &[String],
    embed_cfg: &crate::embedder::EmbedConfig,
    model_override: Option<&str>,
    http: &reqwest::Client,
) -> Result<LlmExtractionOutput, String> {
    let default_types = ["PERSON", "ORGANIZATION", "CONCEPT", "LOCATION", "EVENT"];
    let types_str = if entity_types.is_empty() {
        default_types.join(", ")
    } else {
        entity_types.join(", ")
    };

    let prompt = format!(
        r#"You are an entity extraction system. Extract entities and relationships from the text below.

Entity types to extract: {types}

Return a JSON object with exactly this structure (no extra keys, no markdown):
{{
  "entities": [
    {{"name": "EntityName", "type": "ENTITY_TYPE", "description": "Brief factual description"}}
  ],
  "relationships": [
    {{"source": "EntityName1", "target": "EntityName2", "description": "relationship description", "strength": 0.8}}
  ]
}}

TEXT:
{text}

JSON:"#,
        types = types_str,
        text = text,
    );

    match embed_cfg.provider.as_str() {
        "openai" | "custom" => {
            let base = embed_cfg.url.trim_end_matches('/');
            let url = format!("{base}/chat/completions");
            let model = model_override
                .unwrap_or_else(|| {
                    if embed_cfg.model.contains("embed") { "gpt-4o-mini" } else { &embed_cfg.model }
                });
            let body = serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": prompt}],
                "temperature": 0,
                "response_format": {"type": "json_object"}
            });
            let mut req = http.post(&url).json(&body);
            if let Some(ref key) = embed_cfg.api_key {
                req = req.bearer_auth(key);
            }
            let resp = req.send().await.map_err(|e| e.to_string())?;
            let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            let content = json["choices"][0]["message"]["content"]
                .as_str()
                .ok_or_else(|| "no content in LLM response".to_string())?;
            serde_json::from_str(content).map_err(|e| format!("JSON parse error: {e}"))
        }
        "ollama" => {
            let base = embed_cfg.url.trim_end_matches('/');
            let url = format!("{base}/api/generate");
            let model = model_override.unwrap_or(&embed_cfg.model);
            let body = serde_json::json!({
                "model": model,
                "prompt": prompt,
                "format": "json",
                "stream": false
            });
            let resp = http.post(&url).json(&body).send().await.map_err(|e| e.to_string())?;
            let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            let content = json["response"]
                .as_str()
                .ok_or_else(|| "no response field from Ollama".to_string())?;
            serde_json::from_str(content).map_err(|e| format!("JSON parse error: {e}"))
        }
        other => Err(format!("entity extraction not supported for provider '{other}'")),
    }
}
