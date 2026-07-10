// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! PlanningContext and PlannerFingerprint.
//!
//! Both are deterministically hashable typed structs — no `HashMap`, no
//! timestamps, no random IDs. This guarantees that equal inputs always produce
//! equal hashes (RFC-0001 §3.3, invariant I-03).
use serde::{Deserialize, Serialize};

// ── PlannerFingerprint ────────────────────────────────────────────────────────

/// A stable digest of the Planner's behavioral configuration.
///
/// Changes when any behavioral aspect of the Planner changes: routing logic,
/// feature flags, metadata schema version. A cached `ExecutionGraph` is reusable
/// only when the `PlannerFingerprint.hash` matches exactly.
///
/// `hash = BLAKE3(version_str ‖ routing_config_hash ‖ feature_flags_hash ‖ metadata_schema_version_le)`
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlannerFingerprint {
    pub version: String,
    pub routing_config_hash: [u8; 32],
    pub feature_flags_hash: [u8; 32],
    pub metadata_schema_version: u32,
    /// Pre-computed: `BLAKE3(version ‖ routing_config_hash ‖ feature_flags_hash ‖ metadata_schema_version_le)`.
    pub hash: [u8; 32],
}

impl PlannerFingerprint {
    /// Compute a new `PlannerFingerprint` from its components.
    pub fn compute(
        version: impl Into<String>,
        routing_config_hash: [u8; 32],
        feature_flags_hash: [u8; 32],
        metadata_schema_version: u32,
    ) -> Self {
        let version = version.into();
        let mut hasher = blake3::Hasher::new();
        hasher.update(version.as_bytes());
        hasher.update(&routing_config_hash);
        hasher.update(&feature_flags_hash);
        hasher.update(&metadata_schema_version.to_le_bytes());
        let hash = *hasher.finalize().as_bytes();
        PlannerFingerprint { version, routing_config_hash, feature_flags_hash, metadata_schema_version, hash }
    }

    pub fn hash_hex(&self) -> String {
        self.hash.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

// ── PlanningContext ───────────────────────────────────────────────────────────

/// The fully-typed context provided to the Planner alongside an `Operation`.
///
/// Must be deterministically serializable: no `HashMap`, no wall-clock time,
/// no random values. All fields that influence the task graph topology must be
/// captured here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanningContext {
    /// Which capabilities are active on the node right now.
    pub capability_set: CapabilitySet,
    /// Metadata schema version — bumped when `valori-metadata` types change.
    pub schema_version: u32,
    /// Shard count at planning time. Graph may differ if topology changes.
    pub shard_count: u8,
    /// Cluster epoch — bumped on membership changes.
    pub cluster_epoch: u64,
    /// Whether the node is running in cluster mode.
    pub cluster_mode: bool,
}

/// A bitmask of capabilities active on the node at planning time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySet {
    pub embed: bool,
    pub llm: bool,
    pub object_store: bool,
    pub cluster: bool,
    pub shard_count: u8,
}

/// Content-addressed hash of a `PlanningContext`.
/// `PlanningContextHash = BLAKE3(bincode(context))`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlanningContextHash(pub [u8; 32]);

impl PlanningContextHash {
    pub fn compute(ctx: &PlanningContext) -> Self {
        let bytes = bincode::serde::encode_to_vec(ctx, bincode::config::standard())
            .expect("PlanningContext must be deterministically encodable");
        let hash = blake3::hash(&bytes);
        PlanningContextHash(*hash.as_bytes())
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> PlanningContext {
        PlanningContext {
            capability_set: CapabilitySet { embed: true, llm: false, object_store: false, cluster: false, shard_count: 1 },
            schema_version: 1,
            shard_count: 1,
            cluster_epoch: 0,
            cluster_mode: false,
        }
    }

    #[test]
    fn context_hash_is_deterministic() {
        let c = ctx();
        let h1 = PlanningContextHash::compute(&c);
        let h2 = PlanningContextHash::compute(&c);
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_context_different_hash() {
        let mut c2 = ctx();
        c2.schema_version = 2;
        assert_ne!(PlanningContextHash::compute(&ctx()), PlanningContextHash::compute(&c2));
    }

    #[test]
    fn fingerprint_compute() {
        let fp = PlannerFingerprint::compute("0.2.4", [1u8; 32], [2u8; 32], 1);
        assert_ne!(fp.hash, [0u8; 32]);

        let fp2 = PlannerFingerprint::compute("0.2.4", [1u8; 32], [2u8; 32], 1);
        assert_eq!(fp.hash, fp2.hash);

        let fp3 = PlannerFingerprint::compute("0.2.5", [1u8; 32], [2u8; 32], 1);
        assert_ne!(fp.hash, fp3.hash);
    }
}
