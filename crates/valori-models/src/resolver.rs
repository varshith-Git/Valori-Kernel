// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`Resolver`] — M3.
//!
//! Answers the question "which installed model should I use?" without the
//! caller having to know any model ID.
//!
//! # Example
//!
//! ```no_run
//! use valori_models::resolver::Resolver;
//! use valori_models::types::ModelTask;
//!
//! # let manifests = vec![];
//! let resolver = Resolver::new(&manifests);
//! let model = resolver.resolve(ModelTask::Embedding, Some(768)).unwrap();
//! println!("using {}", model.id);
//! ```

use crate::error::{ModelError, ModelResult};
use crate::manifest::ModelManifest;
use crate::types::{ManifestStatus, ModelTask};

/// Selects the best installed model for a given task and optional dimension.
///
/// Stateless: borrow the manifest slice, call `resolve`, done. No locks,
/// no async. The `ModelManager` owns the manifests; this is a pure query.
pub struct Resolver<'a> {
    manifests: &'a [ModelManifest],
}

impl<'a> Resolver<'a> {
    pub fn new(manifests: &'a [ModelManifest]) -> Self {
        Self { manifests }
    }

    /// Find the best installed model for `task`.
    ///
    /// Selection priority (highest first):
    /// 1. Installed, correct task, exact dimension match (if `dimensions` given).
    /// 2. Installed, correct task, any dimension (if `dimensions` is `None`).
    ///
    /// Returns `ModelError::NotFound` when no suitable model is installed.
    pub fn resolve(
        &self,
        task: ModelTask,
        dimensions: Option<usize>,
    ) -> ModelResult<&ModelManifest> {
        let installed: Vec<&ModelManifest> = self
            .manifests
            .iter()
            .filter(|m| m.status == ManifestStatus::Installed && m.task == task)
            .collect();

        if installed.is_empty() {
            return Err(ModelError::NotFound(format!(
                "no installed model for task '{task}'"
            )));
        }

        if let Some(dim) = dimensions {
            // Exact dimension match.
            if let Some(m) = installed.iter().copied().find(|m| m.dimensions == dim) {
                return Ok(m);
            }
            return Err(ModelError::NotFound(format!(
                "no installed {task} model with {dim} dimensions \
                 (installed dims: {})",
                installed
                    .iter()
                    .map(|m| m.dimensions.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }

        // No dimension requirement — return the first (insertion-order priority).
        Ok(installed[0])
    }

    /// All installed models that can produce embeddings compatible with `dim`.
    pub fn compatible_embedding_models(&self, dim: usize) -> Vec<&ModelManifest> {
        self.manifests
            .iter()
            .filter(|m| {
                m.status == ManifestStatus::Installed
                    && m.task == ModelTask::Embedding
                    && m.dimensions == dim
            })
            .collect()
    }

    /// Return the best installed embedding model for a collection.
    ///
    /// This is the one-liner that `ModelManager::resolve_for_collection` calls:
    /// given the collection's vector dimension, find a ready model.
    pub fn resolve_for_embedding(&self, dim: usize) -> ModelResult<&ModelManifest> {
        self.resolve(ModelTask::Embedding, Some(dim))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ModelFormat, ProviderKind};

    fn installed(id: &str, task: ModelTask, dims: usize) -> ModelManifest {
        ModelManifest {
            id: id.into(),
            name: id.into(),
            version: None,
            provider: ProviderKind::OpenAI,
            family: None,
            task,
            dimensions: dims,
            quantization: None,
            format: ModelFormat::Remote,
            sha256: None,
            size_bytes: 0,
            installed_at: Some(1_000_000),
            path: None,
            status: ManifestStatus::Installed,
            min_ram_mb: 0,
            license: None,
            homepage: None,
            download_url: None,
        }
    }

    fn available(id: &str, task: ModelTask, dims: usize) -> ModelManifest {
        let mut m = installed(id, task, dims);
        m.status = ManifestStatus::Available;
        m.installed_at = None;
        m
    }

    #[test]
    fn resolves_by_task_no_dim() {
        let m = installed("openai/ada", ModelTask::Embedding, 1536);
        let r = Resolver::new(std::slice::from_ref(&m));
        assert_eq!(
            r.resolve(ModelTask::Embedding, None).unwrap().id,
            "openai/ada"
        );
    }

    #[test]
    fn resolves_exact_dim() {
        let manifests = vec![
            installed("m/a", ModelTask::Embedding, 1536),
            installed("m/b", ModelTask::Embedding, 768),
        ];
        let r = Resolver::new(&manifests);
        assert_eq!(
            r.resolve(ModelTask::Embedding, Some(768)).unwrap().id,
            "m/b"
        );
    }

    #[test]
    fn wrong_dim_returns_not_found() {
        let m = installed("m/a", ModelTask::Embedding, 1536);
        let r = Resolver::new(std::slice::from_ref(&m));
        let err = r.resolve(ModelTask::Embedding, Some(384)).unwrap_err();
        assert!(err.to_string().contains("384"));
    }

    #[test]
    fn available_models_excluded() {
        let manifests = vec![available("m/available", ModelTask::Embedding, 1536)];
        let r = Resolver::new(&manifests);
        assert!(r.resolve(ModelTask::Embedding, None).is_err());
    }

    #[test]
    fn no_models_for_task_returns_not_found() {
        let m = installed("m/a", ModelTask::Embedding, 1536);
        let r = Resolver::new(std::slice::from_ref(&m));
        let err = r.resolve(ModelTask::Generation, None).unwrap_err();
        assert!(err.to_string().contains("generation"));
    }

    #[test]
    fn compatible_embedding_models_filters_correctly() {
        let manifests = vec![
            installed("m/a", ModelTask::Embedding, 768),
            installed("m/b", ModelTask::Embedding, 1536),
            installed("m/c", ModelTask::Embedding, 768),
        ];
        let r = Resolver::new(&manifests);
        let compat = r.compatible_embedding_models(768);
        assert_eq!(compat.len(), 2);
        assert!(compat.iter().all(|m| m.dimensions == 768));
    }

    #[test]
    fn resolve_for_embedding_convenience() {
        let m = installed("bge/small", ModelTask::Embedding, 384);
        let r = Resolver::new(std::slice::from_ref(&m));
        assert_eq!(r.resolve_for_embedding(384).unwrap().id, "bge/small");
    }
}
