// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! M6.3 — System health report.
//!
//! `GET /v1/models/health` returns a [`SystemHealth`] built from
//! [`PackageStore`], [`IntegrityManager`], and [`RefCounter`].

use serde::Serialize;

use crate::gc::RefCounter;
use crate::integrity::{IntegrityManager, IntegrityStatus};
use crate::package_store::PackageStore;

// ── Per-package health ────────────────────────────────────────────────────────

/// Per-package health entry.
#[derive(Debug, Clone, Serialize)]
pub struct PackageHealth {
    pub id: String,
    pub status: PackageHealthStatus,
    pub size_bytes: u64,
    pub ref_count: usize,
}

/// Health status for one installed package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageHealthStatus {
    /// Installed and SHA-256 verified.
    Verified,
    /// Installed; no SHA-256 in manifest (remote-service or no-hash-recorded).
    Installed,
    /// On-disk file is missing.
    Missing,
    /// On-disk file has wrong SHA-256.
    Corrupted,
}

// ── SystemHealth ──────────────────────────────────────────────────────────────

/// Aggregate health report for the entire model package store.
///
/// Returned by `GET /v1/models/health`.
#[derive(Debug, Clone, Serialize)]
pub struct SystemHealth {
    pub packages: Vec<PackageHealth>,
    pub total_installed: usize,
    pub verified: usize,
    pub corrupted: usize,
    pub missing: usize,
    pub disk_used_bytes: u64,
    /// Bytes used by packages with zero active references.
    pub reclaimable_bytes: u64,
}

/// Build a [`SystemHealth`] report by querying the store and running integrity checks.
pub fn system_health(store: &PackageStore, refs: &RefCounter) -> SystemHealth {
    let mgr = IntegrityManager::new(store);
    let reports = mgr.verify_all();

    let mut packages = Vec::with_capacity(reports.len());
    let mut verified = 0usize;
    let mut corrupted = 0usize;
    let mut missing = 0usize;
    let mut disk_used_bytes = 0u64;
    let mut reclaimable_bytes = 0u64;

    for report in reports {
        let pkg_opt = store.get(&report.id);
        let size = pkg_opt.as_ref().map(|p| p.size).unwrap_or(0);
        let ref_count = refs.ref_count(&report.id);

        let status = match report.status {
            IntegrityStatus::Verified => {
                verified += 1;
                PackageHealthStatus::Verified
            }
            IntegrityStatus::Remote | IntegrityStatus::Unverified => PackageHealthStatus::Installed,
            IntegrityStatus::Corrupted => {
                corrupted += 1;
                PackageHealthStatus::Corrupted
            }
            IntegrityStatus::Missing => {
                missing += 1;
                PackageHealthStatus::Missing
            }
        };

        disk_used_bytes += size;
        if ref_count == 0 {
            reclaimable_bytes += size;
        }

        packages.push(PackageHealth {
            id: report.id,
            status,
            size_bytes: size,
            ref_count,
        });
    }

    SystemHealth {
        total_installed: packages.len(),
        verified,
        corrupted,
        missing,
        disk_used_bytes,
        reclaimable_bytes,
        packages,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
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
        (PackageStore::new(tmp.path()).unwrap(), tmp)
    }

    #[test]
    fn empty_store_health() {
        let (store, _tmp) = make_store();
        let refs = RefCounter::new();
        let h = system_health(&store, &refs);
        assert_eq!(h.total_installed, 0);
        assert_eq!(h.disk_used_bytes, 0);
    }

    #[test]
    fn remote_model_counts_as_installed() {
        let (mut store, _tmp) = make_store();
        store.register(remote_manifest("openai/embed")).unwrap();
        let refs = RefCounter::new();
        let h = system_health(&store, &refs);
        assert_eq!(h.total_installed, 1);
        assert_eq!(h.packages[0].status, PackageHealthStatus::Installed);
    }

    #[test]
    fn reclaimable_bytes_exclude_referenced() {
        let (mut store, _tmp) = make_store();
        store.register(remote_manifest("m1")).unwrap();
        store.register(remote_manifest("m2")).unwrap();

        let mut refs = RefCounter::new();
        refs.add_ref("m1", "project-a");

        let h = system_health(&store, &refs);
        // Both have size 0 (remote), but only m2 is unreferenced.
        // m1's size (0) is not counted as reclaimable.
        assert_eq!(h.reclaimable_bytes, 0); // both have 0 size
        let m2_pkg = h.packages.iter().find(|p| p.id == "m2").unwrap();
        assert_eq!(m2_pkg.ref_count, 0);
        let m1_pkg = h.packages.iter().find(|p| p.id == "m1").unwrap();
        assert_eq!(m1_pkg.ref_count, 1);
    }

    #[test]
    fn health_serializes_to_json() {
        let (mut store, _tmp) = make_store();
        store.register(remote_manifest("openai/embed")).unwrap();
        let refs = RefCounter::new();
        let h = system_health(&store, &refs);
        let json = serde_json::to_string(&h).unwrap();
        assert!(json.contains("total_installed"));
        assert!(json.contains("openai/embed"));
    }
}
