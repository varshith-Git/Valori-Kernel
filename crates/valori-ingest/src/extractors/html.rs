use scraper::Html;

use crate::document::{Document, IngestError};
use crate::extractor::{Extractor, ReaderCapabilities};
use crate::metadata::DocumentMetadata;
use crate::readers::html::{extract_body_text, extract_meta, extract_meta_name};

pub struct HtmlExtractor;

impl Extractor for HtmlExtractor {
    fn extract(&self, bytes: &[u8], source: Option<&str>) -> Result<Document, IngestError> {
        let raw = std::str::from_utf8(bytes)
            .map_err(|_| IngestError::Reader("HTML input is not valid UTF-8".into()))?;
        let parsed = Html::parse_document(raw);
        let title  = extract_meta(&parsed, "title");
        let author = extract_meta_name(&parsed, "author");
        let text   = extract_body_text(&parsed);
        let id = blake3_hex(text.as_bytes());
        Ok(Document {
            id,
            source: source.unwrap_or("document.html").to_string(),
            mime_type: "text/html".to_string(),
            metadata: DocumentMetadata { title, author, ..Default::default() },
            content: text,
        })
    }

    fn capabilities(&self) -> ReaderCapabilities {
        ReaderCapabilities {
            extensions: vec!["html", "htm"],
            mime_types: vec!["text/html"],
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
    fn extracts_visible_text_and_strips_scripts() {
        let html = b"<html><head><title>T</title></head><body><p>Hello</p><script>alert(1)</script></body></html>";
        let doc = HtmlExtractor.extract(html, None).unwrap();
        assert!(doc.content.contains("Hello"));
        assert!(!doc.content.contains("alert"));
        assert_eq!(doc.metadata.title.as_deref(), Some("T"));
    }
}
