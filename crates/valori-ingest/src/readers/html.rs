// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`HtmlReader`] — extracts visible text from HTML documents.

use scraper::{Html, Selector};

use crate::document::{Document, IngestError};
use crate::metadata::DocumentMetadata;
use crate::reader::Reader;

pub struct HtmlReader;

#[async_trait::async_trait]
impl Reader for HtmlReader {
    async fn read(&self, input: &str, source: Option<&str>) -> Result<Document, IngestError> {
        let doc = Html::parse_document(input);
        let source_str = source.unwrap_or("document.html").to_string();

        let title = extract_meta(&doc, "title");
        let author = extract_meta_name(&doc, "author");
        let text = extract_body_text(&doc);

        let id = blake3_hex(text.as_bytes());
        Ok(Document {
            id,
            source: source_str,
            mime_type: "text/html".to_string(),
            metadata: DocumentMetadata {
                title,
                author,
                ..Default::default()
            },
            content: text,
        })
    }
}

pub fn extract_meta(doc: &Html, tag: &str) -> Option<String> {
    let sel = Selector::parse(tag).ok()?;
    doc.select(&sel)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn extract_meta_name(doc: &Html, name: &str) -> Option<String> {
    let query = format!("meta[name=\"{name}\"]");
    let sel = Selector::parse(&query).ok()?;
    doc.select(&sel)
        .next()
        .and_then(|el| el.value().attr("content"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Walk the DOM body, skip script/style subtrees entirely.
pub fn extract_body_text(doc: &Html) -> String {
    let body_sel = Selector::parse("body").unwrap();
    let skip_sel = Selector::parse("script, style, noscript").unwrap();

    let root = doc.select(&body_sel).next();
    let target = match root {
        Some(b) => b,
        None => return String::new(),
    };

    let mut out = String::new();
    collect_text(target, &skip_sel, &mut out);
    out
}

fn collect_text(el: scraper::ElementRef<'_>, skip: &Selector, out: &mut String) {
    for child in el.children() {
        if let Some(child_el) = scraper::ElementRef::wrap(child) {
            if skip.matches(&child_el) {
                continue; // prune entire subtree
            }
            collect_text(child_el, skip, out);
        } else if let Some(text) = child.value().as_text() {
            let t = text.trim();
            if !t.is_empty() {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(t);
            }
        }
    }
}

fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<!DOCTYPE html>
<html>
<head>
  <title>Test Page</title>
  <meta name="author" content="Alice">
  <style>body { color: red; }</style>
</head>
<body>
  <h1>Hello</h1>
  <p>World paragraph.</p>
  <script>alert('x')</script>
</body>
</html>"#;

    #[tokio::test]
    async fn extracts_visible_text() {
        let doc = HtmlReader.read(SAMPLE, Some("page.html")).await.unwrap();
        assert!(doc.content.contains("Hello"));
        assert!(doc.content.contains("World paragraph"));
        assert!(!doc.content.contains("alert"));
        assert!(!doc.content.contains("color: red"));
    }

    #[tokio::test]
    async fn extracts_title_and_author() {
        let doc = HtmlReader.read(SAMPLE, None).await.unwrap();
        assert_eq!(doc.metadata.title.as_deref(), Some("Test Page"));
        assert_eq!(doc.metadata.author.as_deref(), Some("Alice"));
    }

    #[tokio::test]
    async fn mime_type_is_html() {
        let doc = HtmlReader.read("<p>hi</p>", None).await.unwrap();
        assert_eq!(doc.mime_type, "text/html");
    }

    #[tokio::test]
    async fn source_defaults() {
        let doc = HtmlReader.read("<p>hi</p>", None).await.unwrap();
        assert_eq!(doc.source, "document.html");
    }
}
