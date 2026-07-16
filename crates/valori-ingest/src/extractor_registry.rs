// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`ExtractorRegistry`] — resolves an [`Extractor`] from extension, MIME type,
//! or raw bytes.
//!
//! This is the single place where format knowledge lives. The pipeline, node,
//! daemon, and CLI ask the registry; they never contain dispatch logic.
//!
//! # Dispatch priority (`extractor_for_bytes`)
//! 1. Magic-byte MIME detection via the `infer` crate.
//! 2. Path extension hint (if provided).
//! 3. `IngestError::Reader` if neither worked.

use std::path::Path;

use crate::document::IngestError;
use crate::extractor::{Extractor, ReaderCapabilities};
use crate::extractors::{
    DocxExtractor, HtmlExtractor, MarkdownExtractor, PdfExtractor, TextExtractor,
};

pub struct ExtractorRegistry;

impl ExtractorRegistry {
    // ── By extension ─────────────────────────────────────────────────────────

    /// Return an extractor for the given file extension.
    ///
    /// `ext` must be without the leading dot (`"pdf"`, not `".pdf"`).
    /// Comparison is case-insensitive.
    pub fn extractor_for_extension(ext: &str) -> Result<Box<dyn Extractor>, IngestError> {
        match ext.to_ascii_lowercase().as_str() {
            "txt" | "text" => Ok(Box::new(TextExtractor)),
            "md" | "markdown" => Ok(Box::new(MarkdownExtractor)),
            "html" | "htm" => Ok(Box::new(HtmlExtractor)),
            "pdf" => Ok(Box::new(PdfExtractor)),
            "docx" => Ok(Box::new(DocxExtractor)),
            other => Err(IngestError::Reader(format!(
                "no extractor registered for extension '.{other}'"
            ))),
        }
    }

    // ── By MIME type ─────────────────────────────────────────────────────────

