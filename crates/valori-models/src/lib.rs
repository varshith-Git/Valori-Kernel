// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `valori-models` — the model management subsystem.
//!
//! Answers: what models are installed, where, which provider owns them, are
//! they valid, can they embed, what dimensions do they output, are they
//! compatible with this collection.
//!
//! Does NOT know about: documents, chunks, pipelines, HTTP, desktop UI.
//!
//! # Architecture
//!
//! ```text
//! ModelManager
//!   ├── ModelStore (JsonModelStore)     — persists installed manifests
//!   ├── ProviderRegistry               — builds ModelProvider instances
//!   ├── Resolver                       — selects best model for a task/dim
//!   └── Downloader / Verifier          — download + SHA-256 check
//! ```

pub mod downloader;
pub mod error;
pub mod gc;
pub mod health;
pub mod integrity;
pub mod manifest;
pub mod package_store;
pub mod provider;
pub mod registry;
pub mod resolver;
pub mod storage;
pub mod types;

// Re-export verifier so callers don't need to know the module.
pub mod verifier;

// ── Public surface ────────────────────────────────────────────────────────────

pub use error::{ModelError, ModelResult};

// M1 — Manifest + types
pub use manifest::ModelManifest;
pub use types::{ManifestStatus, ModelFormat, ModelTask, ProviderKind};

// M2 — Provider registry
pub use provider::registry::{ProviderFactory, ProviderRegistry};
pub use provider::{provider_from_config, ModelProvider};

// M3 — Resolver
pub use resolver::Resolver;

// M4 — Downloader
pub use downloader::{DownloadEvent, DownloadJob, DownloadState};

// Storage
pub use storage::{JsonModelStore, ModelStore};

// M5 — Package store
pub use package_store::{InstallLock, PackageManifest, PackageStore};

// M6 — Integrity + GC + health
pub use gc::{GarbageCollector, GcReport, RefCounter, UnreferencedPackage};
pub use health::{system_health, PackageHealth, PackageHealthStatus, SystemHealth};
pub use integrity::{
    repair as repair_package, IntegrityManager, IntegrityReport, IntegrityStatus, RepairAction,
    RepairResult,
};

// ── ModelManager ──────────────────────────────────────────────────────────────

use std::path::{Path, PathBuf};

/// Top-level orchestrator. One instance per data root.
///
/// Create with [`ModelManager::new`], then call [`install`], [`remove`],
/// [`verify`], [`provider_for`], [`resolve`].
pub struct ModelManager {
    pub(crate) store: Box<dyn ModelStore>,
    pub(crate) models_dir: PathBuf,
    pub(crate) catalog: Vec<ModelManifest>, // built-in registry
    pub(crate) providers: ProviderRegistry,
}

impl ModelManager {
    /// Create a new manager rooted at `home`.
    ///
    /// `store` is the persistence backend (use `JsonModelStore::new(home)`).
    /// The built-in registry is loaded automatically.
    pub fn new(home: impl AsRef<Path>, store: Box<dyn ModelStore>) -> ModelResult<Self> {
        let models_dir = home.as_ref().join("models");
        std::fs::create_dir_all(&models_dir)?;
        Ok(Self {
            store,
            models_dir,
            catalog: registry::built_in(),
            providers: ProviderRegistry::with_defaults(),
        })
    }

    // ── Query ──────────────────────────────────────────────────────────────────

    /// All installed manifests.
    pub fn installed(&self) -> Vec<ModelManifest> {
        self.store.list()
    }

    /// All manifests: installed (from store) + available (from built-in catalog
    /// minus already installed). Status reflects real state.
    pub fn all_manifests(&self) -> Vec<ModelManifest> {
        let installed = self.store.list();
        let installed_ids: std::collections::HashSet<_> =
            installed.iter().map(|m| m.id.clone()).collect();

        let mut out = installed;
        for m in &self.catalog {
            if !installed_ids.contains(&m.id) {
                out.push(m.clone());
            }
        }
        out
    }

    /// Total disk usage of all installed local models in bytes.
    pub fn disk_usage_bytes(&self) -> u64 {
        self.store.list().iter().map(|m| m.size_bytes).sum()
    }

    /// Full catalog as JSON (installed + available + disk_bytes). Backward
    /// compat shim for existing `/v1/models` API handlers.
    pub fn catalog_json(&self) -> serde_json::Value {
        let installed = self.store.list();
        let installed_ids: std::collections::HashSet<_> =
            installed.iter().map(|m| m.id.clone()).collect();
        let available: Vec<&ModelManifest> = self
            .catalog
            .iter()
            .filter(|m| !installed_ids.contains(&m.id))
            .collect();
        let disk_bytes = self.disk_usage_bytes();
        serde_json::json!({ "installed": installed, "available": available, "disk_bytes": disk_bytes })
    }

    pub fn count(&self) -> usize {
        self.store.list().len()
    }

