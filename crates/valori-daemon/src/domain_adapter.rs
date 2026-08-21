// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Adapter between the daemon's persistence model and the canonical domain model.
//!
//! [`ProjectManifest`] is the daemon's **persistence** model — the exact shape
//! of `project.json` on disk. [`valori_domain::Project`] is the **domain**
//! model — what a project means, shared with the node, the API, Studio and
//! Cloud. This module is the only place the two meet.
//!
//! Nothing here changes `project.json`. The adapter is purely additive: the
//! daemon continues to read and write manifests exactly as before, and callers
//! opt in to the domain view where they want it. Migrating call sites is step
//! M3; this is M2.
//!
//! # The conversion is deliberately lossy in both directions
//!
//! That is what makes it an adapter rather than a rename:
//!
//! | Manifest-only field | Why it does not belong in the domain model |
//! |---|---|
//! | `workspace` | A daemon-local grouping; the node and Cloud have no such concept |
//! | `restart_policy` | Operational policy of *this daemon's* copy |
//! | `embedding` | Provider config, and its `api_key_ref` is a secret handle |
//! | `storage` | Deployment configuration |
//! | `cluster.nodes[]` | Port allocations — runtime state that changes each start |
//! | `dim`, `index` | **Removed from `valori_domain::Project` entirely** (collection-index-lifecycle phase) — vector configuration is Collection-scoped, not Project-scoped. `ProjectManifest.dim`/`.index` remain on disk for now (still what spawns a local node's `VALORI_DIM`/`VALORI_INDEX`, per that phase's Phase 20, not yet reached) but this adapter no longer reads or writes either field — there is nothing on the domain side to map them to or from. |
//!
//! | Domain-only field | Where the manifest keeps it |
//! |---|---|
//! | `record_count` | Not persisted by the daemon at all — the node reports it |
//!
//! Because of that asymmetry, [`manifest_from_domain`] takes the existing
//! manifest to update. There is no `From<&Project> for ProjectManifest`: a
//! domain project genuinely does not contain enough information to construct
//! one, and an impl that silently defaulted `workspace` and `restart_policy`
//! would be a data-loss bug waiting to happen.

use std::str::FromStr;

use valori_domain::{
    DomainError, Project as DomainProject, ProjectId, ProjectName, ProjectTopology, Timestamp,
};

use crate::project::{ClusterConfig, ProjectManifest};

/// Why a manifest could not be viewed as a domain project.
///
/// Every variant means the manifest on disk holds a value the domain model
/// refuses to represent — which is the point of validating at the boundary
/// rather than discovering it three layers up.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProjectAdapterError {
    /// The manifest's `id` was not a UUID.
    ///
    /// Manifests written by `crate::new_id()` always are. A manifest that
    /// fails here was hand-edited or produced by a foreign writer.
    #[error("project `{name}` has a malformed id: {source}")]
    MalformedId {
        name: String,
        #[source]
        source: DomainError,
    },

    /// The manifest's `name` is not filesystem-safe.
    #[error("project name `{name}` is not valid: {source}")]
    InvalidName {
        name: String,
        #[source]
        source: DomainError,
    },

    /// Replica or shard counts were zero, or the shard count exceeded 255.
    ///
    /// `ClusterConfig::shard_count` is a `u32` on disk while the domain model
    /// uses a `u8`. A value above 255 is not a topology Valori supports; it is
    /// rejected rather than silently truncated.
    #[error("project `{name}` has an unrepresentable topology: {reason}")]
    InvalidTopology { name: String, reason: String },

    /// A topology transition this adapter refuses to perform silently.
    #[error("project `{name}`: unsupported topology change — {reason}")]
    UnsupportedTopologyChange { name: String, reason: &'static str },
}

/// View a persisted manifest as a canonical domain project.
///
/// `record_count` is always `None`: the daemon does not persist it. Callers
/// that want it ask the node.
///
/// # Errors
///
/// See [`ProjectAdapterError`]. This function performs no I/O and never panics.
pub fn manifest_to_domain(
    manifest: &ProjectManifest,
) -> Result<DomainProject, ProjectAdapterError> {
    let name = ProjectName::parse(manifest.name.clone()).map_err(|source| {
        ProjectAdapterError::InvalidName {
            name: manifest.name.clone(),
            source,
        }
    })?;

    let id =
        ProjectId::from_str(&manifest.id).map_err(|source| ProjectAdapterError::MalformedId {
            name: manifest.name.clone(),
            source,
        })?;

    let topology = topology_from_cluster(manifest.cluster.as_ref(), &manifest.name)?;

    Ok(DomainProject {
        id,
        name,
        topology,
        created_at: Timestamp::from_unix_secs(manifest.created_at),
        last_opened_at: manifest.last_opened_at.map(Timestamp::from_unix_secs),
        record_count: None,
    })
}

