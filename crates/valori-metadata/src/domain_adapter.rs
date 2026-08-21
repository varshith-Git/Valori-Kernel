// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Adapter between the control-plane record and the canonical domain model.
//!
//! [`crate::project::Project`] is the **persistence** model for the redb
//! control plane. [`valori_domain::Project`] is the **domain** model. This
//! module is the only place the two meet.
//!
//! Nothing here changes the redb schema or any existing call site. The adapter
//! is additive; migrating consumers is step M3.
//!
//! # The identity gap — the reason this is not a `From` impl
//!
//! The control-plane record has **no id**. It is keyed on `name`, as is the
//! legacy TypeScript manifest, while the daemon's `project.json` carries a UUID
//! (`ARCHITECTURE_AUDIT.md` §9, migration risk R2).
//!
//! Rather than invent an id — which would mint a *different* identity every
//! time the same project was read, silently breaking any future local↔cloud
//! correlation — [`record_to_domain`] requires the caller to supply the
//! [`ProjectId`]. That makes the missing information impossible to ignore, and
//! it names exactly what M3 has to solve: resolving `name → ProjectId` through
//! the daemon registry during the compatibility window.
//!
//! # Lossy in both directions, by design
//!
//! | Record-only field | Why it is not domain |
//! |---|---|
//! | `dir` | Location is not identity — it belongs to `LocalProject` |
//! | `port`, `nodes[]` | Runtime allocation; changes on every start |
//! | `mode` | Derived from `topology.is_cluster()`; storing both lets them disagree |
//!
//! | Domain-only field | Why the record lacks it |
//! |---|---|
//! | `id` | See the identity gap above |
//!
//! `dim`/`index` used to appear in both tables (a record-only `u16 dim` +
//! local `IndexKind`, mapped to/from the domain model's own `dim`/`index`).
//! Both were removed from `valori_metadata::Project` and `valori_domain::Project`
//! in the collection-index-lifecycle phase, with no backward-compat shim —
//! Valori has no production users. `index_to_domain`/`index_from_domain`
//! (the conversion between the now-deleted local `IndexKind` and
//! `valori_domain::IndexKind`) were deleted with them.

use valori_domain::{
    DomainError, Project as DomainProject, ProjectId, ProjectName, ProjectTopology, Timestamp,
};

use crate::project::{Project, ProjectMode};

/// Why a control-plane record could not be viewed as a domain project.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProjectAdapterError {
    /// The record's `name` is not filesystem-safe.
    #[error("project name `{name}` is not valid: {source}")]
    InvalidName {
        name: String,
        #[source]
        source: DomainError,
    },

    /// `node_count` or `shard_count` was zero.
    #[error("project `{name}` has an unrepresentable topology: {source}")]
    InvalidTopology {
        name: String,
        #[source]
        source: DomainError,
    },
}

/// View a control-plane record as a canonical domain project.
///
/// `id` must be supplied by the caller — see the module docs on the identity gap.
///
/// # Errors
///
/// See [`ProjectAdapterError`]. Performs no I/O and never panics.
pub fn record_to_domain(
    record: &Project,
    id: ProjectId,
) -> Result<DomainProject, ProjectAdapterError> {
    let name = ProjectName::parse(record.name.clone()).map_err(|source| {
        ProjectAdapterError::InvalidName {
            name: record.name.clone(),
            source,
        }
    })?;

    let topology =
        ProjectTopology::new(record.node_count, record.shard_count).map_err(|source| {
            ProjectAdapterError::InvalidTopology {
                name: record.name.clone(),
                source,
            }
        })?;

    Ok(DomainProject {
        id,
        name,
        topology,
        created_at: Timestamp::from_unix_secs(record.created_at),
        last_opened_at: record.last_opened_at.map(Timestamp::from_unix_secs),
        record_count: record.record_count,
    })
}

