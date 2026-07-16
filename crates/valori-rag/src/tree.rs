// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Tree-RAG — PageIndex-style hierarchical retrieval, fused with Valori's proof.
//!
//! Two ideas ported into the product:
//!   1. Build a **tree index** of a document — a table-of-contents the machine
//!      can reason over (parent/child sections, line-addressable bodies).
//!   2. **Navigate the tree** to fetch the exact relevant section(s) — no
//!      chunking, no embeddings — and answer with a breadcrumb + line citation.
//!
//! Every retrieval emits a **BLAKE3-chained, replayable receipt**: `verify`
//! re-reads the logged ranges and recomputes the evidence hash, so any
//! tampering with stored content is provable.
//!
//! ## Determinism
//!
//! The navigator is purely deterministic term-frequency reasoning over the tree.
//! No LLM hook — reproducible bit-for-bit, matching Valori's whole ethos.
//!
//! ## Statelessness
//!
//! Build / query / verify are pure functions over the document text and the
//! serialized tree — no engine state, no kernel mutation. The HTTP handlers
//! drop unchanged into both the standalone and cluster routers.

use std::collections::BTreeMap;

use axum::Json;
use serde::{Deserialize, Serialize};

// ── BLAKE3 helpers ─────────────────────────────────────────────────────────────

/// Genesis hash for a receipt chain — 64 hex zeros.
pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Stable content hash of a piece of text.
pub fn hash_text(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

fn join_hash(parts: &[&str]) -> String {
    blake3::hash(parts.join("\u{1f}").as_bytes()).to_hex().to_string()
}

// ── Stopwords ─────────────────────────────────────────────────────────────────

const STOP: &[&str] = &[
    "the", "a", "an", "and", "or", "of", "to", "in", "on", "for", "is", "are",
    "do", "does", "i", "you", "my", "our", "how", "what", "when", "where",
    "which", "can", "may", "get", "much", "many", "per", "with", "at", "be",
    "if", "it", "this", "that", "as", "by", "from", "have", "has",
];

fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            cur.push(ch.to_ascii_lowercase());
        } else if !cur.is_empty() {
            push_token(&mut out, &mut cur);
        }
    }
    if !cur.is_empty() {
        push_token(&mut out, &mut cur);
    }
    out
}

fn push_token(out: &mut Vec<String>, cur: &mut String) {
    if cur.len() > 1 && !STOP.contains(&cur.as_str()) {
        out.push(std::mem::take(cur));
    } else {
        cur.clear();
    }
}

// ── Tree types ────────────────────────────────────────────────────────────────

/// One section of a document — a node in the table-of-contents tree.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeNode {
    pub node_id: String,
    pub title: String,
    pub level: usize,
    /// 1-indexed line where this heading appears.
    pub start_line: usize,
    /// Last line owned by this section (excluding children).
    pub end_line: usize,
    /// Verbatim section body, excluding sub-sections.
    pub own_text: String,
    /// First sentence of the body — a no-LLM summary.
    pub summary: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub children: Vec<String>,
}

/// A hierarchical, line-addressable index of one document.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeIndex {
    pub doc_name: String,
    pub roots: Vec<String>,
    pub nodes: BTreeMap<String, TreeNode>,
}

/// A compact table-of-contents entry (title + summary, no body).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StructureNode {
    pub node_id: String,
    pub title: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub nodes: Vec<StructureNode>,
}

/// A citation back to the exact section + line range an answer came from.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Citation {
    pub node_id: String,
    pub title: String,
    pub breadcrumb: String,
    pub lines: [usize; 2],
}