/// Apply a domain project's fields onto an existing manifest.
///
/// Only the fields the domain model owns are written. `workspace`,
/// `restart_policy`, `embedding`, `storage` and `cluster.nodes[]` are left
/// exactly as they were — this function cannot lose them.
///
/// For a cluster topology the `cluster` block is created if absent, and only
/// `replication` / `shard_count` are updated so existing port allocations in
/// `nodes[]` survive.
///
/// # Cluster demotion is rejected, not silently ignored
///
/// An earlier revision simply skipped the cluster block for a standalone
/// topology. That left a stale `cluster: Some { replication: 3 }` on disk, so
/// the write appeared to succeed and the next read reported three replicas
/// again (review finding F4). Clearing the block instead would discard the
/// node port allocations, which is data loss.
///
/// Neither is acceptable silently, so the transition is an error. A caller that
/// genuinely intends to demote a cluster must clear `manifest.cluster` itself,
/// having decided what happens to the allocations.
///
/// # Errors
///
/// [`ProjectAdapterError::UnsupportedTopologyChange`] on cluster → standalone.
pub fn manifest_from_domain(
    manifest: &mut ProjectManifest,
    project: &DomainProject,
) -> Result<(), ProjectAdapterError> {
    if manifest.cluster.is_some() && !project.topology.is_cluster() {
        return Err(ProjectAdapterError::UnsupportedTopologyChange {
            name: manifest.name.clone(),
            reason: "cannot demote a cluster project to standalone: the manifest's \
                     cluster block holds node port allocations that this adapter \
                     must not discard. Clear `manifest.cluster` explicitly first.",
        });
    }

    // `dim`/`index` are deliberately NOT written here — they no longer exist
    // on the domain model (vector configuration is Collection-scoped). The
    // manifest's own `dim`/`index` fields are left exactly as they were.
    manifest.id = project.id.to_string();
    manifest.name = project.name.to_string();
    manifest.created_at = project.created_at.as_unix_secs();
    manifest.last_opened_at = project.last_opened_at.map(Timestamp::as_unix_secs);

    if project.topology.is_cluster() {
        let cluster = manifest.cluster.get_or_insert_with(|| ClusterConfig {
            replication: project.topology.replicas.get(),
            nodes: Vec::new(),
            shard_count: u32::from(project.topology.shards.get()),
        });
        cluster.replication = project.topology.replicas.get();
        cluster.shard_count = u32::from(project.topology.shards.get());
    }

    Ok(())
}

