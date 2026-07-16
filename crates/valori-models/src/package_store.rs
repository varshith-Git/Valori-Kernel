// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! M5 — Local Package Store.
//!
//! Manages the on-disk layout for installed AI models:
//!
//! ```text
//! <root>/
//!   .locks/                        # advisory lock files (RAII)
//!   .tmp/                          # staging area for atomic installs
//!   embedding/
//!     bge-small-en-v1/
//!       manifest.json              # PackageManifest (schema + ModelManifest)
//!       model.bin                  # (reserved for future local inference)
//!   reranker/
//!     bge-reranker-base/
//!       manifest.json
//! ```
//!
//! # Atomic install (M5.1)
//!
//! Downloads go to `.tmp/<timestamp>/model.bin`, SHA-256 is verified, then
//! the staging directory is renamed into the final location in one `fs::rename`
//! call.  If the process crashes between download and rename, the `.tmp/`
//! entry is cleaned up on the next `PackageStore::new()`.
//!
//! # File locking (M5.2)
//!
//! [`InstallLock`] acquires an exclusive file in `.locks/<id>.lock` using
//! `OpenOptions::create_new`.  The lock is released on drop.  A second
//! process that calls [`PackageStore::acquire_lock`] for the same model while
//! the first holds it gets [`ModelError::InstallConflict`].
//!
//! # Manifest versioning (M5.3)
//!
//! Each package directory contains `manifest.json` — a [`PackageManifest`]
//! that embeds the full [`ModelManifest`] plus `schema_version`, a
//! `package_version` string, `created` / `updated` unix timestamps, and a
//! `size` (total on-disk bytes).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::downloader::{download_and_verify, sha256_hex};
use crate::error::{ModelError, ModelResult};
use crate::manifest::ModelManifest;
use crate::types::{ManifestStatus, ModelTask};
use crate::{now_unix, sanitize};

// ── PackageManifest (M5.3) ────────────────────────────────────────────────────

/// On-disk versioned wrapper around [`ModelManifest`].
///
/// Written to `<package-dir>/manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    /// Bumped when the schema of this file changes.
    pub schema_version: u32,
    /// Semantic version of the package format (not the model weights).
    pub package_version: String,
    /// Unix timestamp when this package was first installed.
    pub created: u64,
    /// Unix timestamp of the last modification (repair, re-verify, update).
    pub updated: u64,
    /// Total bytes occupied by this package directory on disk.
    pub size: u64,
    /// The embedded model manifest.
    pub model: ModelManifest,
}

impl PackageManifest {
    fn new(model: ModelManifest, size: u64) -> Self {
        let now = now_unix();
        Self {
            schema_version: 1,
            package_version: "1.0.0".into(),
            created: now,
            updated: now,
            size,
            model,
        }
    }

    fn touch(&mut self) {
        self.updated = now_unix();
    }
}

// ── InstallLock (M5.2) ────────────────────────────────────────────────────────

/// RAII exclusive lock for a single model install operation.
///
/// Released automatically on drop by removing the lock file.
#[derive(Debug)]
pub struct InstallLock {
    path: PathBuf,
}

impl InstallLock {
    /// Attempt to acquire the lock for `model_id`.
    ///
    /// Fails with [`ModelError::InstallConflict`] if another process holds it.
    pub fn acquire(locks_dir: &Path, model_id: &str) -> ModelResult<Self> {
        std::fs::create_dir_all(locks_dir)?;
        let path = locks_dir.join(format!("{}.lock", sanitize(model_id)));
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|_| ModelError::InstallConflict(model_id.to_string()))?;
        Ok(Self { path })
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

// ── PackageStore ──────────────────────────────────────────────────────────────

/// On-disk package manager for AI model packages.
///
/// Each model lives in `<root>/<task>/<sanitized-id>/manifest.json`.
/// No central index — `list()` scans the directory tree, which is correct
/// for a model store that rarely exceeds a few hundred entries.
pub struct PackageStore {
    root: PathBuf,
}

impl PackageStore {
    /// Open (or create) a package store rooted at `root`.
    ///
    /// Cleans up any leftover `.tmp/` staging directories from crashed installs.
    pub fn new(root: impl AsRef<Path>) -> ModelResult<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(root.join(".locks"))?;

        // Clean up stale staging dirs from previous crashed installs.
        let tmp = root.join(".tmp");
        if tmp.exists() {
            let _ = std::fs::remove_dir_all(&tmp);
        }
        std::fs::create_dir_all(&tmp)?;

        Ok(Self { root })
    }

    // ── Directories ───────────────────────────────────────────────────────────

    /// Canonical directory for a model: `<root>/<task>/<sanitized-id>`.
    pub fn package_dir(&self, manifest: &ModelManifest) -> PathBuf {
        self.root
            .join(manifest.task.to_string())
            .join(sanitize(&manifest.id))
    }