impl TreeIndex {
    /// Build a tree from a markdown document. Zero-LLM: pure header parsing.
    pub fn from_markdown(text: &str, doc_name: &str) -> Self {
        let lines: Vec<&str> = text.split('\n').collect();
        let mut headers: Vec<(usize, usize, String)> = Vec::new();
        let mut in_code = false;
        for (i, line) in lines.iter().enumerate() {
            let s = line.trim();
            if s.starts_with("```") {
                in_code = !in_code;
                continue;
            }
            if in_code {
                continue;
            }
            if let Some((level, title)) = parse_header(s) {
                headers.push((i + 1, level, title));
            }
        }

        let mut nodes: BTreeMap<String, TreeNode> = BTreeMap::new();
        let mut roots: Vec<String> = Vec::new();
        let mut stack: Vec<(String, usize)> = Vec::new();

        for idx in 0..headers.len() {
            let (line_num, level, ref title) = headers[idx];
            let node_id = format!("{:04}", idx + 1);
            let next_header_line = if idx + 1 < headers.len() {
                headers[idx + 1].0
            } else {
                lines.len() + 1
            };
            let body_start = line_num;
            let body_end = next_header_line.saturating_sub(1);
            let own_text = if body_start < body_end {
                lines[body_start..body_end.min(lines.len())].join("\n").trim().to_string()
            } else {
                String::new()
            };
            let summary = first_sentence(&own_text).unwrap_or_else(|| title.clone());

            let mut node = TreeNode {
                node_id: node_id.clone(),
                title: title.clone(),
                level,
                start_line: line_num,
                end_line: next_header_line - 1,
                own_text,
                summary,
                parent: None,
                children: Vec::new(),
            };

            while let Some(&(_, lvl)) = stack.last() {
                if lvl >= level {
                    stack.pop();
                } else {
                    break;
                }
            }
            if let Some((parent_id, _)) = stack.last().cloned() {
                node.parent = Some(parent_id.clone());
                if let Some(p) = nodes.get_mut(&parent_id) {
                    p.children.push(node_id.clone());
                }
            } else {
                roots.push(node_id.clone());
            }
            stack.push((node_id.clone(), level));
            nodes.insert(node_id, node);
        }

        TreeIndex { doc_name: doc_name.to_string(), roots, nodes }
    }

    /// The path from the root to this node, e.g. "Policies > Leave > Sick Leave".
    pub fn breadcrumb(&self, node_id: &str) -> String {
        let mut parts: Vec<&str> = Vec::new();
        let mut cur = self.nodes.get(node_id);
        while let Some(n) = cur {
            parts.push(&n.title);
            cur = n.parent.as_ref().and_then(|p| self.nodes.get(p));
        }
        parts.reverse();
        parts.join(" > ")
    }

    /// The compact table-of-contents (titles + summaries, no body).
    pub fn structure_map(&self) -> Vec<StructureNode> {
        self.roots.iter().map(|r| self.build_structure(r)).collect()
    }

    fn build_structure(&self, nid: &str) -> StructureNode {
        let n = &self.nodes[nid];
        StructureNode {
            node_id: n.node_id.clone(),
            title: n.title.clone(),
            summary: n.summary.clone(),
            nodes: n.children.iter().map(|c| self.build_structure(c)).collect(),
        }
    }

    /// Score every node against the query. Returns `(node_id, score, matched_terms)`,
    /// sorted best-first. Title matches weigh most, then summary, then body.
    pub fn rank_nodes(&self, query: &str) -> Vec<(String, f64, Vec<String>)> {
        let q_terms: std::collections::BTreeSet<String> = tokens(query).into_iter().collect();
        let mut scored: Vec<(String, f64, Vec<String>)> = Vec::new();
        for (nid, n) in &self.nodes {
            let title_t: std::collections::BTreeSet<String> = tokens(&n.title).into_iter().collect();
            let sum_t: std::collections::BTreeSet<String> = tokens(&n.summary).into_iter().collect();
            let body_counts = count(tokens(&n.own_text));
            let mut matched = Vec::new();
            let mut score = 0.0_f64;
            for q in &q_terms {
                let mut hit = false;
                if title_t.contains(q) {
                    score += 5.0;
                    hit = true;
                }
                if sum_t.contains(q) {
                    score += 2.0;
                    hit = true;
                }
                if let Some(&c) = body_counts.get(q) {
                    score += (c.min(3) as f64) * 1.0;
                    hit = true;
                }
                if hit {
                    matched.push(q.clone());
                }
            }
            if score > 0.0 {
                // prefer leaf/specific sections over broad parents on ties
                score += 0.1 * n.level as f64;
                scored.push((nid.clone(), (score * 100.0).round() / 100.0, matched));
            }
        }
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }

