// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! M6 — Integrity Manager.
//!
//! Verifies and repairs installed packages without needing access to the
//! network configuration — it delegates to the existing [`verifier`] for
//! SHA-256 checking and to [`PackageStore`] for repair.

use serde::Serialize;

use crate::error::ModelResult;
use crate::now_unix;
use crate::package_store::PackageStore;
use crate::verifier::{verify_manifest_full, VerifyStatus};

// ── IntegrityReport ───────────────────────────────────────────────────────────

/// Per-package integrity report.
#[derive(Debug, Clone, Serialize)]
pub struct IntegrityReport {
    pub id: String,
    pub status: IntegrityStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_sha: Option<String>,
    pub checked_at: u64,
}

/// Outcome of an integrity check for one package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrityStatus {
    /// File present and SHA-256 matches.
    Verified,
    /// Remote-service model — no local file to verify.
    Remote,
    /// File missing from disk.
    Missing,
    /// File present but SHA-256 in manifest was not set (cannot verify).
    Unverified,
    /// File present but SHA-256 does not match.
    Corrupted,
}

impl IntegrityReport {
    pub fn is_healthy(&self) -> bool {
        matches!(
            self.status,
            IntegrityStatus::Verified | IntegrityStatus::Remote | IntegrityStatus::Unverified
        )
    }
}

// ── RepairResult ──────────────────────────────────────────────────────────────

/// Outcome of a repair attempt.
#[derive(Debug, Clone, Serialize)]
pub struct RepairResult {
    pub id: String,
    pub action: RepairAction,
}

/// What the repair did (or what the caller must do).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairAction {
    /// Already healthy — no action taken.
    AlreadyHealthy,
    /// Rescanned and updated the stored size on disk.
    SizeRepaired,
    /// Package was corrupted or missing — caller must reinstall via
    /// `PackageStore::install()`.  The manifest's `download_url` contains the URL.
    NeedsReinstall { download_url: Option<String> },
}

// ── IntegrityManager ──────────────────────────────────────────────────────────

/// Verify and repair installed packages.
///
/// ```rust,ignore
/// let mgr = IntegrityManager::new(&store);
/// let reports = mgr.verify_all();
/// for r in reports.iter().filter(|r| !r.is_healthy()) {
///     println!("PROBLEM: {} — {:?}", r.id, r.status);
/// }
/// ```
pub struct IntegrityManager<'a> {
    store: &'a PackageStore,
}

impl<'a> IntegrityManager<'a> {
    pub fn new(store: &'a PackageStore) -> Self {
        Self { store }
    }

    /// Verify a single package by model id.
    pub fn verify(&self, id: &str) -> ModelResult<IntegrityReport> {
        let pkg = self
            .store
            .get(id)
            .ok_or_else(|| crate::error::ModelError::NotFound(format!("package '{id}'")))?;
        Ok(self.check(&pkg.model))
    }

    /// Verify all installed packages.
    pub fn verify_all(&self) -> Vec<IntegrityReport> {
        self.store
            .list()
            .into_iter()
            .map(|pkg| self.check(&pkg.model))
            .collect()
    }

    fn check(&self, model: &crate::manifest::ModelManifest) -> IntegrityReport {
        let result = verify_manifest_full(model);
        let status = match result.status {
            VerifyStatus::Ok => IntegrityStatus::Verified,
            VerifyStatus::Remote => IntegrityStatus::Remote,
            VerifyStatus::Missing => IntegrityStatus::Missing,
            VerifyStatus::Unverified => IntegrityStatus::Unverified,
            VerifyStatus::Corrupted => IntegrityStatus::Corrupted,
        };
        IntegrityReport {
            id: model.id.clone(),
            status,
            expected_sha: result.expected_sha,
            actual_sha: result.actual_sha,
            checked_at: now_unix(),
        }
    }
}