    pub fn get(&self, id: &str) -> ModelResult<ModelManifest> {
        self.store
            .get(id)
            .ok_or_else(|| ModelError::NotFound(format!("model '{id}'")))
    }

    /// Look up a manifest in the built-in catalog (may not be installed).
    pub fn catalog_entry(&self, id: &str) -> ModelResult<&ModelManifest> {
        self.catalog
            .iter()
            .find(|m| m.id == id)
            .ok_or_else(|| ModelError::NotFound(format!("'{id}' not in built-in catalog")))
    }

    // ── Resolver (M3) ──────────────────────────────────────────────────────────

    /// Find the best installed model for `task`, optionally requiring `dimensions`.
    pub fn resolve(
        &self,
        task: ModelTask,
        dimensions: Option<usize>,
    ) -> ModelResult<ModelManifest> {
        let installed = self.store.list();
        let resolver = Resolver::new(&installed);
        resolver.resolve(task, dimensions).cloned()
    }

    /// Find the best installed embedding model compatible with `dim`.
    pub fn resolve_for_collection(&self, dim: usize) -> ModelResult<ModelManifest> {
        self.resolve(ModelTask::Embedding, Some(dim))
    }

    // ── Provider (M2) ──────────────────────────────────────────────────────────

    /// Build a live `ModelProvider` for an installed manifest.
    pub fn provider_for(&self, id: &str) -> ModelResult<Box<dyn ModelProvider>> {
        let manifest = self.get(id)?;
        self.providers.build_from_manifest(&manifest)
    }

    /// Build a provider from explicit config params (used by env-var code paths).
    pub fn provider_from_config(
        &self,
        kind: &str,
        model: &str,
        base_url: Option<&str>,
        api_key: Option<&str>,
        dim: usize,
    ) -> ModelResult<Box<dyn ModelProvider>> {
        self.providers.build(kind, model, base_url, api_key, dim)
    }

    // ── Install / Remove ───────────────────────────────────────────────────────

    /// Install a model. Remote-service models register instantly. Local models
    /// stream-download + SHA-256 verify into `<models_dir>/<sanitized_id>/`.
    pub async fn install(&mut self, id: &str) -> ModelResult<ModelManifest> {
        if self.store.exists(id) {
            return Err(ModelError::AlreadyExists(format!("model '{id}'")));
        }
        let spec = self.catalog_entry(id)?.clone();

        let (path, size_bytes, sha256) = match &spec.download_url {
            None => (None, 0u64, None),
            Some(url) => {
                let dir = self.models_dir.join(sanitize(id));
                std::fs::create_dir_all(&dir)?;
                let dest = dir.join("model.bin");
                let (size, hash) = downloader::download_and_verify(
                    url,
                    spec.sha256.as_deref().unwrap_or(""),
                    &dest,
                )
                .await?;
                (Some(dest.display().to_string()), size, Some(hash))
            }
        };

        let mut manifest = spec.clone();
        manifest.installed_at = Some(now_unix());
        manifest.path = path;
        manifest.size_bytes = size_bytes;
        manifest.sha256 = sha256.or(spec.sha256.clone());
        manifest.status = ManifestStatus::Installed;

        self.store.insert(manifest.clone())?;
        Ok(manifest)
    }

    /// Remove an installed model and delete its local files.
    pub fn remove(&mut self, id: &str) -> ModelResult<()> {
        let manifest = self.store.remove(id)?;
        if manifest.is_local() {
            let dir = self.models_dir.join(sanitize(id));
            let _ = std::fs::remove_dir_all(&dir);
        }
        Ok(())
    }

    // ── Verify (M6) ────────────────────────────────────────────────────────────

    /// Re-verify a local model's on-disk SHA-256.
    pub fn verify(&self, id: &str) -> ModelResult<verifier::VerifyStatus> {
        let manifest = self.get(id)?;
        Ok(verifier::verify_manifest(&manifest))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Filesystem-safe directory name from a model id (`a/b` → `a_b`).
pub(crate) fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub(crate) fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager() -> ModelManager {
        let tmp = std::env::temp_dir().join("valori_test_manager");
        std::fs::create_dir_all(&tmp).unwrap();
        let store = Box::new(JsonModelStore::new(&tmp).unwrap());
        ModelManager::new(&tmp, store).unwrap()
    }

    #[test]
    fn catalog_json_has_installed_and_available() {
        let m = make_manager();
        let c = m.catalog_json();
        assert!(c["available"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false));
    }

    #[test]
    fn resolve_fails_when_nothing_installed() {
        let m = make_manager();
        assert!(m.resolve(ModelTask::Embedding, Some(1536)).is_err());
    }

    #[test]
    fn provider_from_config_dummy() {
        let m = make_manager();
        let p = m
            .provider_from_config("dummy", "dummy", None, None, 4)
            .unwrap();
        assert_eq!(p.dim(), 4);
    }

    #[test]
    fn sanitize_id() {
        assert_eq!(
            sanitize("openai/text-embedding-3-small"),
            "openai_text-embedding-3-small"
        );
    }
}
