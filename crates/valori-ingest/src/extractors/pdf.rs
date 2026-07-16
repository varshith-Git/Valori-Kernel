use crate::document::{Document, IngestError};
use crate::extractor::{Extractor, ReaderCapabilities};
use crate::metadata::DocumentMetadata;

pub struct PdfExtractor;

impl Extractor for PdfExtractor {
    /// Extract text from PDF bytes. CPU-bound; wrap in `spawn_blocking` if on an async thread.
    fn extract(&self, bytes: &[u8], source: Option<&str>) -> Result<Document, IngestError> {
        let text = pdf_extract::extract_text_from_mem(bytes)
            .map_err(|e| IngestError::Reader(e.to_string()))?;

        let page_count = lopdf::Document::load_mem(bytes)
            .map(|doc| doc.get_pages().len())
            .ok();

        let id = blake3_hex(text.as_bytes());
        Ok(Document {
            id,
            source: source.unwrap_or("document.pdf").to_string(),
            mime_type: "application/pdf".to_string(),
            metadata: DocumentMetadata {
                page_count,
                ..Default::default()
            },
            content: text,
        })
    }

    fn capabilities(&self) -> ReaderCapabilities {
        ReaderCapabilities {
            extensions: vec!["pdf"],
            mime_types: vec!["application/pdf"],
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
    fn empty_bytes_returns_reader_error() {
        // PDF magic bytes are %PDF-; empty bytes are not a valid PDF.
        let err = PdfExtractor.extract(b"", None).unwrap_err();
        assert!(matches!(err, IngestError::Reader(_)));
    }

    #[test]
    fn garbage_bytes_returns_reader_error() {
        let err = PdfExtractor.extract(b"not a pdf", None).unwrap_err();
        assert!(matches!(err, IngestError::Reader(_)));
    }
}