    /// Pick the top-k relevant nodes plus a human-readable reasoning trace.
    pub fn select_nodes(&self, query: &str, k: usize) -> (Vec<String>, String) {
        let ranked: Vec<_> = self.rank_nodes(query).into_iter().take(k).collect();
        if ranked.is_empty() {
            return (Vec::new(), "No section matched the query terms.".to_string());
        }
        let reasoning = ranked
            .iter()
            .map(|(nid, sc, mt)| {
                format!("{nid} ({}) matched {mt:?} score={sc}", self.nodes[nid].title)
            })
            .collect::<Vec<_>>()
            .join("; ");
        (ranked.into_iter().map(|(nid, _, _)| nid).collect(), reasoning)
    }

    /// Navigate the tree and answer the query with citations and a receipt.
    pub fn answer(&self, query: &str, k: usize, prev_hash: &str) -> AnswerResult {
        let (node_ids, reasoning) = self.select_nodes(query, k);
        let mut citations = Vec::new();
        let mut evidence_parts = Vec::new();
        let mut ranges = Vec::new();
        for nid in &node_ids {
            let n = &self.nodes[nid];
            citations.push(Citation {
                node_id: nid.clone(),
                title: n.title.clone(),
                breadcrumb: self.breadcrumb(nid),
                lines: [n.start_line, n.end_line],
            });
            evidence_parts.push(format!("[{}]\n{}", self.breadcrumb(nid), n.own_text));
            ranges.push([n.start_line, n.end_line]);
        }
        let evidence_text = evidence_parts.join("\n\n");
        let answer_text = if evidence_text.is_empty() {
            "No relevant section found.".to_string()
        } else {
            evidence_text.clone()
        };

        let receipt = Receipt::make(query, &node_ids, &ranges, &evidence_text, &answer_text, prev_hash);

        AnswerResult {
            query: query.to_string(),
            answer: answer_text,
            citations,
            visited_node_ids: node_ids,
            fetched_ranges: ranges,
            evidence_text,
            reasoning,
            receipt,
        }
    }

    /// Replay a receipt against THIS tree: re-read the logged nodes and check the
    /// evidence hash still matches. Detects tampering with stored content.
    pub fn verify_receipt(&self, receipt: &Receipt) -> bool {
        let mut parts = Vec::new();
        for nid in &receipt.visited_node_ids {
            match self.nodes.get(nid) {
                Some(n) => parts.push(format!("[{}]\n{}", self.breadcrumb(nid), n.own_text)),
                None => return false,
            }
        }
        hash_text(&parts.join("\n\n")) == receipt.evidence_hash
    }

    /// Returns top-k nodes with scores normalised to [0, 1] (max score = 1.0).
    pub fn rank_nodes_normalized(&self, query: &str, k: usize) -> Vec<(String, f64)> {
        let ranked = self.rank_nodes(query);
        let max = ranked.first().map(|(_, s, _)| *s).unwrap_or(1.0).max(1e-9);
        ranked.into_iter().take(k)
            .map(|(nid, s, _)| (nid, s / max))
            .collect()
    }
}

// ── Answer result ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnswerResult {
    pub query: String,
    pub answer: String,
    pub citations: Vec<Citation>,
    pub visited_node_ids: Vec<String>,
    pub fetched_ranges: Vec<[usize; 2]>,
    pub evidence_text: String,
    pub reasoning: String,
    pub receipt: Receipt,
}

// ── Receipt ───────────────────────────────────────────────────────────────────

/// One tamper-evident record of a single retrieval, chained with BLAKE3.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub query: String,
    pub query_hash: String,
    pub visited_node_ids: Vec<String>,
    pub fetched_ranges: Vec<[usize; 2]>,
    pub evidence_hash: String,
    pub answer_hash: String,
    pub prev_hash: String,
    pub receipt_hash: String,
    pub hash_algo: String,
    pub timestamp: u64,
}

impl Receipt {
    pub fn make(
        query: &str,
        visited_node_ids: &[String],
        fetched_ranges: &[[usize; 2]],
        evidence_text: &str,
        answer: &str,
        prev_hash: &str,
    ) -> Self {
        let query_hash = hash_text(query);
        let evidence_hash = hash_text(evidence_text);
        let answer_hash = hash_text(answer);
        let ranges_str = fetched_ranges
            .iter()
            .map(|r| format!("{}-{}", r[0], r[1]))
            .collect::<Vec<_>>()
            .join(";");
        let nodes_str = visited_node_ids.join(",");
        let receipt_hash = join_hash(&[
            prev_hash,
            &query_hash,
            &nodes_str,
            &ranges_str,
            &evidence_hash,
            &answer_hash,
        ]);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Receipt {
            query: query.to_string(),
            query_hash,
            visited_node_ids: visited_node_ids.to_vec(),
            fetched_ranges: fetched_ranges.to_vec(),
            evidence_hash,
            answer_hash,
            prev_hash: prev_hash.to_string(),
            receipt_hash,
            hash_algo: "blake3".to_string(),
            timestamp,
        }
    }

