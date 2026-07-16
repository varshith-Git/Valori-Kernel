// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`Reader`] trait + [`TextReader`] — the first stage of the ingest pipeline.

use crate::document::{Document, IngestError};

/// Converts raw input into a [`Document`].
///
/// Implementations own the format concern: `TextReader` accepts a plain string;
/// future readers (`MarkdownReader`, `PdfReader`, …) will handle their formats
/// without changing any other pipeline stage.
#[async_trait::async_trait]
pub trait Reader: Send + Sync {
    async fn read(&self, input: &str, source: Option<&str>) -> Result<Document, IngestError>;
}

/// Accepts plain text as-is. No parsing, no conversion.
///
/// Named `TextReader` not `ValoriReader` — describes the format, not the brand.
pub struct TextReader;

#[async_trait::async_trait]
impl Reader for TextReader {
    async fn read(&self, input: &str, source: Option<&str>) -> Result<Document, IngestError> {
        Ok(Document::from_text(input, source))
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn text_reader_produces_document() {
        let r = TextReader;
        let doc = r.read("hello world", Some("test.txt")).await.unwrap();
        assert_eq!(doc.content, "hello world");
        assert_eq!(doc.source, "test.txt");
        assert_eq!(doc.mime_type, "text/plain");
        assert!(!doc.id.is_empty());
    }

    #[tokio::test]
    async fn text_reader_default_source() {
        let r = TextReader;
        let doc = r.read("hi", None).await.unwrap();
        assert_eq!(doc.source, "text");
    }
}