/// Attempt to repair a package in place.
///
/// Takes `&mut PackageStore` because repair may update the stored manifest.
///
/// - Healthy packages → [`RepairAction::AlreadyHealthy`].
/// - Size discrepancy → [`RepairAction::SizeRepaired`] (rescans disk).
/// - Missing / corrupted → [`RepairAction::NeedsReinstall`] (caller re-downloads).
pub fn repair(store: &mut PackageStore, id: &str) -> ModelResult<RepairResult> {
    let pkg = store
        .get(id)
        .ok_or_else(|| crate::error::ModelError::NotFound(format!("package '{id}'")))?;
    let mgr = IntegrityManager::new(store);
    let report = mgr.check(&pkg.model);

    let action = match report.status {
        IntegrityStatus::Verified | IntegrityStatus::Remote | IntegrityStatus::Unverified => {
            // Re-scan size in case it drifted (file was overwritten externally).
            store.repair(id)?;
            RepairAction::AlreadyHealthy
        }
        IntegrityStatus::Missing | IntegrityStatus::Corrupted => RepairAction::NeedsReinstall {
            download_url: pkg.model.download_url.clone(),
        },
    };

    Ok(RepairResult {
        id: id.to_string(),
        action,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::downloader::sha256_hex;
    use crate::manifest::ModelManifest;
    use crate::package_store::PackageStore;
    use crate::types::{ManifestStatus, ModelFormat, ModelTask, ProviderKind};

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
    fn verify_remote_model_is_remote() {
        let (mut store, _tmp) = make_store();
        store.register(remote_manifest("openai/embed")).unwrap();
        let mgr = IntegrityManager::new(&store);
        let r = mgr.verify("openai/embed").unwrap();
        assert_eq!(r.status, IntegrityStatus::Remote);
        assert!(r.is_healthy());
    }

    #[test]
    fn verify_all_covers_all_packages() {
        let (mut store, _tmp) = make_store();
        store.register(remote_manifest("m1")).unwrap();
        store.register(remote_manifest("m2")).unwrap();
        let mgr = IntegrityManager::new(&store);
        assert_eq!(mgr.verify_all().len(), 2);
    }

    #[test]
    fn verify_not_found_errors() {
        let (store, _tmp) = make_store();
        let mgr = IntegrityManager::new(&store);
        let err = mgr.verify("nonexistent").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn verify_valid_file_is_verified() {
        let (mut store, _tmp) = make_store();
        let data = b"model weights";
        let sha = sha256_hex(data);
        let stage = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(stage.path(), data).unwrap();

        let mut m = remote_manifest("local/mymodel");
        m.provider = ProviderKind::Local;
        m.format = ModelFormat::Onnx;
        m.sha256 = Some(sha);
        store.commit_staged(m, stage.path()).unwrap();

        let mgr = IntegrityManager::new(&store);
        let r = mgr.verify("local/mymodel").unwrap();
        assert_eq!(r.status, IntegrityStatus::Verified);
    }

    #[test]
    fn verify_corrupted_file_is_corrupted() {
        let (mut store, tmp) = make_store();
        let data = b"model weights";
        let sha = sha256_hex(data);
        let stage = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(stage.path(), data).unwrap();

        let mut m = remote_manifest("local/corrupt");
        m.provider = ProviderKind::Local;
        m.format = ModelFormat::Onnx;
        m.sha256 = Some(sha);
        let pkg = store.commit_staged(m, stage.path()).unwrap();

        // Corrupt the file.
        let model_path = pkg.model.path.as_ref().unwrap();
        std::fs::write(model_path, b"garbage").unwrap();

        let mgr = IntegrityManager::new(&store);
        let r = mgr.verify("local/corrupt").unwrap();
        assert_eq!(r.status, IntegrityStatus::Corrupted);
        assert!(!r.is_healthy());

        drop(tmp);
    }

    #[test]
    fn repair_remote_model_is_already_healthy() {
        let (mut store, _tmp) = make_store();
        store.register(remote_manifest("openai/embed")).unwrap();
        let result = repair(&mut store, "openai/embed").unwrap();
        assert_eq!(result.action, RepairAction::AlreadyHealthy);
    }

    #[test]
    fn repair_not_found_errors() {
        let (mut store, _tmp) = make_store();
        let err = repair(&mut store, "ghost").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }
}