    fn locks_dir(&self) -> PathBuf {
        self.root.join(".locks")
    }

    fn tmp_dir(&self) -> PathBuf {
        self.root.join(".tmp")
    }

    fn manifest_path(pkg_dir: &Path) -> PathBuf {
        pkg_dir.join("manifest.json")
    }

    // ── Lock acquisition ──────────────────────────────────────────────────────

    /// Acquire an exclusive install lock for `model_id`.
    pub fn acquire_lock(&self, model_id: &str) -> ModelResult<InstallLock> {
        InstallLock::acquire(&self.locks_dir(), model_id)
    }

    // ── Query ─────────────────────────────────────────────────────────────────

    /// True if a package directory + manifest.json exists for `id`.
    pub fn exists(&self, id: &str) -> bool {
        // Walk task dirs looking for a matching sanitized id.
        let target = sanitize(id);
        self.iter_task_dirs()
            .any(|task_dir| task_dir.join(&target).join("manifest.json").exists())
    }

    /// Read a single package manifest by model id.
    pub fn get(&self, id: &str) -> Option<PackageManifest> {
        let target = sanitize(id);
        for task_dir in self.iter_task_dirs() {
            let pkg_dir = task_dir.join(&target);
            if let Some(pkg) = self.read_manifest(&pkg_dir) {
                return Some(pkg);
            }
        }
        None
    }

    /// All installed packages (scans disk).
    pub fn list(&self) -> Vec<PackageManifest> {
        self.scan_all()
    }

    /// All packages whose task matches `task`.
    pub fn find_by_task(&self, task: &ModelTask) -> Vec<PackageManifest> {
        let task_dir = self.root.join(task.to_string());
        self.scan_dir(&task_dir)
    }

    /// Total bytes occupied by all installed packages.
    pub fn disk_usage(&self) -> u64 {
        self.scan_all().iter().map(|p| p.size).sum()
    }

    /// IDs of all installed packages.
    pub fn installed_ids(&self) -> HashSet<String> {
        self.scan_all().into_iter().map(|p| p.model.id).collect()
    }

    // ── Mutation ──────────────────────────────────────────────────────────────

    /// Register a remote-service model (no download needed, no local files).
    ///
    /// Creates the package directory and writes `manifest.json`.
    pub fn register(&mut self, mut model: ModelManifest) -> ModelResult<PackageManifest> {
        let id = model.id.clone();
        if self.exists(&id) {
            return Err(ModelError::AlreadyExists(format!("package '{id}'")));
        }
        let _lock = self.acquire_lock(&id)?;
        let pkg_dir = self.package_dir(&model);
        std::fs::create_dir_all(&pkg_dir)?;

        model.status = ManifestStatus::Installed;
        model.installed_at = Some(now_unix());
        let pkg = PackageManifest::new(model, 0);
        self.write_manifest(&pkg_dir, &pkg)?;
        Ok(pkg)
    }

    /// Full atomic install with download (M5.1).
    ///
    /// 1. Acquire install lock.
    /// 2. Download to `.tmp/<timestamp>/model.bin`.
    /// 3. Verify SHA-256.
    /// 4. Rename staging dir → final package dir.
    /// 5. Write `manifest.json`.
    pub async fn install(&mut self, mut model: ModelManifest) -> ModelResult<PackageManifest> {
        let id = model.id.clone();
        if self.exists(&id) {
            return Err(ModelError::AlreadyExists(format!("package '{id}'")));
        }
        let _lock = self.acquire_lock(&id)?;

        let (path, size_bytes, sha256) = match &model.download_url.clone() {
            None => (None, 0u64, None),
            Some(url) => {
                // Stage download in .tmp/<timestamp>/
                let stage_dir = self.tmp_dir().join(now_unix().to_string());
                std::fs::create_dir_all(&stage_dir)?;
                let staged = stage_dir.join("model.bin");

                let (size, hash) =
                    download_and_verify(url, model.sha256.as_deref().unwrap_or(""), &staged)
                        .await?;

                // Atomic rename: staging dir → final package dir
                let pkg_dir = self.package_dir(&model);
                if pkg_dir.exists() {
                    let _ = std::fs::remove_dir_all(&pkg_dir);
                }
                std::fs::rename(&stage_dir, &pkg_dir)?;

                let final_path = pkg_dir.join("model.bin");
                (Some(final_path.display().to_string()), size, Some(hash))
            }
        };

        // If no download, still create the package directory.
        let pkg_dir = self.package_dir(&model);
        std::fs::create_dir_all(&pkg_dir)?;

        model.status = ManifestStatus::Installed;
        model.installed_at = Some(now_unix());
        model.path = path;
        model.size_bytes = size_bytes;
        model.sha256 = sha256.or(model.sha256.clone());

        let pkg = PackageManifest::new(model, size_bytes);
        self.write_manifest(&pkg_dir, &pkg)?;
        Ok(pkg)
    }

