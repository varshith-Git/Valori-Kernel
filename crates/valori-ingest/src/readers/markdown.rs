// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`MarkdownReader`] — strips Markdown formatting and extracts plain text.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::document::{Document, IngestError};
use crate::metadata::DocumentMetadata;
use crate::reader::Reader;

/// Parses CommonMark Markdown, strips formatting, and returns plain text.
///
/// Preserves paragraph structure via newlines. Extracts a `title` from the
/// first ATX heading (`# …`) when present.
pub struct MarkdownReader;

#[async_trait::async_trait]
impl Reader for MarkdownReader {
    async fn read(&self, input: &str, source: Option<&str>) -> Result<Document, IngestError> {
        let (text, title) = extract_text_and_title(input);
        let source_str = source.unwrap_or("document.md").to_string();
        let id = blake3_hex(text.as_bytes());
        Ok(Document {
            id,
            source: source_str,
            mime_type: "text/markdown".to_string(),
            metadata: DocumentMetadata { title, ..Default::default() },
            content: text,
        })
    }
}

/// Public so `MarkdownExtractor` can reuse the parsing logic without
/// duplicating the pulldown-cmark traversal.
pub fn extract_text_and_title(md: &str) -> (String, Option<String>) {
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH;
    let parser = Parser::new_ext(md, opts);

    let mut out = String::new();
    let mut title: Option<String> = None;
    let mut in_heading1 = false;
    let mut heading_buf = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) if level == pulldown_cmark::HeadingLevel::H1 => {
                in_heading1 = true;
            }
            Event::End(TagEnd::Heading(_)) => {
                if in_heading1 {
                    if title.is_none() {
                        title = Some(heading_buf.trim().to_string());
                    }
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(heading_buf.trim());
                    out.push('\n');
                    heading_buf.clear();
                    in_heading1 = false;
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if in_heading1 {
                    heading_buf.push_str(&t);
                } else {
                    out.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            Event::End(TagEnd::Paragraph) => out.push('\n'),
            Event::End(TagEnd::Item) => out.push('\n'),
            _ => {}
        }
    }

    (out.trim().to_string(), title)
}

fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn extracts_plain_text() {
        let md = "# Hello\n\nThis is **bold** and _italic_.\n\n- item one\n- item two\n";
        let doc = MarkdownReader.read(md, Some("readme.md")).await.unwrap();
        assert_eq!(doc.mime_type, "text/markdown");
        assert!(doc.content.contains("Hello"));
        assert!(doc.content.contains("bold"));
        assert!(doc.content.contains("item one"));
        assert!(!doc.content.contains("**"));
        assert!(!doc.content.contains("_"));
    }

    #[tokio::test]
    async fn extracts_h1_title() {
        let md = "# My Document\n\nSome text.\n";
        let doc = MarkdownReader.read(md, None).await.unwrap();
        assert_eq!(doc.metadata.title.as_deref(), Some("My Document"));
    }

    #[tokio::test]
    async fn no_title_when_no_h1() {
        let md = "## Section\n\nContent.\n";
        let doc = MarkdownReader.read(md, None).await.unwrap();
        assert!(doc.metadata.title.is_none());
    }

    #[tokio::test]
    async fn source_defaults_to_md_extension() {
        let doc = MarkdownReader.read("hello", None).await.unwrap();
        assert_eq!(doc.source, "document.md");
    }

    #[tokio::test]
    async fn id_is_stable() {
        let md = "# A\n\nB\n";
        let a = MarkdownReader.read(md, None).await.unwrap();
        let b = MarkdownReader.read(md, None).await.unwrap();
        assert_eq!(a.id, b.id);
    }
}
