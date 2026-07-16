// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`ReaderRegistry`] — resolves a [`Reader`] from a file path or extension.
//!
//! All format knowledge lives here. The pipeline, node, and any other caller
//! ask the registry; they never contain extension-matching logic themselves.

use std::sync::Arc;

use crate::document::IngestError;
use crate::reader::Reader;
use crate::reader::TextReader;
use crate::readers::{DocxReader, HtmlReader, MarkdownReader, PdfReader};

/// Maps file extensions to [`Reader`] implementations.
///
/// Readers are `Arc`-wrapped so they are cheap to clone and can be held across
/// async boundaries without boxing a new instance per call.
pub struct ReaderRegistry;

impl ReaderRegistry {
    /// Return the appropriate reader for the given file extension.
    ///
    /// `ext` should be without the leading dot (`"md"`, not `".md"`).
    /// Comparison is case-insensitive.
    pub fn reader_for_extension(ext: &str) -> Result<Arc<dyn Reader>, IngestError> {
        match ext.to_ascii_lowercase().as_str() {
            "txt" | "text" => Ok(Arc::new(TextReader)),
            "md" | "markdown" => Ok(Arc::new(MarkdownReader)),
            "html" | "htm" => Ok(Arc::new(HtmlReader)),
            "pdf" => Ok(Arc::new(PdfReader)),
            "docx" => Ok(Arc::new(DocxReader)),
            other => Err(IngestError::Reader(format!(
                "no reader registered for extension '.{other}'"
            ))),
        }
    }

    /// Return the appropriate reader for a file path by inspecting its extension.
    ///
    /// Accepts any type that converts to a `std::path::Path` — `&str`, `&Path`,
    /// `PathBuf`, etc.
    pub fn reader_for_path(
        path: impl AsRef<std::path::Path>,
    ) -> Result<Arc<dyn Reader>, IngestError> {
        let path = path.as_ref();
        let ext = path.extension().and_then(|e| e.to_str()).ok_or_else(|| {
            IngestError::Reader(format!(
                "cannot determine extension for '{}'",
                path.display()
            ))
        })?;
        Self::reader_for_extension(ext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── reader_for_extension ──────────────────────────────────────────────────

    #[test]
    fn txt_resolves() {
        ReaderRegistry::reader_for_extension("txt").unwrap();
    }

    #[test]
    fn text_alias_resolves() {
        ReaderRegistry::reader_for_extension("text").unwrap();
    }

    #[test]
    fn md_resolves() {
        ReaderRegistry::reader_for_extension("md").unwrap();
    }

    #[test]
    fn markdown_alias_resolves() {
        ReaderRegistry::reader_for_extension("markdown").unwrap();
    }

    #[test]
    fn html_resolves() {
        ReaderRegistry::reader_for_extension("html").unwrap();
    }

    #[test]
    fn htm_alias_resolves() {
        ReaderRegistry::reader_for_extension("htm").unwrap();
    }

    #[test]
    fn pdf_resolves() {
        ReaderRegistry::reader_for_extension("pdf").unwrap();
    }

    #[test]
    fn docx_resolves() {
        ReaderRegistry::reader_for_extension("docx").unwrap();
    }

    #[test]
    fn case_insensitive() {
        ReaderRegistry::reader_for_extension("MD").unwrap();
        ReaderRegistry::reader_for_extension("PDF").unwrap();
        ReaderRegistry::reader_for_extension("DOCX").unwrap();
        ReaderRegistry::reader_for_extension("HTML").unwrap();
    }

    #[test]
    fn unknown_extension_is_reader_error() {
        match ReaderRegistry::reader_for_extension("xyz") {
            Err(e) => assert!(e.to_string().contains(".xyz")),
            Ok(_) => panic!("expected error for unknown extension"),
        }
    }

    #[test]
    fn empty_extension_is_reader_error() {
        assert!(ReaderRegistry::reader_for_extension("").is_err());
    }

    // ── reader_for_path ───────────────────────────────────────────────────────

    #[test]
    fn path_dispatches_by_extension() {
        ReaderRegistry::reader_for_path(Path::new("report.pdf")).unwrap();
        ReaderRegistry::reader_for_path(Path::new("notes.md")).unwrap();
        ReaderRegistry::reader_for_path(Path::new("index.html")).unwrap();
        ReaderRegistry::reader_for_path(Path::new("doc.docx")).unwrap();
        ReaderRegistry::reader_for_path(Path::new("log.txt")).unwrap();
    }

    #[test]
    fn path_with_directory_component() {
        ReaderRegistry::reader_for_path(Path::new("/some/dir/notes.markdown")).unwrap();
    }

    #[test]
    fn path_without_extension_is_reader_error() {
        assert!(ReaderRegistry::reader_for_path(Path::new("Makefile")).is_err());
    }

    #[test]
    fn path_unknown_extension_is_reader_error() {
        assert!(ReaderRegistry::reader_for_path(Path::new("archive.tar.gz")).is_err());
    }
}