    /// Commit a pre-staged file into the package store atomically.
    ///
    /// Use this when download is orchestrated externally (e.g. by [`DownloadJob`]).
    /// `staged_file` must already exist; it is renamed into the package directory.
    pub fn commit_staged(
        &mut self,
        mut model: ModelManifest,
        staged_file: &Path,
    ) -> ModelResult<PackageManifest> {
        let id = model.id.clone();
        let _lock = self.acquire_lock(&id)?;

        let pkg_dir = self.package_dir(&model);
        std::fs::create_dir_all(&pkg_dir)?;

        let dest = pkg_dir.join(
            staged_file
                .file_name()
                .unwrap_or(std::ffi::OsStr::new("model.bin")),
        );

        // Verify SHA before committing.
        if let Some(expected) = &model.sha256 {
            let bytes = std::fs::read(staged_file)?;
            let actual = sha256_hex(&bytes);
            if &actual != expected {
                return Err(ModelError::Verify(format!(
                    "SHA mismatch: expected {expected}, got {actual}"
                )));
            }
        }

        let size = staged_file.metadata().map(|m| m.len()).unwrap_or(0);
        std::fs::rename(staged_file, &dest)?;

        model.status = ManifestStatus::Installed;
        model.installed_at = Some(now_unix());
        model.path = Some(dest.display().to_string());
        model.size_bytes = size;

        let pkg = PackageManifest::new(model, size);
        self.write_manifest(&pkg_dir, &pkg)?;
        Ok(pkg)
    }

    /// Remove a package: deletes its directory and all contained files.
    pub fn remove(&mut self, id: &str) -> ModelResult<PackageManifest> {
        let pkg = self
            .get(id)
            .ok_or_else(|| ModelError::NotFound(format!("package '{id}'")))?;
        let pkg_dir = self.package_dir(&pkg.model);
        if pkg_dir.exists() {
            std::fs::remove_dir_all(&pkg_dir)?;
        }
        Ok(pkg)
    }

    /// Re-scan the package directory and update the stored size.
    pub fn repair(&mut self, id: &str) -> ModelResult<PackageManifest> {
        let mut pkg = self
            .get(id)
            .ok_or_else(|| ModelError::NotFound(format!("package '{id}'")))?;
        let pkg_dir = self.package_dir(&pkg.model);
        let size = dir_size(&pkg_dir);
        pkg.size = size;
        pkg.model.size_bytes = size;
        pkg.touch();
        self.write_manifest(&pkg_dir, &pkg)?;
        Ok(pkg)
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    fn iter_task_dirs(&self) -> impl Iterator<Item = PathBuf> + '_ {
        let tasks = [
            ModelTask::Embedding,
            ModelTask::Generation,
            ModelTask::Reranker,
            ModelTask::Vision,
            ModelTask::Speech,
        ];
        tasks.into_iter().map(|t| self.root.join(t.to_string()))
    }

    fn scan_all(&self) -> Vec<PackageManifest> {
        self.iter_task_dirs()
            .flat_map(|d| self.scan_dir(&d))
            .collect()
    }

