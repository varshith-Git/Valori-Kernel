// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! M6.1–M6.3 — Garbage Collector + Reference Counter.
//!
//! # Reference counting (M6.2)
//!
//! [`RefCounter`] tracks which projects / collections are using each installed
//! model.  When a collection is created with a given `model_id`, the node adds
//! a reference.  When the collection is dropped, the reference is removed.
//! [`GarbageCollector`] uses the ref counter to identify models that are safe
//! to delete.
//!
//! # Garbage collection (M6.1)
//!
//! [`GarbageCollector`] compares the set of installed packages against the set
//! of referenced model IDs.  Packages with zero references are "unreferenced"
//! and their bytes are reclaimable.  Callers decide whether to actually free
//! them by calling [`GarbageCollector::clean`].
//!
//! # Design notes
//!
//! - [`RefCounter`] is in-memory only.  The node is responsible for
//!   re-populating it from persistent collection metadata on startup.
//! - [`GarbageCollector`] never deletes a model that has ≥ 1 reference, even
//!   if asked to `clean` it directly.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::error::{ModelError, ModelResult};
use crate::package_store::PackageStore;

// ── RefCounter (M6.2) ─────────────────────────────────────────────────────────

/// Tracks which collections / projects reference each installed model.
///
/// Reference counts are in-memory; the node re-builds them from its metadata
/// store on startup.
///
/// ```rust,ignore
/// let mut refs = RefCounter::new();
/// refs.add_ref("openai/text-embedding-3-small", "project-acme");
/// refs.add_ref("openai/text-embedding-3-small", "project-beta");
/// assert_eq!(refs.ref_count("openai/text-embedding-3-small"), 2);
/// assert!(!refs.can_delete("openai/text-embedding-3-small"));
/// refs.remove_ref("openai/text-embedding-3-small", "project-acme");
/// refs.remove_ref("openai/text-embedding-3-small", "project-beta");
/// assert!(refs.can_delete("openai/text-embedding-3-small"));
/// ```
#[derive(Debug, Default)]
pub struct RefCounter {
    /// model_id → set of project/collection ids that reference it.
    refs: HashMap<String, HashSet<String>>,
}

impl RefCounter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `project_id` is using `model_id`.
    pub fn add_ref(&mut self, model_id: &str, project_id: &str) {
        self.refs
            .entry(model_id.to_string())
            .or_default()
            .insert(project_id.to_string());
    }

    /// Remove `project_id`'s reference to `model_id`.
    ///
    /// Silently ignores unknown (model_id, project_id) pairs.
    pub fn remove_ref(&mut self, model_id: &str, project_id: &str) {
        if let Some(set) = self.refs.get_mut(model_id) {
            set.remove(project_id);
            if set.is_empty() {
                self.refs.remove(model_id);
            }
        }
    }

    /// Number of projects currently referencing `model_id`.
    pub fn ref_count(&self, model_id: &str) -> usize {
        self.refs.get(model_id).map(|s| s.len()).unwrap_or(0)
    }

    /// Returns `true` if no project references this model (safe to delete).
    pub fn can_delete(&self, model_id: &str) -> bool {
        self.ref_count(model_id) == 0
    }

    /// Snapshot of all model IDs that have at least one reference.
    pub fn all_referenced_ids(&self) -> HashSet<String> {
        self.refs.keys().cloned().collect()
    }

    /// Project IDs that reference `model_id`.
    pub fn referencing_projects(&self, model_id: &str) -> Vec<String> {
        self.refs
            .get(model_id)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }
}

// ── UnreferencedPackage ───────────────────────────────────────────────────────

/// A package that no project currently references.
#[derive(Debug, Clone, Serialize)]
pub struct UnreferencedPackage {
    pub id: String,
    /// On-disk bytes that would be freed by removing this package.
    pub reclaimable_bytes: u64,
}

/// Summary of a garbage-collection scan.
#[derive(Debug, Clone, Serialize)]
pub struct GcReport {
    pub unreferenced: Vec<UnreferencedPackage>,
    /// Total bytes across all unreferenced packages.
    pub reclaimable_bytes: u64,
}

// ── GarbageCollector (M6.1) ───────────────────────────────────────────────────

/// Identifies unreferenced packages and (optionally) removes them.
pub struct GarbageCollector<'a> {
    store: &'a mut PackageStore,
}

impl<'a> GarbageCollector<'a> {
    pub fn new(store: &'a mut PackageStore) -> Self {
        Self { store }
    }

    /// Scan installed packages and report which ones are unreferenced.
    ///
    /// Does NOT remove anything — call [`clean`] for that.
    pub fn scan(&self, refs: &RefCounter) -> GcReport {
        let unreferenced: Vec<UnreferencedPackage> = self
            .store
            .list()
            .into_iter()
            .filter(|p| refs.can_delete(&p.model.id))
            .map(|p| UnreferencedPackage {
                reclaimable_bytes: p.size,
                id: p.model.id,
            })
            .collect();

        let reclaimable_bytes = unreferenced.iter().map(|u| u.reclaimable_bytes).sum();
        GcReport {
            unreferenced,
            reclaimable_bytes,
        }
    }

