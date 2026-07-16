use std::io::{Cursor, Read};

use crate::document::{Document, IngestError};
use crate::extractor::{Extractor, ReaderCapabilities};
use crate::metadata::DocumentMetadata;
use crate::readers::docx::{extract_core_props, extract_text_runs};

pub struct DocxExtractor;

impl Extractor for DocxExtractor {
    /// Extract text from DOCX bytes. CPU-bound; wrap in `spawn_blocking` if on an async thread.
    fn extract(&self, bytes: &[u8], source: Option<&str>) -> Result<Document, IngestError> {
        let cursor = Cursor::new(bytes);
        let mut zip =
            zip::ZipArchive::new(cursor).map_err(|e| IngestError::Reader(e.to_string()))?;

        let body = read_entry(&mut zip, "word/document.xml")?;
        let text = extract_text_runs(&body)?;

        let (title, author) = read_entry(&mut zip, "docProps/core.xml")
            .ok()
            .and_then(|b| extract_core_props(&b).ok())
            .unwrap_or((None, None));

        let id = blake3_hex(text.as_bytes());
        Ok(Document {
            id,
            source: source.unwrap_or("document.docx").to_string(),
            mime_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                .to_string(),
            metadata: DocumentMetadata {
                title,
                author,
                ..Default::default()
            },
            content: text,
        })
    }

    fn capabilities(&self) -> ReaderCapabilities {
        ReaderCapabilities {
            extensions: vec!["docx"],
            mime_types: vec![
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ],
            supports_streaming: false,
            supports_metadata: true,
            supports_images: false,
        }
    }
}

fn read_entry(
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Vec<u8>, IngestError> {
    let mut entry = zip
        .by_name(name)
        .map_err(|_| IngestError::Reader(format!("entry '{name}' not found in archive")))?;
    let mut buf = Vec::new();
    entry
        .read_to_end(&mut buf)
        .map_err(|e| IngestError::Reader(e.to_string()))?;
    Ok(buf)
}

fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bytes_returns_reader_error() {
        let err = DocxExtractor.extract(b"", None).unwrap_err();
        assert!(matches!(err, IngestError::Reader(_)));
    }

    #[test]
    fn garbage_bytes_returns_reader_error() {
        let err = DocxExtractor.extract(b"not a zip", None).unwrap_err();
        assert!(matches!(err, IngestError::Reader(_)));
    }
}