    fn scan_dir(&self, task_dir: &Path) -> Vec<PackageManifest> {
        let Ok(entries) = std::fs::read_dir(task_dir) else {
            return vec![];
        };
        entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| self.read_manifest(&e.path()))
            .collect()
    }

    fn read_manifest(&self, pkg_dir: &Path) -> Option<PackageManifest> {
        let path = Self::manifest_path(pkg_dir);
        let bytes = std::fs::read(&path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn write_manifest(&self, pkg_dir: &Path, pkg: &PackageManifest) -> ModelResult<()> {
        let path = Self::manifest_path(pkg_dir);
        let json = serde_json::to_vec_pretty(pkg)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Recursively sum bytes in a directory.
pub(crate) fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| {
            let p = e.path();
            if p.is_dir() {
                dir_size(&p)
            } else {
                p.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ModelFormat, ProviderKind};

    fn remote_manifest(id: &str) -> ModelManifest {
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
            installed_at: None,
            path: None,
            status: ManifestStatus::Available,
            min_ram_mb: 0,
            license: None,
            homepage: None,
            download_url: None,
        }
    }

    fn make_store() -> (PackageStore, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let store = PackageStore::new(tmp.path()).unwrap();
        (store, tmp)
    }

    #[test]
    fn register_and_exists() {
        let (mut s, _tmp) = make_store();
        assert!(!s.exists("openai/text-embedding-3-small"));
        s.register(remote_manifest("openai/text-embedding-3-small"))
            .unwrap();
        assert!(s.exists("openai/text-embedding-3-small"));
    }

    #[test]
    fn duplicate_register_errors() {
        let (mut s, _tmp) = make_store();
        s.register(remote_manifest("openai/text-embedding-3-small"))
            .unwrap();
        let err = s
            .register(remote_manifest("openai/text-embedding-3-small"))
            .unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn list_returns_registered_packages() {
        let (mut s, _tmp) = make_store();
        s.register(remote_manifest("m1")).unwrap();
        s.register(remote_manifest("m2")).unwrap();
        assert_eq!(s.list().len(), 2);
    }

    #[test]
    fn get_by_id() {
        let (mut s, _tmp) = make_store();
        s.register(remote_manifest("openai/text-embedding-3-small"))
            .unwrap();
        let pkg = s.get("openai/text-embedding-3-small").unwrap();
        assert_eq!(pkg.model.id, "openai/text-embedding-3-small");
        assert_eq!(pkg.schema_version, 1);
        assert!(pkg.created > 0);
    }

    #[test]
    fn remove_clears_directory() {
        let (mut s, _tmp) = make_store();
        s.register(remote_manifest("openai/ada-002")).unwrap();
        assert!(s.exists("openai/ada-002"));
        s.remove("openai/ada-002").unwrap();
        assert!(!s.exists("openai/ada-002"));
        assert_eq!(s.list().len(), 0);
    }

    #[test]
    fn find_by_task_filters_correctly() {
        let (mut s, _tmp) = make_store();
        s.register(remote_manifest("embed-model")).unwrap();

        let mut gen = remote_manifest("gen-model");
        gen.task = ModelTask::Generation;
        s.register(gen).unwrap();

        let embeddings = s.find_by_task(&ModelTask::Embedding);
        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0].model.id, "embed-model");
    }

    #[test]
    fn disk_usage_sums_package_sizes() {
        let (mut s, _tmp) = make_store();
        s.register(remote_manifest("m1")).unwrap();
        s.register(remote_manifest("m2")).unwrap();
        // Remote models have size 0; total should be 0.
        assert_eq!(s.disk_usage(), 0);
    }

    #[test]
    fn commit_staged_verifies_sha() {
        let (mut s, _tmp) = make_store();
        let data = b"fake model weights";
        let sha = sha256_hex(data);

        let stage = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(stage.path(), data).unwrap();

        let mut m = remote_manifest("local/mymodel");
        m.provider = ProviderKind::Local;
        m.format = ModelFormat::Onnx;
        m.sha256 = Some(sha);

        let pkg = s.commit_staged(m, stage.path()).unwrap();
        assert!(s.exists("local/mymodel"));
        assert!(pkg.model.path.is_some());
    }

    #[test]
    fn commit_staged_rejects_bad_sha() {
        let (mut s, _tmp) = make_store();
        let stage = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(stage.path(), b"data").unwrap();

        let mut m = remote_manifest("local/badmodel");
        m.sha256 = Some("deadbeef".repeat(8));

        let err = s.commit_staged(m, stage.path()).unwrap_err();
        assert!(err.to_string().contains("SHA mismatch") || err.to_string().contains("erif"));
    }

    #[test]
    fn install_lock_prevents_duplicate_acquire() {
        let tmp = tempfile::tempdir().unwrap();
        let locks = tmp.path().join(".locks");
        let _lock1 = InstallLock::acquire(&locks, "my-model").unwrap();
        let err = InstallLock::acquire(&locks, "my-model").unwrap_err();
        assert!(err.to_string().contains("already being installed"));
    }

    #[test]
    fn install_lock_releases_on_drop() {
        let tmp = tempfile::tempdir().unwrap();
        let locks = tmp.path().join(".locks");
        {
            let _lock = InstallLock::acquire(&locks, "my-model").unwrap();
        }
        // Should succeed after drop.
        let _lock2 = InstallLock::acquire(&locks, "my-model").unwrap();
    }

    #[test]
    fn repair_updates_size() {
        let (mut s, _tmp) = make_store();
        s.register(remote_manifest("openai/ada")).unwrap();
        let pkg = s.repair("openai/ada").unwrap();
        assert!(pkg.updated >= pkg.created);
    }

    #[test]
    fn stale_tmp_cleaned_on_new() {
        let tmp = tempfile::tempdir().unwrap();
        // Pre-create a stale staging dir.
        let stale = tmp.path().join(".tmp").join("stale-dir");
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join("model.bin"), b"stale").unwrap();
        // Opening the store should clean it.
        let _s = PackageStore::new(tmp.path()).unwrap();
        // .tmp/ exists but its stale subdir is gone.
        assert!(!tmp.path().join(".tmp").join("stale-dir").exists());
    }
}
