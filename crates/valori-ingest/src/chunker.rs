// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Deterministic text chunking strategies.
//!
//! Four strategies are provided:
//! - **tree** — split on section headers (numbered "3.1 Title" or "## Title").
//!   One chunk per section; title prepended to body.
//! - **conversation** — split on question boundaries (lines ending with `?`).
//!   Groups each Q+answer block as one chunk.
//! - **sentence** — split on sentence endings (`.  !  ?`). Each sentence is one
//!   unit; ±2 surrounding sentences included for LLM context.
//! - **fixed** — overlapping fixed-size windows (default 1000 chars, overlap 200).
//!
//! **auto** sniffs the text and picks the best strategy automatically.

use serde::{Deserialize, Serialize};

/// Maximum accepted text length for ingest/chunk endpoints.
/// 10 MB is generous for real documents; beyond this the chunker + embedding
/// loops become a DoS vector.
pub const MAX_INGEST_TEXT_BYTES: usize = 10 * 1024 * 1024;

// ── Output type ───────────────────────────────────────────────────────────────

/// One chunk produced by a chunking strategy.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IngestChunk {
    /// 0-based position in the chunk sequence.
    pub index: usize,
    /// Section title (tree strategy) or empty string.
    pub title: String,
    /// Full chunk text ready to embed.
    pub text: String,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Chunk `text` using the given `strategy`.
///
/// Returns `(chunks, strategy_name_actually_used)`. The second element differs
/// from `strategy` when auto-detection or fallback occurs (e.g. `"tree->auto"`).
pub fn chunk_document(
    text: &str,
    strategy: &str,
    chunk_size: usize,
    chunk_overlap: usize,
) -> (Vec<IngestChunk>, String) {
    match strategy {
        "tree" => {
            let nodes = chunk_tree(text);
            if nodes.len() >= 2 {
                return (nodes, "tree".into());
            }
            (
                chunk_fixed(text, chunk_size, chunk_overlap),
                "tree->fixed".into(),
            )
        }
        "conversation" => {
            let nodes = chunk_conversation(text);
            if nodes.len() >= 2 {
                return (nodes, "conversation".into());
            }
            let (c, _) = chunk_document(text, "fixed", chunk_size, chunk_overlap);
            (c, "conversation->fixed".into())
        }
        "sentence" => (chunk_sentence_window(text), "sentence".into()),
        "fixed" => (chunk_fixed(text, chunk_size, chunk_overlap), "fixed".into()),
        _ => {
            let detected = detect_strategy(text);
            chunk_document(text, detected, chunk_size, chunk_overlap)
        }
    }
}

/// BLAKE3 content hash of a chunk's text.
///
/// Used by `POST /v1/ingest/update` to diff old vs new chunks — unchanged
/// chunks share the same hash and are not re-embedded.
pub fn chunk_content_hash(text: &str) -> [u8; 32] {
    blake3::hash(text.as_bytes()).into()
}

// ── Auto-detection ────────────────────────────────────────────────────────────

fn detect_strategy(text: &str) -> &'static str {
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len().max(1);

    let header_lines = lines.iter().filter(|l| is_section_header(l)).count();
    let ts_lines = lines
        .iter()
        .filter(|l| {
            let s = l.trim();
            s.len() >= 4
                && s.chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
                && s.contains(':')
                && s.len() <= 8
        })
        .count();
    let question_lines = lines.iter().filter(|l| l.trim().ends_with('?')).count();

    if header_lines * 10 > total {
        "tree"
    } else if ts_lines * 5 > total || (question_lines > 3 && ts_lines > 3) {
        "conversation"
    } else {
        "fixed"
    }
}

// ── Tree chunker ──────────────────────────────────────────────────────────────

fn is_section_header(line: &str) -> bool {
    let s = line.trim();
    if s.is_empty() {
        return false;
    }
    if s.starts_with('#') {
        let rest = s.trim_start_matches('#').trim();
        return rest.len() >= 3;
    }
    let first = s.chars().next().unwrap_or(' ');
    if first.is_ascii_digit() {
        let head: String = s
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let rest = s[head.len()..].trim();
        return rest.len() >= 2
            && rest
                .chars()
                .next()
                .map(|c| c.is_uppercase())
                .unwrap_or(false)
            && rest.len() <= 80
            && !rest.ends_with('.');
    }
    false
}

fn chunk_tree(text: &str) -> Vec<IngestChunk> {
    let normalized = normalize(text);
    let lines: Vec<&str> = normalized.lines().collect();

    let mut header_positions: Vec<usize> = Vec::new();
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
        if is_section_header(s) {
            header_positions.push(i);
        }
    }

    if header_positions.len() < 2 {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    for (idx, &start) in header_positions.iter().enumerate() {
        let end = if idx + 1 < header_positions.len() {
            header_positions[idx + 1]
        } else {
            lines.len()
        };
        let title = lines[start].trim().to_string();
        let body = lines[start + 1..end]
            .iter()
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        if body.len() < 30 && idx + 1 < header_positions.len() {
            continue;
        }
        let combined = format!("{}\n{}", title, body);
        chunks.push(IngestChunk {
            index: chunks.len(),
            title,
            text: combined,
        });
    }
    chunks
}