/// Derive a domain topology from the manifest's optional cluster block.
///
/// `None` means standalone — one replica, one shard. This is the daemon's
/// encoding; the domain model has no nullable topology.
fn topology_from_cluster(
    cluster: Option<&ClusterConfig>,
    name: &str,
) -> Result<ProjectTopology, ProjectAdapterError> {
    let (replicas, shards) = match cluster {
        None => (1u8, 1u8),
        Some(c) => {
            let shards =
                u8::try_from(c.shard_count).map_err(|_| ProjectAdapterError::InvalidTopology {
                    name: name.to_string(),
                    reason: format!(
                        "shard_count {} exceeds the supported maximum of 255",
                        c.shard_count
                    ),
                })?;
            (c.replication, shards)
        }
    };

    ProjectTopology::new(replicas, shards).map_err(|e| ProjectAdapterError::InvalidTopology {
        name: name.to_string(),
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{JsonProjectStore, ProjectNode};

    fn manifest() -> ProjectManifest {
        ProjectManifest {
            id: "7c9e6679-7425-40de-944b-e07fc1f90ae7".to_string(),
            name: "research-notes".to_string(),
            dim: Some(384),
            index: Some("hnsw".to_string()),
            workspace: "default".to_string(),
            restart_policy: Default::default(),
            created_at: 1_750_000_000,
            last_opened_at: Some(1_750_000_500),
            cluster: None,
            embedding: Default::default(),
            storage: Default::default(),
        }
    }

    #[test]
    fn standalone_manifest_becomes_a_standalone_domain_project() {
        let project = manifest_to_domain(&manifest()).unwrap();

        assert_eq!(project.name.as_str(), "research-notes");
        assert_eq!(project.topology, ProjectTopology::STANDALONE);
        assert!(!project.topology.is_cluster());
        assert_eq!(project.created_at.as_unix_secs(), 1_750_000_000);
        assert_eq!(project.record_count, None, "the daemon never persists this");
    }

    #[test]
    fn cluster_block_becomes_topology() {
        let mut m = manifest();
        m.cluster = Some(ClusterConfig {
            replication: 3,
            nodes: vec![ProjectNode {
                id: 1,
                http_port: 4010,
                raft_port: Some(4110),
            }],
            shard_count: 4,
        });

        let project = manifest_to_domain(&m).unwrap();
        assert_eq!(project.topology.replicas.get(), 3);
        assert_eq!(project.topology.shards.get(), 4);
        assert!(project.topology.is_cluster());
    }

    #[test]
    fn round_trip_preserves_the_daemon_only_fields() {
        let original = {
            let mut m = manifest();
            m.workspace = "research".to_string();
            m.embedding.provider = Some("ollama".to_string());
            m.cluster = Some(ClusterConfig {
                replication: 3,
                nodes: vec![ProjectNode {
                    id: 2,
                    http_port: 4011,
                    raft_port: Some(4111),
                }],
                shard_count: 2,
            });
            m
        };

        let domain = manifest_to_domain(&original).unwrap();
        let mut updated = original.clone();
        manifest_from_domain(&mut updated, &domain).unwrap();

        assert_eq!(
            updated, original,
            "a domain round-trip must not perturb the manifest"
        );
        // The specific fields the domain model cannot see:
        assert_eq!(updated.workspace, "research");
        assert_eq!(updated.embedding.provider.as_deref(), Some("ollama"));
        assert_eq!(
            updated.cluster.as_ref().unwrap().nodes[0].http_port,
            4011,
            "port allocations must survive — they are runtime state the domain \
             model deliberately does not carry"
        );
    }

    // ── Adapter boundary: invariants must hold on data read from disk ────────

    #[test]
    fn hostile_manifest_name_cannot_reach_the_domain_model() {
        // `ProjectManifest.name` is a plain String, so a hand-edited or
        // foreign-written project.json can hold anything. The adapter is the
        // boundary that stops it.
        for hostile in ["../../etc/passwd", "../..", "a/b", "has space", ""] {
            let mut m = manifest();
            m.name = hostile.to_string();
            assert!(
                matches!(
                    manifest_to_domain(&m),
                    Err(ProjectAdapterError::InvalidName { .. })
                ),
                "manifest name {hostile:?} must be rejected at the adapter"
            );
        }
    }

    #[test]
    fn names_the_daemon_accepts_survive_the_adapter() {
        // The F2 regression guard: these names exist on disk today because
        // `ProjectStore::is_valid_name` accepts them. If the adapter rejects
        // one, `ProjectStore::list()` silently drops that project.
        for existing in ["_scratch", "-tmp", &"a".repeat(64)] {
            let mut m = manifest();
            m.name = existing.to_string();
            assert!(
                JsonProjectStore::is_valid_name(existing),
                "precondition: the daemon accepts {existing:?}"
            );
            assert!(
                manifest_to_domain(&m).is_ok(),
                "adapter must represent the daemon-created name {existing:?}"
            );
        }
    }

    #[test]
    fn cluster_demotion_is_rejected_rather_than_silently_dropped() {
        // F4: previously the cluster block was simply skipped for a standalone
        // topology, leaving a stale `replication: 3` on disk so the next read
        // reported a cluster again.
        let mut m = manifest();
        m.cluster = Some(ClusterConfig {
            replication: 3,
            nodes: vec![ProjectNode {
                id: 1,
                http_port: 4010,
                raft_port: Some(4110),
            }],
            shard_count: 2,
        });

        let mut standalone = manifest_to_domain(&m).unwrap();
        standalone.topology = ProjectTopology::STANDALONE;

        let err = manifest_from_domain(&mut m, &standalone).unwrap_err();
        assert!(matches!(
            err,
            ProjectAdapterError::UnsupportedTopologyChange { .. }
        ));
        assert_eq!(
            m.cluster.as_ref().unwrap().replication,
            3,
            "the manifest must be left untouched when the change is refused"
        );
        assert_eq!(m.cluster.as_ref().unwrap().nodes.len(), 1);
    }

    #[test]
    fn standalone_promotion_to_cluster_is_allowed() {
        let mut m = manifest();
        assert!(m.cluster.is_none());

        let mut promoted = manifest_to_domain(&m).unwrap();
        promoted.topology = ProjectTopology::new(3, 2).unwrap();

        manifest_from_domain(&mut m, &promoted).unwrap();
        let cluster = m.cluster.as_ref().unwrap();
        assert_eq!(cluster.replication, 3);
        assert_eq!(cluster.shard_count, 2);
    }

    // `oversized_dimension_is_rejected_not_saturated` and `unknown_index_is_rejected`
    // were removed here: the domain model no longer carries `dim`/`index` at
    // all (collection-index-lifecycle phase), so this adapter has nothing to
    // reject them into. `ProjectManifest.dim`/`.index` themselves are
    // untouched by this adapter now — see the module doc's field table.

    #[test]
    fn malformed_id_is_rejected_not_silently_replaced() {
        let mut m = manifest();
        m.id = "not-a-uuid".to_string();
        assert!(matches!(
            manifest_to_domain(&m),
            Err(ProjectAdapterError::MalformedId { .. })
        ));
    }

    #[test]
    fn oversized_shard_count_is_rejected_not_truncated() {
        let mut m = manifest();
        m.cluster = Some(ClusterConfig {
            replication: 3,
            nodes: Vec::new(),
            shard_count: 300,
        });
        assert!(matches!(
            manifest_to_domain(&m),
            Err(ProjectAdapterError::InvalidTopology { .. })
        ));
    }

    #[test]
    fn zero_replication_is_rejected() {
        let mut m = manifest();
        m.cluster = Some(ClusterConfig {
            replication: 0,
            nodes: Vec::new(),
            shard_count: 1,
        });
        assert!(matches!(
            manifest_to_domain(&m),
            Err(ProjectAdapterError::InvalidTopology { .. })
        ));
    }
}
