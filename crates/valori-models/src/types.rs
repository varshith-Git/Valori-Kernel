// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Core enums shared across the model management subsystem.

use serde::{Deserialize, Serialize};

// ── ModelTask ─────────────────────────────────────────────────────────────────

/// What a model is capable of doing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTask {
    Embedding,
    Generation,
    Reranker,
    Vision,
    Speech,
}

impl std::fmt::Display for ModelTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelTask::Embedding => write!(f, "embedding"),
            ModelTask::Generation => write!(f, "generation"),
            ModelTask::Reranker => write!(f, "reranker"),
            ModelTask::Vision => write!(f, "vision"),
            ModelTask::Speech => write!(f, "speech"),
        }
    }
}

// ── ModelFormat ───────────────────────────────────────────────────────────────

/// How the model weights are stored (or that they are remote).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFormat {
    /// ONNX weights on disk.
    Onnx,
    /// GGUF weights on disk (llama.cpp family).
    Gguf,
    /// Safetensors weights on disk (HuggingFace).
    Safetensors,
    /// No local file; the provider serves the model over its own API.
    Remote,
}

// ── ProviderKind ──────────────────────────────────────────────────────────────

/// Which provider serves this model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind {
    #[serde(rename = "openai")]
    OpenAI,
    #[serde(rename = "ollama")]
    Ollama,
    #[serde(rename = "voyage")]
    Voyage,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "azure_openai")]
    AzureOpenAI,
    /// Any OpenAI-compatible API not otherwise listed.
    #[serde(rename = "custom")]
    Custom,
    /// Local inference (ONNX / GGUF) — no external API.
    #[serde(rename = "local")]
    Local,
    /// Deterministic zeros — tests only.
    #[serde(rename = "dummy")]
    Dummy,
}

impl ProviderKind {
    /// Canonical lowercase tag used in registry entries and config parsing.
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::OpenAI => "openai",
            ProviderKind::Ollama => "ollama",
            ProviderKind::Voyage => "voyage",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::AzureOpenAI => "azure_openai",
            ProviderKind::Custom => "custom",
            ProviderKind::Local => "local",
            ProviderKind::Dummy => "dummy",
        }
    }

    /// Parse from a config string; returns `None` for unrecognised values.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "openai" => Some(ProviderKind::OpenAI),
            "ollama" => Some(ProviderKind::Ollama),
            "voyage" => Some(ProviderKind::Voyage),
            "anthropic" => Some(ProviderKind::Anthropic),
            "azure_openai" => Some(ProviderKind::AzureOpenAI),
            "custom" => Some(ProviderKind::Custom),
            "local" => Some(ProviderKind::Local),
            "dummy" => Some(ProviderKind::Dummy),
            _ => None,
        }
    }
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ── ManifestStatus ────────────────────────────────────────────────────────────

/// Lifecycle state of a model entry.
///
/// `Available` and `Installed` are the two stable states; the rest are
/// transient and not persisted to disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ManifestStatus {
    /// Known to the registry but not installed.
    Available,
    /// Queued for download (not yet started).
    Queued,
    /// Actively downloading.
    Downloading {
        progress_bytes: u64,
        total_bytes: u64,
    },
    /// Download paused mid-stream.
    Paused { progress_bytes: u64 },
    /// Download complete; verifying SHA-256.
    Verifying,
    /// Fully installed and ready.
    Installed,
    /// Download or verification failed.
    Failed { reason: String },
}

impl ManifestStatus {
    pub fn is_installed(&self) -> bool {
        matches!(self, ManifestStatus::Installed)
    }
    pub fn is_available(&self) -> bool {
        matches!(self, ManifestStatus::Available)
    }
    pub fn is_in_progress(&self) -> bool {
        matches!(
            self,
            ManifestStatus::Queued
                | ManifestStatus::Downloading { .. }
                | ManifestStatus::Paused { .. }
                | ManifestStatus::Verifying
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_kind_roundtrip_str() {
        let kinds = [
            ProviderKind::OpenAI,
            ProviderKind::Ollama,
            ProviderKind::Voyage,
            ProviderKind::Custom,
            ProviderKind::Local,
            ProviderKind::Dummy,
        ];
        for k in &kinds {
            let s = k.as_str();
            let back = ProviderKind::from_str(s).unwrap();
            assert_eq!(k, &back, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn provider_kind_unknown_returns_none() {
        assert!(ProviderKind::from_str("unknown_provider").is_none());
    }

    #[test]
    fn manifest_status_helpers() {
        assert!(ManifestStatus::Installed.is_installed());
        assert!(!ManifestStatus::Available.is_installed());
        assert!(ManifestStatus::Available.is_available());
        assert!(ManifestStatus::Queued.is_in_progress());
        assert!(ManifestStatus::Downloading {
            progress_bytes: 0,
            total_bytes: 100
        }
        .is_in_progress());
        assert!(!ManifestStatus::Installed.is_in_progress());
    }

    #[test]
    fn model_task_display() {
        assert_eq!(ModelTask::Embedding.to_string(), "embedding");
        assert_eq!(ModelTask::Reranker.to_string(), "reranker");
    }

    #[test]
    fn enums_serde_roundtrip() {
        let task: ModelTask = serde_json::from_str(r#""embedding""#).unwrap();
        assert_eq!(task, ModelTask::Embedding);
        assert_eq!(
            serde_json::to_string(&ModelTask::Generation).unwrap(),
            r#""generation""#
        );

        let fmt: ModelFormat = serde_json::from_str(r#""remote""#).unwrap();
        assert_eq!(fmt, ModelFormat::Remote);
    }
}
