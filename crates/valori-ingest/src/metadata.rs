// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`DocumentMetadata`] — typed provenance for a document entering the pipeline.
//!
//! All extractors and readers populate these fields consistently.
//! Fields that the format cannot supply default to `None`.

use serde::{Deserialize, Serialize};

/// Structured metadata extracted alongside document content.
///
/// `source` and `mime_type` live on [`crate::document::Document`] directly;
/// everything else lives here. All fields are optional except the defaults that
/// every reader can always fill.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DocumentMetadata {
    /// Document title from heading, PDF Info, or DOCX core properties.
    pub title: Option<String>,
    /// Author from PDF Info `/Author` or DOCX `dc:creator`.
    pub author: Option<String>,
    /// BCP-47 language tag detected or declared in the document.
    /// Not auto-detected yet; set by callers that know the language.
    pub language: Option<String>,
    /// ISO-8601 creation timestamp from PDF `/CreationDate` or DOCX `dcterms:created`.
    pub created_at: Option<String>,
    /// ISO-8601 last-modified timestamp from PDF `/ModDate` or DOCX `dcterms:modified`.
    pub modified_at: Option<String>,
    /// Page count for paginated formats (PDF, DOCX).
    pub page_count: Option<usize>,
}

impl DocumentMetadata {
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    pub fn with_page_count(mut self, n: usize) -> Self {
        self.page_count = Some(n);
        self
    }
}

impl From<DocumentMetadata> for serde_json::Value {
    fn from(m: DocumentMetadata) -> Self {
        serde_json::to_value(m).unwrap_or_default()
    }
}
