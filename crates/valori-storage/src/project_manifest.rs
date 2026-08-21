// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `ProjectManifest` — the durable, project-level root of storage discovery
//! (Phase 2.2, collection-storage-runtime-integration).
//!
//! # What this is, and what it deliberately excludes
//!
//! This is the top of the discovery hierarchy the spec's final
//! architectural rule describes:
//! `StorageProvider → ProjectManifest → shard metadata → CollectionManifests
//! → CollectionSnapshots → WAL`. It answers "does this project exist, and
//! what's its deployment/storage shape" — never "what are its collections'
//! dimensions." Per Phase 1 (and re-affirmed here, deliberately): **this
//! type must never gain a `dimension`/`metric`/`index` field.** Those are
//! `CollectionManifest`'s job. Reuses `valori_domain::{ProjectId,
//! ProjectName, ProjectTopology}` — the same canonical types
//! `valori_domain::Project` already uses — rather than inventing a second
//! project-identity type.

use serde::{Deserialize, Serialize};
use valori_domain::{ProjectId, ProjectName, ProjectTopology};

use crate::provider::{StorageError, StorageKey};

pub const PROJECT_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// The durable, project-level manifest. Discovery root: a caller reads this
/// first (`StorageKey::ProjectManifest`) before ever asking for a
/// collection manifest — see `valori_state::collection_bootstrap::discover_project`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub schema_version: u32,
    pub project_id: ProjectId,
    pub name: ProjectName,
    /// Replica/shard counts — deployment topology, not vector configuration
    /// (Phase 1's ownership boundary, reused verbatim).
    pub topology: ProjectTopology,
    pub created_at_unix: u64,
}

impl ProjectManifest {
    pub fn new(
        project_id: ProjectId,
        name: ProjectName,
        topology: ProjectTopology,
        created_at_unix: u64,
    ) -> Self {
        Self {
            schema_version: PROJECT_MANIFEST_SCHEMA_VERSION,
            project_id,
            name,
            topology,
            created_at_unix,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("ProjectManifest serialization is infallible")
    }

    pub fn decode(key: &StorageKey, bytes: &[u8]) -> Result<Self, StorageError> {
        let manifest: Self =
            serde_json::from_slice(bytes).map_err(|e| StorageError::InvalidManifest {
                key: key.clone(),
                reason: format!("malformed JSON: {e}"),
            })?;
        if manifest.schema_version > PROJECT_MANIFEST_SCHEMA_VERSION {
            return Err(StorageError::UnsupportedVersion {
                key: key.clone(),
                version: manifest.schema_version,
            });
        }
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn sample() -> ProjectManifest {
        ProjectManifest::new(
            ProjectId::new(),
            ProjectName::from_str("demo").unwrap(),
            ProjectTopology::STANDALONE,
            1_000,
        )
    }

    #[test]
    fn encode_decode_roundtrip() {
        let m = sample();
        let key = StorageKey::ProjectManifest {
            project_id: m.project_id,
        };
        let bytes = m.encode();
        assert_eq!(ProjectManifest::decode(&key, &bytes).unwrap(), m);
    }

    #[test]
    fn decode_rejects_corrupt_json() {
        let key = StorageKey::ProjectManifest {
            project_id: ProjectId::new(),
        };
        assert!(ProjectManifest::decode(&key, b"{not json").is_err());
    }

    #[test]
    fn decode_rejects_newer_schema_version() {
        let key = StorageKey::ProjectManifest {
            project_id: ProjectId::new(),
        };
        let future = format!(
            r#"{{"schema_version":99,"project_id":"{}","name":"demo","topology":{{"replicas":1,"shards":1}},"created_at_unix":0}}"#,
            ProjectId::new()
        );
        let err = ProjectManifest::decode(&key, future.as_bytes()).unwrap_err();
        assert!(matches!(err, StorageError::UnsupportedVersion { .. }));
    }

    #[test]
    fn manifest_never_carries_vector_configuration_fields() {
        // Structural guard, not just a doc comment: serialize and assert
        // the forbidden keys are genuinely absent from the wire shape.
        let json = serde_json::to_value(sample()).unwrap();
        for forbidden in ["dimension", "dim", "metric", "index"] {
            assert!(
                json.get(forbidden).is_none(),
                "ProjectManifest must never carry {forbidden} — that is CollectionManifest's job"
            );
        }
    }
}
