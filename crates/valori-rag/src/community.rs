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

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize, Deserialize)]
pub struct CommunitySummary {
    pub community_id: u32,
    pub member_count: usize,
    pub centroid_record_id: Option<u32>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema), schema(as = CommunityDetectResponse))]
#[derive(Serialize, Deserialize)]
pub struct DetectResponse {
    pub community_count: usize,
    pub node_count: usize,
    pub communities: Vec<CommunitySummary>,
    /// BLAKE3 hex receipt over sorted assignments.
    pub receipt: String,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema), schema(as = CommunityDetectRequest))]
#[derive(Deserialize)]
pub struct DetectRequest {
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub max_iter: Option<u32>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema), schema(as = CommunitySearchRequest))]
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

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize, Deserialize)]
pub struct CommunityHit {
    pub community_id: u32,
    pub score: f32,
    pub member_count: usize,
    pub sample_node_ids: Vec<u32>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema), schema(as = CommunitySearchResponse))]
#[derive(Serialize, Deserialize)]
pub struct SearchResponse {
    pub communities: Vec<CommunityHit>,
    pub total_communities_searched: usize,
}

// ── Entity extraction types ───────────────────────────────────────────────────

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Deserialize)]
pub struct ExtractEntitiesRequest {
    pub text: String,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub entity_types: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize, Deserialize, Debug)]
