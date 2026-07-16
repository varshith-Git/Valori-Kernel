// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`DocumentValidator`] — structural checks on a document before chunking.
//!
//! The validator is a standalone gate, not yet wired into `IngestPipeline`.
//! Callers invoke it explicitly; a future pipeline phase will add it as an
//! optional stage between `Reader` and `Chunker`.

use crate::document::Document;

/// Reasons a document can be rejected before chunking.
#[derive(Debug, PartialEq)]
pub enum ValidationError {
    /// The document produced no extractable text.
    Empty,
    /// Content length exceeds the configured byte ceiling.
    TooLarge { size: usize, limit: usize },
    /// The document has more pages than the configured limit.
    TooManyPages { count: usize, limit: usize },
    /// Content contains bytes that are not valid UTF-8.
    MalformedUtf8,
    /// The PDF is password-protected and cannot be read.
    ProtectedDocument,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::Empty =>
                write!(f, "document is empty"),
            ValidationError::TooLarge { size, limit } =>
                write!(f, "document is {size} bytes, exceeds limit of {limit}"),
            ValidationError::TooManyPages { count, limit } =>
                write!(f, "document has {count} pages, exceeds limit of {limit}"),
            ValidationError::MalformedUtf8 =>
                write!(f, "document contains invalid UTF-8"),
            ValidationError::ProtectedDocument =>
                write!(f, "document is password-protected"),
        }
    }
}

impl std::error::Error for ValidationError {}

/// Configurable document validator.
///
/// All limits have sensible defaults; callers override what they need.
pub struct DocumentValidator {
    /// Maximum content length in bytes. Default: 50 MiB.
    pub max_bytes: usize,
    /// Maximum page count for paginated formats. `None` = no limit.
    pub max_pages: Option<usize>,
}

impl Default for DocumentValidator {
    fn default() -> Self {
        Self { max_bytes: 50 * 1024 * 1024, max_pages: None }
    }
}

impl DocumentValidator {
    pub fn new() -> Self { Self::default() }

    pub fn with_max_bytes(mut self, n: usize) -> Self { self.max_bytes = n; self }
    pub fn with_max_pages(mut self, n: usize) -> Self { self.max_pages = Some(n); self }

    /// Run all checks against `doc`. Returns `Ok(())` or the first failing check.
    pub fn validate(&self, doc: &Document) -> Result<(), ValidationError> {
        // 1. Empty content
        if doc.content.trim().is_empty() {
            return Err(ValidationError::Empty);
        }

        // 2. Size ceiling
        if doc.content.len() > self.max_bytes {
            return Err(ValidationError::TooLarge {
                size: doc.content.len(),
                limit: self.max_bytes,
            });
        }

        // 3. UTF-8 validity — content is already `String` so this can't fail
        //    at the Doc level; guard is for raw-bytes callers who build the
        //    content string themselves.
        if std::str::from_utf8(doc.content.as_bytes()).is_err() {
            return Err(ValidationError::MalformedUtf8);
        }

        // 4. Page limit (optional)
        if let (Some(limit), Some(count)) = (self.max_pages, doc.metadata.page_count) {
            if count > limit {
                return Err(ValidationError::TooManyPages { count, limit });
            }
        }

        // 5. Protected PDF heuristic — check for the "/Encrypt" marker in
        //    the raw mime type; real detection happens at extraction time.
        //    If the reader surfaced this, it would have set mime_type to
        //    "application/pdf+protected". For now this is a placeholder.
        if doc.mime_type == "application/pdf+protected" {
            return Err(ValidationError::ProtectedDocument);
        }

        Ok(())
    }
}

/// Validate raw bytes for UTF-8 before constructing a Document.
/// Useful when a Reader has bytes before decoding them to String.
pub fn validate_utf8(bytes: &[u8]) -> Result<(), ValidationError> {
    std::str::from_utf8(bytes)
        .map(|_| ())
        .map_err(|_| ValidationError::MalformedUtf8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    fn doc(content: &str) -> Document {
        Document::from_text(content, Some("test.txt"))
    }

    #[test]
    fn empty_content_rejected() {
        let v = DocumentValidator::new();
        assert_eq!(v.validate(&doc("")).unwrap_err(), ValidationError::Empty);
        assert_eq!(v.validate(&doc("   ")).unwrap_err(), ValidationError::Empty);
    }

    #[test]
    fn valid_document_passes() {
        let v = DocumentValidator::new();
        assert!(v.validate(&doc("Hello world.")).is_ok());
    }

    #[test]
    fn oversized_content_rejected() {
        let v = DocumentValidator::new().with_max_bytes(10);
        let err = v.validate(&doc("This is longer than ten bytes")).unwrap_err();
        assert!(matches!(err, ValidationError::TooLarge { .. }));
    }

    #[test]
    fn content_within_limit_passes() {
        let v = DocumentValidator::new().with_max_bytes(100);
        assert!(v.validate(&doc("short")).is_ok());
    }

    #[test]
    fn page_limit_enforced() {
        let v = DocumentValidator::new().with_max_pages(5);
        let mut d = doc("some text");
        d.metadata.page_count = Some(10);
        let err = v.validate(&d).unwrap_err();
        assert!(matches!(err, ValidationError::TooManyPages { count: 10, limit: 5 }));
    }

    #[test]
    fn page_limit_not_enforced_when_none_configured() {
        let v = DocumentValidator::new(); // no max_pages
        let mut d = doc("some text");
        d.metadata.page_count = Some(9999);
        assert!(v.validate(&d).is_ok());
    }

    #[test]
    fn validate_utf8_rejects_invalid_bytes() {
        let bad: &[u8] = &[0xFF, 0xFE];
        assert_eq!(validate_utf8(bad).unwrap_err(), ValidationError::MalformedUtf8);
    }

    #[test]
    fn validate_utf8_accepts_valid_bytes() {
        assert!(validate_utf8(b"hello").is_ok());
    }
}
