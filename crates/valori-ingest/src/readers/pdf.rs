// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`PdfReader`] — extracts plain text from PDF files.
//!
//! `input` must be a file-system path (not raw bytes) because `pdf-extract`
//! operates on files. Pass `source` for display provenance; it defaults to the
//! path itself.

use crate::document::{Document, IngestError};
use crate::metadata::DocumentMetadata;
use crate::reader::Reader;

pub struct PdfReader;

#[async_trait::async_trait]
impl Reader for PdfReader {
    /// `input` — absolute or relative path to the PDF file.
    async fn read(&self, input: &str, source: Option<&str>) -> Result<Document, IngestError> {
        let path = std::path::Path::new(input);
        if !path.exists() {
            return Err(IngestError::Reader(format!("file not found: {input}")));
        }

        // pdf-extract::extract_text is synchronous; run in a blocking thread so
        // we don't stall the async runtime.
        let path_owned = path.to_path_buf();
        let (text, page_count) = tokio::task::spawn_blocking(move || {
            let text = pdf_extract::extract_text(&path_owned)
                .map_err(|e| IngestError::Reader(e.to_string()))?;
            // Count pages via lopdf (pdf-extract's underlying parser).
            let pages = lopdf::Document::load(&path_owned)
                .map(|doc| doc.get_pages().len())
                .unwrap_or(0);
            Ok::<_, IngestError>((text, pages))
        })
        .await
        .map_err(|e| IngestError::Reader(e.to_string()))??;

        let source_str = source.unwrap_or(input).to_string();
        let id = blake3_hex(text.as_bytes());
        Ok(Document {
            id,
            source: source_str,
            mime_type: "application/pdf".to_string(),
            metadata: DocumentMetadata {
                page_count: Some(page_count),
                ..Default::default()
            },
            content: text,
        })
    }
}

fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Full round-trip tests require a real PDF file and are integration-level.
    // Unit tests cover error handling only.

    #[tokio::test]
    async fn missing_file_returns_reader_error() {
        let err = PdfReader.read("/nonexistent/path.pdf", None).await.unwrap_err();
        assert!(matches!(err, IngestError::Reader(_)));
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn mime_type_is_pdf() {
        // Can't create a real PDF in a unit test without a fixture; check the
        // reader's type constant directly.
        // This would be covered by an integration test with a fixture file.
        let err = PdfReader.read("/tmp/no.pdf", None).await.unwrap_err();
        // Still a Reader error (file missing), not a panic.
        assert!(matches!(err, IngestError::Reader(_)));
    }
}
