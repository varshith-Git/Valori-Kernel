use crate::document::{Document, IngestError};
use crate::extractor::{Extractor, ReaderCapabilities};
use crate::metadata::DocumentMetadata;

pub struct TextExtractor;

impl Extractor for TextExtractor {
    fn extract(&self, bytes: &[u8], source: Option<&str>) -> Result<Document, IngestError> {
        let content = std::str::from_utf8(bytes)
            .map_err(|_| IngestError::Reader("input is not valid UTF-8".into()))?
            .to_string();
        let id = blake3_hex(bytes);
        Ok(Document {
            id,
            source: source.unwrap_or("text").to_string(),
            mime_type: "text/plain".to_string(),
            metadata: DocumentMetadata::default(),
            content,
        })
    }

    fn capabilities(&self) -> ReaderCapabilities {
        ReaderCapabilities {
            extensions: vec!["txt", "text"],
            mime_types: vec!["text/plain"],
            supports_streaming: false,
            supports_metadata: false,
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
    fn extracts_utf8_text() {
        let doc = TextExtractor.extract(b"hello world", Some("a.txt")).unwrap();
        assert_eq!(doc.content, "hello world");
        assert_eq!(doc.mime_type, "text/plain");
        assert_eq!(doc.source, "a.txt");
    }

    #[test]
    fn rejects_invalid_utf8() {
        let err = TextExtractor.extract(&[0xFF, 0xFE], None).unwrap_err();
        assert!(matches!(err, IngestError::Reader(_)));
    }

    #[test]
    fn id_is_stable() {
        let a = TextExtractor.extract(b"abc", None).unwrap();
        let b = TextExtractor.extract(b"abc", None).unwrap();
        assert_eq!(a.id, b.id);
    }
}
