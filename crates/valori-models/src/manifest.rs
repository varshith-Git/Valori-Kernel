// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`ModelManifest`] — the single source of truth for a model entry.
//!
//! One manifest describes a model whether it is merely known (Available),
//! being downloaded (Downloading), or fully on disk (Installed).
//!
//! Persisted manifests always have `status: Installed`; transient states
//! are only in memory.

use serde::{Deserialize, Serialize};

use crate::types::{ManifestStatus, ModelFormat, ModelTask, ProviderKind};

/// Complete description of one model.
///
/// Replaces the old `InstalledModel` + `ModelSpec` split — a single type
/// covers both "available in registry" and "installed on disk".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelManifest {
    /// Stable identifier: `provider/model-name`, e.g. `"openai/text-embedding-3-small"`.
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Semantic version string if known (`"1.5"`, `"v3"`, …). Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Which provider serves this model.
    pub provider: ProviderKind,
    /// Model family for grouping in UI: `"bge"`, `"text-embedding-3"`, `"nomic"`, …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Primary capability.
    pub task: ModelTask,
    /// Output vector dimension (0 for non-embedding models).
    pub dimensions: usize,
    /// Quantization tag if applicable: `"Q4_K_M"`, `"fp32"`, `"int8"`, …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantization: Option<String>,
    /// How the weights are stored.
    pub format: ModelFormat,
    /// SHA-256 hex of the local file. `None` for remote-only models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// File size in bytes (0 for remote models).
    pub size_bytes: u64,
    /// Unix seconds when the model was installed. `None` if not yet installed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_at: Option<u64>,
    /// Absolute path to the local model file. `None` for remote models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Lifecycle state. Always `Installed` for persisted manifests.
    pub status: ManifestStatus,
    /// Minimum recommended RAM in MiB (0 = unknown).
    #[serde(default)]
    pub min_ram_mb: u64,
    /// SPDX license identifier: `"Apache-2.0"`, `"MIT"`, `"CC-BY-4.0"`, …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// URL of the model homepage or HuggingFace card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Download URL for local models. `None` for remote-service models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
}

impl ModelManifest {
    /// True when this model is usable for inference right now.
    pub fn is_ready(&self) -> bool {
        self.status.is_installed()
    }

    /// True when the model can produce embeddings.
    pub fn supports_embedding(&self) -> bool {
        self.task == ModelTask::Embedding && self.dimensions > 0
    }

    /// True when this manifest represents a local file (not a remote API).
    pub fn is_local(&self) -> bool {
        !matches!(self.format, ModelFormat::Remote)
    }

    /// Check whether `dim` is compatible with this model.
    pub fn is_compatible_with_dim(&self, dim: usize) -> bool {
        self.dimensions == dim
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manifest(task: ModelTask, dims: usize, status: ManifestStatus) -> ModelManifest {
        ModelManifest {
            id: "test/model".into(),
            name: "Test Model".into(),
            version: None,
            provider: ProviderKind::OpenAI,
            family: Some("test".into()),
            task,
            dimensions: dims,
            quantization: None,
            format: ModelFormat::Remote,
            sha256: None,
            size_bytes: 0,
            installed_at: None,
            path: None,
            status,
            min_ram_mb: 0,
            license: None,
            homepage: None,
            download_url: None,
        }
    }

    #[test]
    fn is_ready_only_when_installed() {
        let m = make_manifest(ModelTask::Embedding, 1536, ManifestStatus::Installed);
        assert!(m.is_ready());
        let m2 = make_manifest(ModelTask::Embedding, 1536, ManifestStatus::Available);
        assert!(!m2.is_ready());
    }

    #[test]
    fn supports_embedding_checks_task_and_dims() {
        let m = make_manifest(ModelTask::Embedding, 1536, ManifestStatus::Installed);
        assert!(m.supports_embedding());
        let m2 = make_manifest(ModelTask::Generation, 0, ManifestStatus::Installed);
        assert!(!m2.supports_embedding());
        let m3 = make_manifest(ModelTask::Embedding, 0, ManifestStatus::Installed);
        assert!(!m3.supports_embedding());
    }

    #[test]
    fn is_local_matches_format() {
        let local = make_manifest(ModelTask::Embedding, 384, ManifestStatus::Installed);
        // Remote format
        assert!(!local.is_local()); // Remote format set above
        let mut onnx = local.clone();
        onnx.format = ModelFormat::Onnx;
        assert!(onnx.is_local());
    }

    #[test]
    fn serde_roundtrip() {
        let m = make_manifest(ModelTask::Embedding, 768, ManifestStatus::Installed);
        let json = serde_json::to_string(&m).unwrap();
        let back: ModelManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn optional_fields_omitted_in_json() {
        let m = make_manifest(ModelTask::Embedding, 1536, ManifestStatus::Available);
        let json = serde_json::to_string(&m).unwrap();
        // Optional None fields should not appear
        assert!(!json.contains(r#""version""#));
        assert!(!json.contains(r#""sha256""#));
        assert!(!json.contains(r#""path""#));
    }
}
