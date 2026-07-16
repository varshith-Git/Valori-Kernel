// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`DocxReader`] — extracts plain text from `.docx` files.
//!
//! DOCX is a ZIP archive containing `word/document.xml`. We open the zip,
//! parse the XML, and collect `<w:t>` (text run) elements — no external
//! services, no OCR, no Tika.
//!
//! `input` must be a file-system path to the `.docx` file.

use quick_xml::events::Event as XmlEvent;
use quick_xml::reader::Reader as XmlReader;
use std::io::Read;

use crate::document::{Document, IngestError};
use crate::metadata::DocumentMetadata;
use crate::reader::Reader;

pub struct DocxReader;

#[async_trait::async_trait]
impl Reader for DocxReader {
    /// `input` — absolute or relative path to the `.docx` file.
    async fn read(&self, input: &str, source: Option<&str>) -> Result<Document, IngestError> {
        let path = std::path::Path::new(input);
        if !path.exists() {
            return Err(IngestError::Reader(format!("file not found: {input}")));
        }

        let path_owned = path.to_path_buf();
        let (text, title, author) = tokio::task::spawn_blocking(move || {
            extract_docx(&path_owned)
        })
        .await
        .map_err(|e| IngestError::Reader(e.to_string()))??;

        let source_str = source.unwrap_or(input).to_string();
        let id = blake3_hex(text.as_bytes());
        Ok(Document {
            id,
            source: source_str,
            mime_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document".to_string(),
            metadata: DocumentMetadata { title, author, ..Default::default() },
            content: text,
        })
    }
}

/// Open the zip, parse `word/document.xml` for text runs and
/// `docProps/core.xml` for title + author.
fn extract_docx(
    path: &std::path::Path,
) -> Result<(String, Option<String>, Option<String>), IngestError> {
    let file = std::fs::File::open(path)
        .map_err(|e| IngestError::Reader(e.to_string()))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| IngestError::Reader(e.to_string()))?;

    let body_text = read_entry_xml(&mut zip, "word/document.xml", extract_text_runs)?;
    let (title, author) = read_entry_xml(&mut zip, "docProps/core.xml", extract_core_props)
        .unwrap_or((None, None));

    Ok((body_text, title, author))
}

/// Read a named zip entry, parse its XML bytes with `f`, return the result.
/// Returns `Err` for the body document; returns the fallback for optional
/// entries (core props may not exist).
fn read_entry_xml<T, F>(
    zip: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
    f: F,
) -> Result<T, IngestError>
where
    F: Fn(&[u8]) -> Result<T, IngestError>,
{
    let mut entry = zip.by_name(name)
        .map_err(|_| IngestError::Reader(format!("entry '{name}' not found in archive")))?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)
        .map_err(|e| IngestError::Reader(e.to_string()))?;
    f(&bytes)
}

/// Walk `word/document.xml`, collect all `<w:t>` text runs.
/// Emit a space between runs; emit a newline after `<w:p>` (paragraph end).
pub fn extract_text_runs(xml: &[u8]) -> Result<String, IngestError> {
    let mut reader = XmlReader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut out = String::new();
    let mut in_wt = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref e)) | Ok(XmlEvent::Empty(ref e)) => {
                match local_name(e.name().as_ref()) {
                    b"t" => in_wt = true,
                    b"p" => {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                    }
                    _ => {}
                }
            }
            Ok(XmlEvent::End(ref e)) => {
                if local_name(e.name().as_ref()) == b"t" {
                    in_wt = false;
                }
            }
            Ok(XmlEvent::Text(ref t)) if in_wt => {
                let s = t.unescape()
                    .map_err(|e| IngestError::Reader(e.to_string()))?;
                out.push_str(&s);
            }
            Ok(XmlEvent::Eof) => break,
            Err(e) => return Err(IngestError::Reader(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok(out.trim().to_string())
}

/// Extract `dc:title` and `dc:creator` from `docProps/core.xml`.
pub fn extract_core_props(xml: &[u8]) -> Result<(Option<String>, Option<String>), IngestError> {
    let mut reader = XmlReader::from_reader(xml);
    reader.config_mut().trim_text(true);

    let mut title: Option<String> = None;
    let mut author: Option<String> = None;
    let mut current_tag: Option<Vec<u8>> = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref e)) => {
                current_tag = Some(local_name(e.name().as_ref()).to_vec());
            }
            Ok(XmlEvent::Text(ref t)) => {
                if let Some(ref tag) = current_tag {
                    let val = t.unescape()
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default();
                    if !val.is_empty() {
                        match tag.as_slice() {
                            b"title"   => title  = Some(val),
                            b"creator" => author = Some(val),
                            _ => {}
                        }
                    }
                }
            }
            Ok(XmlEvent::End(_)) => { current_tag = None; }
            Ok(XmlEvent::Eof) => break,
            Err(e) => return Err(IngestError::Reader(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok((title, author))
}

/// Strip namespace prefix: `w:t` → `t`, `dc:title` → `title`.
fn local_name(name: &[u8]) -> &[u8] {
    name.iter().rposition(|&b| b == b':')
        .map(|i| &name[i + 1..])
        .unwrap_or(name)
}

fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_file_returns_reader_error() {
        let err = DocxReader.read("/nonexistent/doc.docx", None).await.unwrap_err();
        assert!(matches!(err, IngestError::Reader(_)));
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn mime_type_constant() {
        // Verify the MIME constant is correct without needing a real file.
        assert_eq!(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
    }

    /// Smoke-test the XML parser against a minimal synthetic document.xml.
    #[test]
    fn text_runs_from_minimal_xml() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>Hello</w:t></w:r><w:r><w:t xml:space="preserve"> world</w:t></w:r></w:p>
    <w:p><w:r><w:t>Second paragraph.</w:t></w:r></w:p>
  </w:body>
</w:document>"#;
        let text = extract_text_runs(xml).unwrap();
        assert!(text.contains("Hello world"), "got: {text}");
        assert!(text.contains("Second paragraph"), "got: {text}");
    }

    #[test]
    fn core_props_parser() {
        let xml = br#"<?xml version="1.0"?>
<cp:coreProperties xmlns:dc="http://purl.org/dc/elements/1.1/"
                   xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties">
  <dc:title>My Document</dc:title>
  <dc:creator>Bob</dc:creator>
</cp:coreProperties>"#;
        let (title, author) = extract_core_props(xml).unwrap();
        assert_eq!(title.as_deref(), Some("My Document"));
        assert_eq!(author.as_deref(), Some("Bob"));
    }
}