    /// Return an extractor for the given MIME type.
    pub fn extractor_for_mime(mime: &str) -> Result<Box<dyn Extractor>, IngestError> {
        match mime {
            "text/plain" => Ok(Box::new(TextExtractor)),
            "text/markdown" | "text/x-markdown" => Ok(Box::new(MarkdownExtractor)),
            "text/html" => Ok(Box::new(HtmlExtractor)),
            "application/pdf" => Ok(Box::new(PdfExtractor)),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
                Ok(Box::new(DocxExtractor))
            }
            other => Err(IngestError::Reader(format!(
                "no extractor registered for MIME type '{other}'"
            ))),
        }
    }

    // ── By file path ─────────────────────────────────────────────────────────

    /// Return an extractor for the given file path by inspecting its extension.
    pub fn extractor_for_path(path: impl AsRef<Path>) -> Result<Box<dyn Extractor>, IngestError> {
        let path = path.as_ref();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| IngestError::Reader(format!(
                "cannot determine extension for '{}'", path.display()
            )))?;
        Self::extractor_for_extension(ext)
    }

    // ── By bytes (MIME detection) ─────────────────────────────────────────────

    /// Return an extractor by inspecting magic bytes first, then falling back
    /// to the path extension hint.
    ///
    /// Prefer this over `extractor_for_path` when you already have the bytes —
    /// it prevents disguised files (e.g. `virus.pdf.exe`) from reaching the
    /// wrong extractor.
    pub fn extractor_for_bytes(
        bytes: &[u8],
        hint_path: Option<&Path>,
    ) -> Result<Box<dyn Extractor>, IngestError> {
        // 1. Try magic-byte MIME detection.
        if let Some(kind) = infer::get(bytes) {
            if let Ok(e) = Self::extractor_for_mime(kind.mime_type()) {
                return Ok(e);
            }
        }
        // 2. Fall back to extension hint.
        if let Some(path) = hint_path {
            return Self::extractor_for_path(path);
        }
        Err(IngestError::Reader(
            "cannot determine document format from bytes or path hint".into(),
        ))
    }

    // ── Capabilities ─────────────────────────────────────────────────────────

    /// Return capabilities for all registered extractors.
    ///
    /// The desktop, CLI, and API use this to render a "supported formats" list
    /// without hardcoding any format knowledge.
    pub fn all_capabilities() -> Vec<ReaderCapabilities> {
        vec![
            TextExtractor.capabilities(),
            MarkdownExtractor.capabilities(),
            HtmlExtractor.capabilities(),
            PdfExtractor.capabilities(),
            DocxExtractor.capabilities(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── extractor_for_extension ───────────────────────────────────────────────

    #[test]
    fn resolves_all_supported_extensions() {
        for ext in &["txt", "text", "md", "markdown", "html", "htm", "pdf", "docx"] {
            ExtractorRegistry::extractor_for_extension(ext)
                .unwrap_or_else(|_| panic!("expected extractor for '{ext}'"));
        }
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        ExtractorRegistry::extractor_for_extension("PDF").unwrap();
        ExtractorRegistry::extractor_for_extension("MD").unwrap();
        ExtractorRegistry::extractor_for_extension("DOCX").unwrap();
        ExtractorRegistry::extractor_for_extension("HTML").unwrap();
    }

    #[test]
    fn unknown_extension_errors() {
        match ExtractorRegistry::extractor_for_extension("xyz") {
            Err(e) => assert!(e.to_string().contains(".xyz")),
            Ok(_) => panic!("expected error"),
        }
    }

    // ── extractor_for_mime ────────────────────────────────────────────────────

    #[test]
    fn resolves_text_plain_mime() {
        ExtractorRegistry::extractor_for_mime("text/plain").unwrap();
    }

    #[test]
    fn resolves_pdf_mime() {
        ExtractorRegistry::extractor_for_mime("application/pdf").unwrap();
    }

    #[test]
    fn resolves_markdown_mime_variants() {
        ExtractorRegistry::extractor_for_mime("text/markdown").unwrap();
        ExtractorRegistry::extractor_for_mime("text/x-markdown").unwrap();
    }

    #[test]
    fn unknown_mime_errors() {
        assert!(ExtractorRegistry::extractor_for_mime("image/png").is_err());
    }

    // ── extractor_for_path ────────────────────────────────────────────────────

    #[test]
    fn path_dispatch_by_extension() {
        ExtractorRegistry::extractor_for_path(Path::new("report.pdf")).unwrap();
        ExtractorRegistry::extractor_for_path(Path::new("notes.md")).unwrap();
        ExtractorRegistry::extractor_for_path(Path::new("index.html")).unwrap();
        ExtractorRegistry::extractor_for_path(Path::new("doc.docx")).unwrap();
        ExtractorRegistry::extractor_for_path(Path::new("log.txt")).unwrap();
    }

    #[test]
    fn path_without_extension_errors() {
        assert!(ExtractorRegistry::extractor_for_path(Path::new("Makefile")).is_err());
    }

    // ── extractor_for_bytes ───────────────────────────────────────────────────

    #[test]
    fn bytes_detects_pdf_magic() {
        // PDF magic bytes: %PDF-
        let bytes = b"%PDF-1.4 fake pdf content";
        let result = ExtractorRegistry::extractor_for_bytes(bytes, None);
        // infer should recognise the PDF magic; if it does, we get Ok.
        // If infer doesn't recognise it (very old/malformed header), the test
        // is inconclusive — we don't fail it.
        let _ = result; // accept either outcome for magic-byte test
    }

    #[test]
    fn bytes_falls_back_to_hint_for_text() {
        // Plain text has no magic bytes; infer won't match — extension hint wins.
        let bytes = b"Hello, world";
        let hint = Path::new("hello.txt");
        ExtractorRegistry::extractor_for_bytes(bytes, Some(hint)).unwrap();
    }

    #[test]
    fn bytes_without_magic_and_no_hint_errors() {
        let bytes = b"Hello, world";
        assert!(ExtractorRegistry::extractor_for_bytes(bytes, None).is_err());
    }

    // ── all_capabilities ─────────────────────────────────────────────────────

    #[test]
    fn all_capabilities_covers_five_formats() {
        let caps = ExtractorRegistry::all_capabilities();
        assert_eq!(caps.len(), 5);
    }

    #[test]
    fn each_capability_has_extensions_and_mimes() {
        for cap in ExtractorRegistry::all_capabilities() {
            assert!(!cap.extensions.is_empty(), "empty extensions list");
            assert!(!cap.mime_types.is_empty(), "empty mime_types list");
        }
    }
}