    /// Remove all unreferenced packages and return the bytes freed.
    ///
    /// Skips any package that has acquired a reference between [`scan`] and
    /// this call (double-checks before deletion).
    pub fn clean(&mut self, refs: &RefCounter) -> ModelResult<u64> {
        let report = self.scan(refs);
        let mut freed = 0u64;
        for pkg in report.unreferenced {
            // Re-check: a ref may have been added since the scan.
            if refs.can_delete(&pkg.id) {
                freed += pkg.reclaimable_bytes;
                let _ = self.store.remove(&pkg.id);
            }
        }
        Ok(freed)
    }

    /// Refuse to delete `model_id` if it has active references.
    ///
    /// Returns [`ModelError::Provider`] (re-using the closest semantic error)
    /// if the model is in use.
    pub fn safe_delete(&mut self, model_id: &str, refs: &RefCounter) -> ModelResult<()> {
        if !refs.can_delete(model_id) {
            let projects = refs.referencing_projects(model_id).join(", ");
            return Err(ModelError::Provider(format!(
                "cannot delete '{model_id}': in use by [{projects}]"
            )));
        }
        self.store.remove(model_id)?;
        Ok(())
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

    // ── RefCounter ────────────────────────────────────────────────────────────

    #[test]
    fn ref_counter_basic() {
        let mut r = RefCounter::new();
        r.add_ref("m1", "project-a");
        assert_eq!(r.ref_count("m1"), 1);
        assert!(!r.can_delete("m1"));

        r.add_ref("m1", "project-b");
        assert_eq!(r.ref_count("m1"), 2);

        r.remove_ref("m1", "project-a");
        assert_eq!(r.ref_count("m1"), 1);

        r.remove_ref("m1", "project-b");
        assert_eq!(r.ref_count("m1"), 0);
        assert!(r.can_delete("m1"));
    }

    #[test]
    fn ref_counter_unknown_model_is_deletable() {
        let r = RefCounter::new();
        assert!(r.can_delete("nonexistent"));
        assert_eq!(r.ref_count("nonexistent"), 0);
    }

    #[test]
    fn all_referenced_ids() {
        let mut r = RefCounter::new();
        r.add_ref("m1", "p1");
        r.add_ref("m2", "p1");
        let ids = r.all_referenced_ids();
        assert!(ids.contains("m1"));
        assert!(ids.contains("m2"));
    }

    #[test]
    fn referencing_projects_list() {
        let mut r = RefCounter::new();
        r.add_ref("m1", "p1");
        r.add_ref("m1", "p2");
        let mut projects = r.referencing_projects("m1");
        projects.sort();
        assert_eq!(projects, vec!["p1", "p2"]);
    }

    // ── GarbageCollector ──────────────────────────────────────────────────────

    #[test]
    fn scan_identifies_unreferenced() {
        let (mut store, _tmp) = make_store();
        store.register(remote_manifest("m1")).unwrap();
        store.register(remote_manifest("m2")).unwrap();

        let mut refs = RefCounter::new();
        refs.add_ref("m1", "project-a");

        let gc = GarbageCollector::new(&mut store);
        let report = gc.scan(&refs);

        assert_eq!(report.unreferenced.len(), 1);
        assert_eq!(report.unreferenced[0].id, "m2");
    }

    #[test]
    fn scan_empty_when_all_referenced() {
        let (mut store, _tmp) = make_store();
        store.register(remote_manifest("m1")).unwrap();

        let mut refs = RefCounter::new();
        refs.add_ref("m1", "project-a");

        let gc = GarbageCollector::new(&mut store);
        let report = gc.scan(&refs);
        assert!(report.unreferenced.is_empty());
        assert_eq!(report.reclaimable_bytes, 0);
    }

    #[test]
    fn clean_removes_unreferenced() {
        let (mut store, _tmp) = make_store();
        store.register(remote_manifest("m1")).unwrap();
        store.register(remote_manifest("m2")).unwrap();

        let refs = RefCounter::new(); // nothing referenced

        let mut gc = GarbageCollector::new(&mut store);
        let freed = gc.clean(&refs).unwrap();
        assert_eq!(freed, 0); // remote models have 0 size
        assert_eq!(store.list().len(), 0);
    }

    #[test]
    fn safe_delete_fails_when_referenced() {
        let (mut store, _tmp) = make_store();
        store.register(remote_manifest("m1")).unwrap();

        let mut refs = RefCounter::new();
        refs.add_ref("m1", "project-a");

        let mut gc = GarbageCollector::new(&mut store);
        let err = gc.safe_delete("m1", &refs).unwrap_err();
        assert!(err.to_string().contains("in use"));
        assert!(store.exists("m1")); // not deleted
    }

    #[test]
    fn safe_delete_succeeds_when_unreferenced() {
        let (mut store, _tmp) = make_store();
        store.register(remote_manifest("m1")).unwrap();

        let refs = RefCounter::new();
        let mut gc = GarbageCollector::new(&mut store);
        gc.safe_delete("m1", &refs).unwrap();
        assert!(!store.exists("m1"));
    }
}