    fn recompute(&self, prev: &str) -> bool {
        let ranges_str = self
            .fetched_ranges
            .iter()
            .map(|r| format!("{}-{}", r[0], r[1]))
            .collect::<Vec<_>>()
            .join(";");
        let expected = join_hash(&[
            prev,
            &self.query_hash,
            &self.visited_node_ids.join(","),
            &ranges_str,
            &self.evidence_hash,
            &self.answer_hash,
        ]);
        expected == self.receipt_hash && self.prev_hash == prev
    }
}

/// Verify that a sequence of receipts forms an unbroken chain.
pub fn verify_chain(receipts: &[Receipt]) -> bool {
    let mut prev = GENESIS.to_string();
    for r in receipts {
        if !r.recompute(&prev) {
            return false;
        }
        prev = r.receipt_hash.clone();
    }
    true
}

// ── HTTP request / response types ─────────────────────────────────────────────

#[derive(Deserialize)]
pub struct BuildRequest {
    pub text: String,
    #[serde(default)]
    pub doc_name: Option<String>,
}

#[derive(Serialize)]
pub struct BuildResponse {
    pub cache_key: String,
    pub doc_name: String,
    pub node_count: usize,
    pub structure_map: Vec<StructureNode>,
    pub tree: TreeIndex,
}

#[derive(Deserialize)]
pub struct QueryRequest {
    #[serde(default)]
    pub tree: Option<TreeIndex>,
    #[serde(default)]
    pub cache_key: Option<String>,
    pub query: String,
    #[serde(default = "default_k")]
    pub k: usize,
    #[serde(default)]
    pub prev_hash: Option<String>,
}

fn default_k() -> usize { 2 }

#[derive(Deserialize)]
pub struct VerifyRequest {
    pub tree: TreeIndex,
    pub receipt: Receipt,
}

#[derive(Serialize)]
pub struct VerifyResponse {
    pub valid: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HybridHit {
    pub source: String,
    pub score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breadcrumb: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<[usize; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance: Option<f32>,
}

#[derive(Deserialize)]
pub struct HybridRequest {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub tree: Option<TreeIndex>,
    #[serde(default)]
    pub cache_key: Option<String>,
    pub query: String,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default = "default_k")]
    pub k: usize,
    #[serde(default = "default_tree_weight")]
    pub tree_weight: f64,
    #[serde(default)]
    pub prev_hash: Option<String>,
    #[serde(default)]
    pub doc_name: Option<String>,
}

fn default_tree_weight() -> f64 { 0.6 }

#[derive(Serialize, Deserialize)]
pub struct HybridResponse {
    pub query: String,
    pub hits: Vec<HybridHit>,
    pub tree_hit_count: usize,
    pub vector_hit_count: usize,
    pub tree_answer: Option<AnswerResult>,
    pub reasoning: String,
}

// ── Stateless handlers (drop into both routers, no engine access) ─────────────

/// `POST /v1/tree/verify` — replay a receipt against the tree.
pub async fn tree_verify(Json(payload): Json<VerifyRequest>) -> Json<VerifyResponse> {
    Json(VerifyResponse {
        valid: payload.tree.verify_receipt(&payload.receipt),
    })
}

#[derive(Deserialize)]
pub struct ChainVerifyRequest {
    pub receipts: Vec<Receipt>,
}

#[derive(Serialize)]
pub struct ChainVerifyResponse {
    pub valid: bool,
    pub broken_at: Option<usize>,
}

