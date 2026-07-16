// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `ModelStore` — persistence seam for installed model manifests.
//!
//! `JsonModelStore` is the default on-disk backend (write-then-rename atomic
//! flush). Any backend that implements `ModelStore` drops in without changing
//! `ModelManager`.

use std::path::{Path, PathBuf};

use crate::error::{ModelError, ModelResult};
use crate::manifest::ModelManifest;

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Persistence contract for installed model manifests.
///
/// Only `Installed` manifests are persisted; transient states live in memory.
pub trait ModelStore: Send + Sync {
    fn list(&self) -> Vec<ModelManifest>;
    fn get(&self, id: &str) -> Option<ModelManifest>;
    fn exists(&self, id: &str) -> bool;
    fn insert(&mut self, manifest: ModelManifest) -> ModelResult<()>;
    fn remove(&mut self, id: &str) -> ModelResult<ModelManifest>;
    /// Update an existing entry in place (used when status or path changes).
    fn update(&mut self, manifest: ModelManifest) -> ModelResult<()>;
}

// ── JSON backend ──────────────────────────────────────────────────────────────

/// Filesystem-backed `ModelStore` (`<home>/models.json`, write-then-rename).
pub struct JsonModelStore {
    file: PathBuf,
    manifests: Vec<ModelManifest>,
}

impl JsonModelStore {
    pub fn new(home: impl AsRef<Path>) -> ModelResult<Self> {
        let file = home.as_ref().join("models.json");
        let manifests = match std::fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(ModelError::Json)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(ModelError::Io(e)),
        };
        Ok(Self { file, manifests })
    }

    fn flush(&self) -> ModelResult<()> {
        let tmp = self.file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&self.manifests)?)?;
        std::fs::rename(&tmp, &self.file)?;
        Ok(())
    }
}

impl ModelStore for JsonModelStore {
    fn list(&self) -> Vec<ModelManifest> {
        self.manifests.clone()
    }

    fn get(&self, id: &str) -> Option<ModelManifest> {
        self.manifests.iter().find(|m| m.id == id).cloned()
    }

    fn exists(&self, id: &str) -> bool {
        self.manifests.iter().any(|m| m.id == id)
    }

    fn insert(&mut self, manifest: ModelManifest) -> ModelResult<()> {
        self.manifests.push(manifest);
        self.flush()
    }

    fn remove(&mut self, id: &str) -> ModelResult<ModelManifest> {
        let idx = self
            .manifests
            .iter()
            .position(|m| m.id == id)
            .ok_or_else(|| ModelError::NotFound(format!("model '{id}'")))?;
        let removed = self.manifests.remove(idx);
        self.flush()?;
        Ok(removed)
    }

    fn update(&mut self, manifest: ModelManifest) -> ModelResult<()> {
        let id = manifest.id.clone();
        let idx = self
            .manifests
            .iter()
            .position(|m| m.id == id)
            .ok_or_else(|| ModelError::NotFound(format!("model '{id}'")))?;
        self.manifests[idx] = manifest;
        self.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ManifestStatus, ModelFormat, ModelTask, ProviderKind};

    fn manifest(id: &str) -> ModelManifest {
        ModelManifest {
            id: id.into(),
            name: id.into(),
            version: None,
            provider: ProviderKind::OpenAI,
            family: None,
            task: ModelTask::Embedding,
            dimensions: 1536,
            quantization: None,
            format: ModelFormat::Remote,
            sha256: None,
            size_bytes: 0,
            installed_at: Some(0),
            path: None,
            status: ManifestStatus::Installed,
            min_ram_mb: 0,
            license: None,
            homepage: None,
            download_url: None,
        }
    }

    #[test]
    fn crud_roundtrip() {
        let home = tempfile::tempdir().unwrap();
        let mut store = JsonModelStore::new(home.path()).unwrap();
        assert!(store.list().is_empty());

        store.insert(manifest("a")).unwrap();
        store.insert(manifest("b")).unwrap();
        assert_eq!(store.list().len(), 2);
        assert!(store.exists("a"));

        let removed = store.remove("a").unwrap();
        assert_eq!(removed.id, "a");
        assert!(!store.exists("a"));

        // Reload from disk.
        let store2 = JsonModelStore::new(home.path()).unwrap();
        assert_eq!(store2.list().len(), 1);
        assert!(store2.exists("b"));
    }

    #[test]
    fn update_changes_field() {
        let home = tempfile::tempdir().unwrap();
        let mut store = JsonModelStore::new(home.path()).unwrap();
        store.insert(manifest("x")).unwrap();

        let mut updated = manifest("x");
        updated.size_bytes = 99;
        store.update(updated).unwrap();

        assert_eq!(store.get("x").unwrap().size_bytes, 99);
    }

    #[test]
    fn remove_nonexistent_errors() {
        let home = tempfile::tempdir().unwrap();
        let mut store = JsonModelStore::new(home.path()).unwrap();
        assert!(store.remove("missing").is_err());
    }
}
