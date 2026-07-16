use crate::document::{Document, IngestError};
use crate::extractor::{Extractor, ReaderCapabilities};
use crate::metadata::DocumentMetadata;
// Reuse the extraction logic from the reader module.
use crate::readers::markdown::extract_text_and_title;

pub struct MarkdownExtractor;

impl Extractor for MarkdownExtractor {
    fn extract(&self, bytes: &[u8], source: Option<&str>) -> Result<Document, IngestError> {
        let raw = std::str::from_utf8(bytes)
            .map_err(|_| IngestError::Reader("Markdown input is not valid UTF-8".into()))?;
        let (text, title) = extract_text_and_title(raw);
        let id = blake3_hex(text.as_bytes());
        Ok(Document {
            id,
            source: source.unwrap_or("document.md").to_string(),
            mime_type: "text/markdown".to_string(),
            metadata: DocumentMetadata { title, ..Default::default() },
            content: text,
        })
    }

    fn capabilities(&self) -> ReaderCapabilities {
        ReaderCapabilities {
            extensions: vec!["md", "markdown"],
            mime_types: vec!["text/markdown", "text/x-markdown"],
            supports_streaming: false,
            supports_metadata: true,
            supports_images: false,
        }
    }
}

fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_markdown_formatting() {
        let md = b"# Hello\n\nThis is **bold**.\n";
        let doc = MarkdownExtractor.extract(md, None).unwrap();
        assert!(doc.content.contains("Hello"));
        assert!(doc.content.contains("bold"));
        assert!(!doc.content.contains("**"));
    }

    #[test]
    fn extracts_title_from_h1() {
        let md = b"# My Title\n\nBody text.\n";
        let doc = MarkdownExtractor.extract(md, None).unwrap();
        assert_eq!(doc.metadata.title.as_deref(), Some("My Title"));
    }
}