/// `POST /v1/tree/chain-verify` — verify an ordered sequence of receipts forms
/// an unbroken BLAKE3 chain.
pub async fn tree_chain_verify(
    Json(payload): Json<ChainVerifyRequest>,
) -> Json<ChainVerifyResponse> {
    let mut prev = GENESIS.to_string();
    let mut broken_at = None;
    for (i, r) in payload.receipts.iter().enumerate() {
        if !r.recompute(&prev) {
            broken_at = Some(i);
            break;
        }
        prev = r.receipt_hash.clone();
    }
    Json(ChainVerifyResponse { valid: broken_at.is_none(), broken_at })
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn parse_header(s: &str) -> Option<(usize, String)> {
    if !s.starts_with('#') {
        return None;
    }
    let level = s.chars().take_while(|&c| c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = s[level..].trim_start();
    if s.as_bytes().get(level) != Some(&b' ') || rest.is_empty() {
        return None;
    }
    Some((level, rest.trim().to_string()))
}

fn count(toks: Vec<String>) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for t in toks {
        *out.entry(t).or_insert(0) += 1;
    }
    out
}

fn first_sentence(text: &str) -> Option<String> {
    let body: String = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if body.is_empty() {
        return None;
    }
    let mut end = body.len();
    for (i, ch) in body.char_indices() {
        if ch == '.' || ch == '!' || ch == '?' {
            end = i + 1;
            break;
        }
    }
    let s: String = body[..end].chars().take(160).collect();
    Some(s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# Handbook\nIntro line.\n\n## Annual Leave\nYou get 25 annual leave days per year.\n\n## Sick Leave\nYou get 10 paid sick days per year.\n";

    #[test]
    fn builds_tree_with_hierarchy() {
        let t = TreeIndex::from_markdown(DOC, "handbook");
        assert_eq!(t.roots.len(), 1);
        let root = &t.nodes[&t.roots[0]];
        assert_eq!(root.title, "Handbook");
        assert_eq!(root.children.len(), 2);
    }

    #[test]
    fn breadcrumb_walks_to_root() {
        let t = TreeIndex::from_markdown(DOC, "handbook");
        let bc = t.breadcrumb("0003");
        assert_eq!(bc, "Handbook > Sick Leave");
    }

    #[test]
    fn navigator_distinguishes_sick_from_annual() {
        let t = TreeIndex::from_markdown(DOC, "handbook");
        let (ids, _) = t.select_nodes("how many sick days", 1);
        assert_eq!(ids.len(), 1);
        assert_eq!(t.nodes[&ids[0]].title, "Sick Leave");
    }

    #[test]
    fn answer_carries_citation_and_receipt() {
        let t = TreeIndex::from_markdown(DOC, "handbook");
        let r = t.answer("how many sick days", 1, GENESIS);
        assert_eq!(r.citations.len(), 1);
        assert_eq!(r.citations[0].title, "Sick Leave");
        assert!(r.answer.contains("10 paid sick days"));
        assert!(t.verify_receipt(&r.receipt));
    }

    #[test]
    fn tampering_is_caught_by_receipt_replay() {
        let t = TreeIndex::from_markdown(DOC, "handbook");
        let r = t.answer("how many sick days", 1, GENESIS);
        assert!(t.verify_receipt(&r.receipt));

        let mut tampered = t.clone();
        tampered.nodes.get_mut(&r.visited_node_ids[0]).unwrap().own_text =
            "You get 999 paid sick days per year.".to_string();
        assert!(!tampered.verify_receipt(&r.receipt));
    }

    #[test]
    fn receipt_chain_verifies_and_detects_reorder() {
        let t = TreeIndex::from_markdown(DOC, "handbook");
        let a = t.answer("annual leave", 1, GENESIS);
        let b = t.answer("sick days", 1, &a.receipt.receipt_hash);
        let chain = vec![a.receipt.clone(), b.receipt.clone()];
        assert!(verify_chain(&chain));

        let reordered = vec![b.receipt, a.receipt];
        assert!(!verify_chain(&reordered));
    }

    #[test]
    fn round_trips_through_json() {
        let t = TreeIndex::from_markdown(DOC, "handbook");
        let json = serde_json::to_string(&t).unwrap();
        let back: TreeIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(back.nodes.len(), t.nodes.len());
        assert_eq!(back.breadcrumb("0003"), "Handbook > Sick Leave");
    }

    #[test]
    fn deterministic_build_is_reproducible() {
        let a = TreeIndex::from_markdown(DOC, "handbook");
        let b = TreeIndex::from_markdown(DOC, "handbook");
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }
}