// ── Conversation chunker ──────────────────────────────────────────────────────

fn strip_timestamp(line: &str) -> &str {
    let s = line.trim();
    if s.len() <= 8 && s.chars().all(|c| c.is_ascii_digit() || c == ':') {
        return "";
    }
    s
}

fn chunk_conversation(text: &str) -> Vec<IngestChunk> {
    let normalized = normalize(text);
    let lines: Vec<&str> = normalized.lines().collect();

    let clean: Vec<String> = lines
        .iter()
        .map(|l| strip_timestamp(l).to_string())
        .filter(|l| !l.is_empty())
        .collect();

    if clean.is_empty() {
        return Vec::new();
    }

    let mut q_indices: Vec<usize> = Vec::new();
    for (i, line) in clean.iter().enumerate() {
        if line.trim().ends_with('?') {
            q_indices.push(i);
        }
    }

    if q_indices.len() < 2 {
        return chunk_fixed_str(&clean.join("\n"), 800, 100);
    }

    let mut chunks: Vec<IngestChunk> = Vec::new();

    for (idx, &q_start) in q_indices.iter().enumerate() {
        let q_end = if idx + 1 < q_indices.len() {
            q_indices[idx + 1]
        } else {
            clean.len()
        };
        let block: Vec<&str> = clean[q_start..q_end].iter().map(|s| s.as_str()).collect();
        let text_block = block.join(" ").trim().to_string();
        if text_block.len() < 20 {
            continue;
        }
        let title = clean[q_start].chars().take(80).collect::<String>();
        chunks.push(IngestChunk {
            index: chunks.len(),
            title,
            text: text_block,
        });
    }

    if let Some(&first_q) = q_indices.first() {
        if first_q > 0 {
            let intro = clean[..first_q].join(" ").trim().to_string();
            if intro.len() >= 30 {
                let mut reindexed = vec![IngestChunk {
                    index: 0,
                    title: "Introduction".into(),
                    text: intro,
                }];
                for (i, mut c) in chunks.into_iter().enumerate() {
                    c.index = i + 1;
                    reindexed.push(c);
                }
                return reindexed;
            }
        }
    }

    chunks
}

// ── Sentence-window chunker ───────────────────────────────────────────────────

fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences: Vec<String> = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        current.push(chars[i]);
        if matches!(chars[i], '.' | '!' | '?') {
            let next_meaningful = chars[i + 1..].iter().find(|&&c| c != ' ' && c != '\n');
            let is_boundary = match next_meaningful {
                None => true,
                Some(&c) => c.is_uppercase() || c == '"' || c == '\'' || c == '(',
            };
            if is_boundary && current.trim().split_whitespace().count() >= 4 {
                sentences.push(current.trim().to_string());
                current = String::new();
            }
        }
        i += 1;
    }
    if !current.trim().is_empty() {
        sentences.push(current.trim().to_string());
    }
    sentences
}

fn chunk_sentence_window(text: &str) -> Vec<IngestChunk> {
    let normalized = normalize(text);
    let sentences = split_sentences(&normalized);
    if sentences.is_empty() {
        return Vec::new();
    }

    const WINDOW: usize = 2;

    sentences
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let lo = i.saturating_sub(WINDOW);
            let hi = (i + WINDOW + 1).min(sentences.len());
            let window_text = sentences[lo..hi].join(" ");
            IngestChunk {
                index: i,
                title: String::new(),
                text: window_text,
            }
        })
        .collect()
}

// ── Fixed-size chunker ────────────────────────────────────────────────────────

fn chunk_fixed(text: &str, size: usize, overlap: usize) -> Vec<IngestChunk> {
    chunk_fixed_str(&normalize(text), size, overlap)
}

pub(crate) fn chunk_fixed_str(text: &str, size: usize, overlap: usize) -> Vec<IngestChunk> {
    let size = size.max(50);
    let overlap = overlap.min(size / 2);
    let step = size - overlap;
    let chars: Vec<char> = text.chars().collect();
    let mut chunks = Vec::new();
    let mut start = 0usize;

    while start < chars.len() {
        let end = (start + size).min(chars.len());
        let end = snap_boundary(&chars, end, 80);
        let chunk_text: String = chars[start..end].iter().collect();
        let chunk_text = chunk_text.trim().to_string();
        if chunk_text.len() >= 30 {
            chunks.push(IngestChunk {
                index: chunks.len(),
                title: String::new(),
                text: chunk_text,
            });
        }
        if end >= chars.len() {
            break;
        }
        start += step;
    }
    chunks
}