/// Apply a domain project's fields onto an existing control-plane record.
///
/// `dir`, `port` and `nodes[]` are left untouched — the domain model does not
/// carry them, and overwriting them would discard live runtime allocation.
///
/// `mode` is recomputed from the topology, which is the point: after this call
/// `mode` and `node_count` cannot disagree.
///
pub fn record_from_domain(
    record: &mut Project,
    project: &DomainProject,
) -> Result<(), ProjectAdapterError> {
    record.name = project.name.to_string();
    record.node_count = project.topology.replicas.get();
    record.shard_count = project.topology.shards.get();
    record.mode = if project.topology.is_cluster() {
        ProjectMode::Cluster
    } else {
        ProjectMode::Standalone
    };
    record.created_at = project.created_at.as_unix_secs();
    record.last_opened_at = project.last_opened_at.map(Timestamp::as_unix_secs);
    record.record_count = project.record_count;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn record() -> Project {
        Project {
            name: "research-notes".to_string(),
            dir: PathBuf::from("/tmp/research-notes"),
            port: 3000,
            shard_count: 1,
            node_count: 1,
            mode: ProjectMode::Standalone,
            created_at: 1_750_000_000,
            last_opened_at: None,
            record_count: Some(42),
            nodes: Vec::new(),
        }
    }

    #[test]
    fn record_becomes_a_domain_project_with_the_supplied_id() {
        let id = ProjectId::new();
        let project = record_to_domain(&record(), id).unwrap();

        assert_eq!(
            project.id, id,
            "identity comes from the caller, not the record"
        );
        assert_eq!(project.name.as_str(), "research-notes");
        assert_eq!(project.record_count, Some(42));
        assert!(!project.topology.is_cluster());
    }

    #[test]
    fn cluster_record_becomes_cluster_topology() {
        let mut r = record();
        r.node_count = 3;
        r.shard_count = 4;
        r.mode = ProjectMode::Cluster;

        let project = record_to_domain(&r, ProjectId::new()).unwrap();
        assert_eq!(project.topology.replicas.get(), 3);
        assert_eq!(project.topology.shards.get(), 4);
        assert!(project.topology.is_cluster());
    }

    #[test]
    fn round_trip_preserves_runtime_fields() {
        let original = record();
        let domain = record_to_domain(&original, ProjectId::new()).unwrap();
        let mut updated = original.clone();
        record_from_domain(&mut updated, &domain).unwrap();

        assert_eq!(updated.dir, original.dir, "location is not domain state");
        assert_eq!(updated.port, original.port, "port is runtime allocation");
        assert_eq!(updated.name, original.name);
        assert_eq!(updated.record_count, original.record_count);
    }

    #[test]
    fn mode_is_recomputed_and_cannot_contradict_node_count() {
        // A record that already disagrees with itself: mode says standalone,
        // node_count says three. The domain model has no `mode`, so writing
        // back repairs the inconsistency instead of preserving it.
        let mut inconsistent = record();
        inconsistent.node_count = 3;
        inconsistent.mode = ProjectMode::Standalone;

        let domain = record_to_domain(&inconsistent, ProjectId::new()).unwrap();
        let mut repaired = inconsistent.clone();
        record_from_domain(&mut repaired, &domain).unwrap();

        assert_eq!(repaired.mode, ProjectMode::Cluster);
        assert_eq!(repaired.node_count, 3);
    }

    // ── Adapter boundary ─────────────────────────────────────────────────────

    #[test]
    fn hostile_record_name_cannot_reach_the_domain_model() {
        // `metadata::Project.name` is a plain String with no validation at all.
        for hostile in ["../../etc/passwd", "a/b", "", "has space"] {
            let mut r = record();
            r.name = hostile.to_string();
            assert!(
                matches!(
                    record_to_domain(&r, ProjectId::new()),
                    Err(ProjectAdapterError::InvalidName { .. })
                ),
                "record name {hostile:?} must be rejected at the adapter"
            );
        }
    }

    #[test]
    fn names_the_daemon_accepts_survive_the_adapter() {
        for existing in ["_scratch", "-tmp", &"a".repeat(64)] {
            let mut r = record();
            r.name = existing.to_string();
            assert!(
                record_to_domain(&r, ProjectId::new()).is_ok(),
                "{existing:?}"
            );
        }
    }

    #[test]
    fn zero_node_count_is_rejected() {
        let mut r = record();
        r.node_count = 0;
        assert!(matches!(
            record_to_domain(&r, ProjectId::new()),
            Err(ProjectAdapterError::InvalidTopology { .. })
        ));
    }
}
