// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`DocumentSource`] — typed origin of a document entering the pipeline.
//!
//! Replaces the bare `&str` / `String` that readers currently receive as
//! `input`. Callers construct a `DocumentSource`, pipeline stages read from it.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Where a document comes from.
///
/// Each variant maps to a distinct I/O strategy. The pipeline asks the
/// `DocumentSource` for bytes; the reading strategy is encapsulated here,
/// not spread across handlers, CLI, and daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocumentSource {
    /// A local file on the node's filesystem.
    File(PathBuf),
    /// An HTTP/HTTPS URL. The fetching strategy (streaming, auth) is the
    /// caller's responsibility; the source carries the URL for provenance.
    Url(String),
    /// Bytes already in memory — e.g. a multipart upload or a test fixture.
    Memory,
    /// A single file inside a GitHub repository (API-fetched).
    GitHub {
        repo: String,
        branch: String,
        file: String,
    },
    /// An object in an S3-compatible bucket.
    S3 { bucket: String, key: String },
}

impl DocumentSource {
    /// Human-readable provenance string — used as `Document.source`.
    pub fn as_source_str(&self) -> String {
        match self {
            DocumentSource::File(p) => p.display().to_string(),
            DocumentSource::Url(u) => u.clone(),
            DocumentSource::Memory => "memory".to_string(),
            DocumentSource::GitHub { repo, branch, file } => {
                format!("github:{repo}@{branch}/{file}")
            }
            DocumentSource::S3 { bucket, key } => {
                format!("s3://{bucket}/{key}")
            }
        }
    }

    /// True for sources that have a local path the OS can open.
    pub fn is_local_file(&self) -> bool {
        matches!(self, DocumentSource::File(_))
    }
}

impl From<PathBuf> for DocumentSource {
    fn from(p: PathBuf) -> Self {
        DocumentSource::File(p)
    }
}

impl From<&str> for DocumentSource {
    fn from(s: &str) -> Self {
        if s.starts_with("http://") || s.starts_with("https://") {
            DocumentSource::Url(s.to_string())
        } else {
            DocumentSource::File(PathBuf::from(s))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_source_str() {
        let s = DocumentSource::File(PathBuf::from("/docs/report.pdf"));
        assert_eq!(s.as_source_str(), "/docs/report.pdf");
        assert!(s.is_local_file());
    }

    #[test]
    fn url_source_str() {
        let s = DocumentSource::Url("https://example.com/doc.html".into());
        assert_eq!(s.as_source_str(), "https://example.com/doc.html");
        assert!(!s.is_local_file());
    }

    #[test]
    fn memory_source_str() {
        assert_eq!(DocumentSource::Memory.as_source_str(), "memory");
    }

    #[test]
    fn github_source_str() {
        let s = DocumentSource::GitHub {
            repo: "acme/repo".into(),
            branch: "main".into(),
            file: "README.md".into(),
        };
        assert_eq!(s.as_source_str(), "github:acme/repo@main/README.md");
    }

    #[test]
    fn s3_source_str() {
        let s = DocumentSource::S3 {
            bucket: "my-bucket".into(),
            key: "docs/file.pdf".into(),
        };
        assert_eq!(s.as_source_str(), "s3://my-bucket/docs/file.pdf");
    }

    #[test]
    fn from_str_http_becomes_url() {
        let s = DocumentSource::from("https://example.com/page");
        assert!(matches!(s, DocumentSource::Url(_)));
    }

    #[test]
    fn from_str_path_becomes_file() {
        let s = DocumentSource::from("/tmp/doc.pdf");
        assert!(matches!(s, DocumentSource::File(_)));
    }

    #[test]
    fn serialise_round_trip() {
        let original = DocumentSource::GitHub {
            repo: "a/b".into(),
            branch: "main".into(),
            file: "x.md".into(),
        };
        let json = serde_json::to_string(&original).unwrap();
        let back: DocumentSource = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
    }
}