fn snap_boundary(chars: &[char], pos: usize, window: usize) -> usize {
    if pos >= chars.len() {
        return chars.len();
    }
    let lo = pos.saturating_sub(window);
    let hi = (pos + window).min(chars.len());
    for i in pos..hi {
        if matches!(chars[i], '.' | '!' | '?' | '\n') {
            return i + 1;
        }
    }
    for i in (lo..pos).rev() {
        if matches!(chars[i], '.' | '!' | '?' | '\n') {
            return i + 1;
        }
    }
    pos
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
        .split('\n')
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_strategy_detects_sections() {
        let doc = "# Introduction\nSome intro text that is long enough.\n## Section One\nContent for section one here.\n## Section Two\nContent for section two here too.";
        let (chunks, strategy) = chunk_document(doc, "tree", 1000, 200);
        assert_eq!(strategy, "tree");
        assert!(chunks.len() >= 2);
        assert!(chunks[0].title.contains("Introduction") || chunks[0].title.contains("Section"));
    }

    #[test]
    fn fixed_strategy_produces_overlapping_chunks() {
        let text = "a".repeat(3000);
        let (chunks, strategy) = chunk_document(&text, "fixed", 1000, 200);
        assert_eq!(strategy, "fixed");
        assert!(chunks.len() >= 3);
    }

    #[test]
    fn auto_falls_back_to_fixed_for_prose() {
        let prose =
            "This is a simple sentence. And another one. And yet another longer sentence here.";
        let (chunks, strategy) = chunk_document(prose, "auto", 1000, 200);
        assert!(strategy.contains("fixed") || strategy.contains("sentence"));
        assert!(!chunks.is_empty());
    }

    #[test]
    fn chunk_content_hash_is_deterministic() {
        let h1 = chunk_content_hash("hello world");
        let h2 = chunk_content_hash("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn chunk_content_hash_differs_for_different_text() {
        let h1 = chunk_content_hash("hello");
        let h2 = chunk_content_hash("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn sentence_window_produces_chunks() {
        let text = "Alice went to the market. She bought apples and pears. The weather was sunny. She came back home.";
        let (chunks, strategy) = chunk_document(text, "sentence", 1000, 200);
        assert_eq!(strategy, "sentence");
        assert!(!chunks.is_empty());
        // Each chunk includes surrounding context
        for chunk in &chunks {
            assert!(!chunk.text.is_empty());
        }
    }

    #[test]
    fn tree_falls_back_to_auto_when_too_few_headers() {
        let text = "# Only One Header\nSome content here but no other sections.";
        let (_, strategy) = chunk_document(text, "tree", 1000, 200);
        assert!(strategy.contains("->"), "expected fallback: got {strategy}");
    }

    #[test]
    fn max_ingest_text_bytes_constant_is_reasonable() {
        assert_eq!(MAX_INGEST_TEXT_BYTES, 10 * 1024 * 1024);
    }
}

// ── Chunker trait ─────────────────────────────────────────────────────────────

use crate::document::{Chunk, Document};

/// Splits a [`Document`] into a sequence of [`Chunk`]s.
///
/// Owns the chunking strategy; downstream stages (embedder, writer) never know
/// whether fixed-size, tree, conversation, or a future semantic strategy ran.
pub trait Chunker: Send + Sync {
    fn chunk(&self, doc: &Document) -> Vec<Chunk>;
}

/// Default chunker — wraps the four built-in strategies (auto/tree/conversation/
/// sentence/fixed) with configurable window parameters.
///
/// Named `DefaultChunker`, not `ValoriChunker` — the name describes behavior,
/// not the brand. Future chunkers (`RecursiveChunker`, `MarkdownChunker`, …)
/// implement the same `Chunker` trait.
#[derive(Debug, Clone)]
pub struct DefaultChunker {
    pub strategy: String,
    pub chunk_size: usize,
    pub chunk_overlap: usize,
}

impl DefaultChunker {
    pub fn new(strategy: impl Into<String>, chunk_size: usize, chunk_overlap: usize) -> Self {
        Self {
            strategy: strategy.into(),
            chunk_size,
            chunk_overlap,
        }
    }
}

impl Default for DefaultChunker {
    fn default() -> Self {
        Self::new("auto", 1000, 200)
    }
}

impl Chunker for DefaultChunker {
    fn chunk(&self, doc: &Document) -> Vec<Chunk> {
        let (raw, _) = chunk_document(
            &doc.content,
            &self.strategy,
            self.chunk_size,
            self.chunk_overlap,
        );
        raw.into_iter()
            .map(|ic| Chunk::new(ic.index, ic.title, ic.text))
            .collect()
    }
}

#[cfg(test)]
mod chunker_trait_tests {
    use super::*;

    #[test]
    fn default_chunker_produces_typed_chunks() {
        let doc = Document::from_text(
            "# Section One\nContent.\n## Section Two\nMore content.",
            Some("test"),
        );
        let c = DefaultChunker::new("tree", 1000, 200);
        let chunks = c.chunk(&doc);
        assert!(!chunks.is_empty());
        // Each chunk carries a stable id derived from its text.
        assert!(chunks.iter().all(|ch| !ch.id.is_empty()));
    }
}