pub struct ExtractedEntity {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub description: String,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize, Deserialize, Debug)]
pub struct ExtractedRelationship {
    pub source: String,
    pub target: String,
    pub description: String,
    /// Optional semantic predicate. Older extractors only provide description.
    #[serde(default)]
    pub predicate: Option<String>,
    #[serde(default)]
    pub evidence: Option<AssertionEvidence>,
    #[serde(default)]
    pub strength: f32,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct AssertionEvidence {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub source_text_hash: Option<String>,
    #[serde(default)]
    pub chunk_id: Option<String>,
    #[serde(default)]
    pub passage_id: Option<String>,
    #[serde(default)]
    pub span_start: Option<u64>,
    #[serde(default)]
    pub span_end: Option<u64>,
}

// ── RG7 canonical entities ────────────────────────────────────────────────

/// A source mention is deliberately separate from the entity it resolves to.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityMention {
    pub mention_id: String,
    pub entity_id: String,
    pub source: Option<String>,
    pub document_id: Option<String>,
    pub chunk_id: Option<String>,
    pub passage_id: Option<String>,
    pub span_start: Option<u64>,
    pub span_end: Option<u64>,
    pub surface_form: String,
    pub source_text_hash: Option<String>,
    pub assertion_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CanonicalEntity {
    pub entity_id: String,
    pub canonical_name: String,
    pub entity_type: String,
    pub aliases: Vec<String>,
    pub mention_ids: Vec<String>,
}

fn normalized_entity_text(value: &str) -> String {
    value
        .chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Resolve a mention without fuzzy or model-based merging. Context identity is
/// part of the key, so two unrelated people with the same name remain distinct.
pub fn resolve_canonical_entity(
    canonical_name: &str,
    entity_type: &str,
    aliases: &[String],
    mention: &EntityMention,
) -> CanonicalEntity {
    let normalized_type = normalized_entity_text(entity_type);
    let mut names = vec![canonical_name.to_owned()];
    names.extend(aliases.iter().cloned());
    let surface = normalized_entity_text(&mention.surface_form);
    let matched = names.iter().any(|n| normalized_entity_text(n) == surface);
    let identity_context = if matched {
        format!(
            "{}|{}|{}|{}",
            mention.source.as_deref().unwrap_or(""),
            mention.document_id.as_deref().unwrap_or(""),
            mention.chunk_id.as_deref().unwrap_or(""),
            mention.source_text_hash.as_deref().unwrap_or("")
        )
    } else {
        format!(
            "surface:{}|{}",
            surface,
            mention.source_text_hash.as_deref().unwrap_or("")
        )
    };
    let key = format!(
        "{}\u{1f}{}\u{1f}{}",
        normalized_entity_text(canonical_name),
        normalized_type,
        identity_context
    );
    let entity_id = format!("ent_{}", blake3::hash(key.as_bytes()).to_hex());
    let mut normalized_aliases: Vec<String> = names
        .into_iter()
        .map(|n| normalized_entity_text(&n))
        .filter(|n| !n.is_empty())
        .collect();
    normalized_aliases.sort();
    normalized_aliases.dedup();
    CanonicalEntity {
        entity_id,
        canonical_name: canonical_name.to_owned(),
        entity_type: entity_type.to_owned(),
        aliases: normalized_aliases,
        mention_ids: vec![mention.mention_id.clone()],
    }
}

// ── RG8 structural claim verification ─────────────────────────────────────

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum VerificationOutcome {
    Supports,
    Contradicts,
    Neutral,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationReceipt {
    pub verification_id: String,
    pub outcome: VerificationOutcome,
    pub verifier_type: String,
    pub verifier_version: String,
    pub config_hash: String,
    pub input_assertion_ids: Vec<String>,
    pub evidence_refs: Vec<AssertionEvidence>,
    pub confidence: Option<String>,
    pub confidence_source: String,
    pub receipt_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StructuredClaim {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    #[serde(default)]
    pub negated: bool,
    #[serde(default)]
    pub time_scope: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifyClaimRequest {
    pub left: StructuredClaim,
    pub right: StructuredClaim,
    pub left_assertion_id: String,
    pub right_assertion_id: String,
    #[serde(default)]
    pub evidence_refs: Vec<AssertionEvidence>,
}

/// Verify only facts that can be decided from normalized structure. Similarity
/// and citation are intentionally absent from this function's inputs.
pub fn verify_structured_claims(
    left: (&str, &str, &str, bool, Option<&str>),
    right: (&str, &str, &str, bool, Option<&str>),
    left_id: &str,
    right_id: &str,
    evidence_refs: Vec<AssertionEvidence>,
) -> VerificationReceipt {
    let same_subject = normalized_entity_text(left.0) == normalized_entity_text(right.0);
    let same_predicate = normalized_entity_text(left.1) == normalized_entity_text(right.1);
    let same_scope = left.4 == right.4;
    let outcome = if !same_subject || !same_predicate || !same_scope {
        VerificationOutcome::Unknown
    } else if normalized_entity_text(left.2) == normalized_entity_text(right.2) && left.3 == right.3
    {
        VerificationOutcome::Supports
    } else if left.3 != right.3 && normalized_entity_text(left.2) == normalized_entity_text(right.2)
    {
        VerificationOutcome::Contradicts
    } else if left.2 != right.2 {
        VerificationOutcome::Contradicts
    } else {
        VerificationOutcome::Unknown
    };
    let inputs = vec![left_id.to_owned(), right_id.to_owned()];
    let raw = serde_json::json!({"outcome": outcome, "inputs": inputs, "evidence": evidence_refs});
    let hash = blake3::hash(raw.to_string().as_bytes())
        .to_hex()
        .to_string();
    VerificationReceipt {
        verification_id: format!("ver_{}", hash),
        outcome,
        verifier_type: "structural".into(),
        verifier_version: "rg8-v1".into(),
        config_hash: blake3::hash(b"rg8-structural-v1").to_hex().to_string(),
        input_assertion_ids: vec![left_id.into(), right_id.into()],
        evidence_refs: serde_json::from_value(raw["evidence"].clone()).unwrap_or_default(),
        confidence: Some("1.0".into()),
        confidence_source: "deterministic-structure".into(),
        receipt_hash: hash,
    }
}

/// Builds the canonical, content-addressed identity for an extracted assertion.
/// Deliberately excludes allocated graph IDs so standalone and Raft agree.
pub fn assertion_identity(
    source_text_hash: &str,
    subject: &str,
    predicate: &str,
    object: &str,
    evidence: &AssertionEvidence,
) -> String {
    let canonical = serde_json::json!({
        "source_text_hash": source_text_hash,
        "subject": subject,
        "predicate": predicate,
        "object": object,
        "evidence": evidence,
    });
    format!(
        "ast_{}",
        blake3::hash(canonical.to_string().as_bytes()).to_hex()
    )
}

#[cfg(test)]
mod assertion_tests {
    use super::*;

    #[test]
    fn assertion_ids_are_stable_and_conflicts_coexist() {
        let e = AssertionEvidence {
            chunk_id: Some("c1".into()),
            ..Default::default()
        };
        let a = assertion_identity("h", "s", "supports", "o", &e);
        assert_eq!(a, assertion_identity("h", "s", "supports", "o", &e));
        assert_ne!(a, assertion_identity("h", "s", "contradicts", "o", &e));
    }

    #[test]
    fn rg7_aliases_are_deterministic_and_context_scoped() {
        let m = EntityMention {
            mention_id: "m1".into(),
            surface_form: "  ACME, Inc. ".into(),
            source_text_hash: Some("h".into()),
            ..Default::default()
        };
        let a = resolve_canonical_entity("Acme Inc", "ORG", &["ACME, Inc.".into()], &m);
        let b = resolve_canonical_entity("Acme Inc", "ORG", &["ACME, Inc.".into()], &m);
        assert_eq!(a, b);
        let incompatible =
            resolve_canonical_entity("Acme Inc", "PERSON", &["ACME, Inc.".into()], &m);
        assert_ne!(a.entity_id, incompatible.entity_id);
    }

    #[test]
    fn rg8_structural_verifier_does_not_use_citation_or_similarity() {
        let support = verify_structured_claims(
            ("alice", "owns", "Acme", false, Some("2025")),
            ("Alice", "owns", "acme", false, Some("2025")),
            "a",
            "b",
            vec![],
        );
        assert_eq!(support.outcome, VerificationOutcome::Supports);
        let contradiction = verify_structured_claims(
            ("alice", "owns", "Acme", false, Some("2025")),
            ("alice", "owns", "Acme", true, Some("2025")),
            "a",
            "c",
            vec![],
        );
        assert_eq!(contradiction.outcome, VerificationOutcome::Contradicts);
        let different_scope = verify_structured_claims(
            ("alice", "owns", "Acme", false, Some("2024")),
            ("alice", "owns", "Other", false, Some("2025")),
            "a",
            "d",
            vec![],
        );
        assert_eq!(different_scope.outcome, VerificationOutcome::Unknown);
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LlmExtractionOutput {
    pub entities: Vec<ExtractedEntity>,
    pub relationships: Vec<ExtractedRelationship>,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct InsertedEntity {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub description: String,
    pub node_id: u32,
    pub record_id: Option<u32>,
    /// RG7 deterministic identity for this source-resolved entity.
    pub entity_id: String,
    pub canonical_name: String,
    pub aliases: Vec<String>,
    pub mention_id: String,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Serialize)]
pub struct InsertedRelationship {
    pub source_name: String,
    pub target_name: String,
    pub description: String,
    pub strength: f32,
    pub edge_id: u32,
    pub assertion_id: String,
    pub evidence: AssertionEvidence,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
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
