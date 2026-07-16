// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Built-in model catalog — M7.
//!
//! Every entry here is available to the desktop without an internet connection.
//! Remote-service models (OpenAI, Ollama, Voyage) have no `download_url`; they
//! are registered instantly. Local models (ONNX, GGUF) have a `download_url`
//! and start life as `Available` until explicitly installed.

use crate::manifest::ModelManifest;
use crate::types::{ManifestStatus, ModelFormat, ModelTask, ProviderKind};

/// Return the curated built-in model catalog.
///
/// All entries start as `Available`. `ModelManager::install()` transitions
/// them to `Installed` and persists them in `JsonModelStore`.
pub fn built_in() -> Vec<ModelManifest> {
    vec![
        // ── OpenAI remote ─────────────────────────────────────────────────────
        remote(
            "openai/text-embedding-3-small",
            "OpenAI text-embedding-3-small",
            ProviderKind::OpenAI,
            "text-embedding-3",
            1536,
            Some("MIT"),
            Some("https://platform.openai.com/docs/guides/embeddings"),
        ),
        remote(
            "openai/text-embedding-3-large",
            "OpenAI text-embedding-3-large",
            ProviderKind::OpenAI,
            "text-embedding-3",
            3072,
            Some("MIT"),
            Some("https://platform.openai.com/docs/guides/embeddings"),
        ),
        remote(
            "openai/text-embedding-ada-002",
            "OpenAI text-embedding-ada-002 (legacy)",
            ProviderKind::OpenAI,
            "ada",
            1536,
            Some("MIT"),
            Some("https://platform.openai.com/docs/guides/embeddings"),
        ),
        // ── Ollama remote ─────────────────────────────────────────────────────
        remote(
            "ollama/nomic-embed-text",
            "Nomic Embed Text (via Ollama)",
            ProviderKind::Ollama,
            "nomic",
            768,
            Some("Apache-2.0"),
            Some("https://ollama.com/library/nomic-embed-text"),
        ),
        remote(
            "ollama/mxbai-embed-large",
            "MixedBread AI mxbai-embed-large (via Ollama)",
            ProviderKind::Ollama,
            "mxbai",
            1024,
            Some("Apache-2.0"),
            Some("https://ollama.com/library/mxbai-embed-large"),
        ),
        remote(
            "ollama/bge-m3",
            "BAAI bge-m3 multilingual (via Ollama)",
            ProviderKind::Ollama,
            "bge",
            1024,
            Some("MIT"),
            Some("https://ollama.com/library/bge-m3"),
        ),
        // ── Voyage remote ─────────────────────────────────────────────────────
        remote(
            "voyage/voyage-3",
            "Voyage AI voyage-3",
            ProviderKind::Voyage,
            "voyage",
            1024,
            None,
            Some("https://www.voyageai.com/"),
        ),
        remote(
            "voyage/voyage-3-lite",
            "Voyage AI voyage-3-lite",
            ProviderKind::Voyage,
            "voyage",
            512,
            None,
            Some("https://www.voyageai.com/"),
        ),
        // ── Local ONNX models ─────────────────────────────────────────────────
        // SHA-256 and size are populated once local inference ships (E1-full).
        local_onnx(
            "baai/bge-small-en-v1.5",
            "BAAI bge-small-en-v1.5 (ONNX, 384-dim)",
            "bge",
            384,
            "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/onnx/model.onnx",
            33_000_000,
            Some("MIT"),
            Some("https://huggingface.co/BAAI/bge-small-en-v1.5"),
            256,
        ),
        local_onnx(
            "baai/bge-base-en-v1.5",
            "BAAI bge-base-en-v1.5 (ONNX, 768-dim)",
            "bge",
            768,
            "https://huggingface.co/BAAI/bge-base-en-v1.5/resolve/main/onnx/model.onnx",
            109_000_000,
            Some("MIT"),
            Some("https://huggingface.co/BAAI/bge-base-en-v1.5"),
            512,
        ),
        local_onnx(
            "baai/bge-large-en-v1.5",
            "BAAI bge-large-en-v1.5 (ONNX, 1024-dim)",
            "bge",
            1024,
            "https://huggingface.co/BAAI/bge-large-en-v1.5/resolve/main/onnx/model.onnx",
            335_000_000,
            Some("MIT"),
            Some("https://huggingface.co/BAAI/bge-large-en-v1.5"),
            1024,
        ),
    ]
}

// ── Constructors ──────────────────────────────────────────────────────────────

fn remote(
    id: &str,
    name: &str,
    provider: ProviderKind,
    family: &str,
    dimensions: usize,
    license: Option<&str>,
    homepage: Option<&str>,
) -> ModelManifest {
    ModelManifest {
        id: id.into(),
        name: name.into(),
        version: None,
        provider,
        family: Some(family.into()),
        task: ModelTask::Embedding,
        dimensions,
        quantization: None,
        format: ModelFormat::Remote,
        sha256: None,
        size_bytes: 0,
        installed_at: None,
        path: None,
        status: ManifestStatus::Available,
        min_ram_mb: 0,
        license: license.map(Into::into),
        homepage: homepage.map(Into::into),
        download_url: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn local_onnx(
    id: &str,
    name: &str,
    family: &str,
    dimensions: usize,
    download_url: &str,
    size_bytes: u64,
    license: Option<&str>,
    homepage: Option<&str>,
    min_ram_mb: u64,
) -> ModelManifest {
    ModelManifest {
        id: id.into(),
        name: name.into(),
        version: Some("1.5".into()),
        provider: ProviderKind::Local,
        family: Some(family.into()),
        task: ModelTask::Embedding,
        dimensions,
        quantization: Some("fp32".into()),
        format: ModelFormat::Onnx,
        sha256: None,
        size_bytes,
        installed_at: None,
        path: None,
        status: ManifestStatus::Available,
        min_ram_mb,
        license: license.map(Into::into),
        homepage: homepage.map(Into::into),
        download_url: Some(download_url.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_non_empty() {
        let catalog = built_in();
        assert!(!catalog.is_empty());
    }

    #[test]
    fn all_entries_available() {
        for m in built_in() {
            assert!(
                m.status.is_available(),
                "built-in entry '{}' should start as Available",
                m.id
            );
        }
    }

    #[test]
    fn no_duplicate_ids() {
        let catalog = built_in();
        let mut seen = std::collections::HashSet::new();
        for m in &catalog {
            assert!(seen.insert(m.id.clone()), "duplicate id '{}'", m.id);
        }
    }

    #[test]
    fn remote_models_have_no_download_url() {
        for m in built_in()
            .iter()
            .filter(|m| m.format == ModelFormat::Remote)
        {
            assert!(
                m.download_url.is_none(),
                "'{}' is Remote but has download_url",
                m.id
            );
        }
    }

    #[test]
    fn local_models_have_download_url() {
        for m in built_in()
            .iter()
            .filter(|m| m.format != ModelFormat::Remote)
        {
            assert!(
                m.download_url.is_some(),
                "'{}' is local but missing download_url",
                m.id
            );
        }
    }

    #[test]
    fn embedding_models_have_nonzero_dims() {
        for m in built_in().iter().filter(|m| m.task == ModelTask::Embedding) {
            assert!(m.dimensions > 0, "'{}' has dimensions=0", m.id);
        }
    }
}
