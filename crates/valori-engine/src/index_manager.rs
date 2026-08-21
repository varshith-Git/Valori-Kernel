// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Collection index lifecycle manager.
//!
//! # Design
//!
//! Indexes are **derived acceleration structures** — not authoritative database state.
//! The snapshot + WAL path is the correctness guarantee; the index is an optional
//! speedup. This module manages the lifecycle of those optional structures.
//!
//! # States
//!
//! ```text
//!       NONE ──► BUILDING ──► READY ──► ACTIVE ──► RETIRING
//!                    │                               │
//!                    └───► FAILED                   (drop)
//! ```
//!
//! A collection can have at most one BUILDING generation at a time.
//! The ACTIVE generation always serves searches until atomically replaced.
//! A FAILED build leaves the previous ACTIVE generation unchanged.
//!
//! # Generations
//!
//! Each built index gets a monotonically increasing, collection-scoped
//! generation number. A generation is immutable once created — a parameter
//! change or type change creates a new generation, not an in-place mutate.
//!
//! # Background build
//!
//! Building uses `tokio::task::spawn_blocking` to avoid blocking the async
//! runtime. The build takes a snapshot of the record set at `base_lsn`, then
//! catches up WAL entries that arrived during the build.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// The lifecycle state of one index generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexState {
    /// No dedicated ANN index. Exact namespace-scoped search is used.
    None,
    /// A new generation is being constructed in the background.
    /// The active generation (if any) continues to serve.
    Building,
    /// Construction is complete and validated; not yet atomically swapped in.
    Ready,
    /// This generation is currently serving searches.
    Active,
    /// Construction failed. The previous active generation is unaffected.
    Failed,
    /// Superseded — will be cleaned up once no recovery window requires it.
    Retiring,
}

impl std::fmt::Display for IndexState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexState::None => write!(f, "none"),
            IndexState::Building => write!(f, "building"),
            IndexState::Ready => write!(f, "ready"),
            IndexState::Active => write!(f, "active"),
            IndexState::Failed => write!(f, "failed"),
            IndexState::Retiring => write!(f, "retiring"),
        }
    }
}

/// The type and parameters of one index generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexSpec {
    /// `"hnsw"`, `"ivf"`, or `"bq"`. Not `"brute"` — NONE state covers that.
    pub index_type: String,
    /// Serialized parameters — currently only HNSW m/ef_construction/ef_search
    /// and IVF n_list/n_probe. An empty map means "use node defaults".
    #[serde(default)]
    pub parameters: serde_json::Value,
}

impl IndexSpec {
    pub fn hnsw_defaults() -> Self {
        Self {
            index_type: "hnsw".into(),
            parameters: serde_json::json!({}),
        }
    }
    pub fn ivf_defaults() -> Self {
        Self {
            index_type: "ivf".into(),
            parameters: serde_json::json!({}),
        }
    }
    pub fn bq_defaults() -> Self {
        Self {
            index_type: "bq".into(),
            parameters: serde_json::json!({}),
        }
    }
}

/// Metadata for one built or in-progress index generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexGeneration {
    /// Monotonically increasing, collection-scoped.
    pub generation: u32,
    /// The index type + parameters this generation was built with.
    pub spec: IndexSpec,
    /// The event-log height (LSN) at which the build snapshot was taken.
    /// All records present at this height are in the index.
    pub base_lsn: u64,
    /// Unix seconds when the build was requested.
    pub started_at: u64,
    /// Unix seconds when the build completed (READY/ACTIVE/FAILED).
    pub completed_at: Option<u64>,
    /// Human-readable failure reason, set on FAILED.
    pub error: Option<String>,
}

impl IndexGeneration {
    pub fn new(generation: u32, spec: IndexSpec, base_lsn: u64) -> Self {
        Self {
            generation,
            spec,
            base_lsn,
            started_at: now_unix(),
            completed_at: None,
            error: None,
        }
    }
}

/// Lifecycle state for all index generations of one collection.
///
/// Invariants:
/// - At most one generation is BUILDING at a time.
/// - A generation is ACTIVE if it is currently serving searches.
/// - ACTIVE and BUILDING can coexist: the active index serves while a new one builds.
/// - A FAILED generation leaves the active one untouched.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CollectionIndexState {
    /// The spec the user wants, even if a build hasn't started yet or is in progress.
    pub desired: Option<IndexSpec>,
    /// Currently serving generation (if any).
    pub active_generation: Option<u32>,
    /// Generation currently building (if any).
    pub building_generation: Option<u32>,
    /// All known generations (active, retired, failed, building).
    pub generations: Vec<(u32, IndexState, IndexGeneration)>,
    /// Next generation id to allocate.
    pub next_generation: u32,
}

impl CollectionIndexState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the metadata for `gen`, if known.
    pub fn get_generation(&self, gen: u32) -> Option<&IndexGeneration> {
        self.generations
            .iter()
            .find(|(g, _, _)| *g == gen)
            .map(|(_, _, meta)| meta)
    }

    /// Returns the state for `gen`, if known.
    pub fn get_state(&self, gen: u32) -> Option<&IndexState> {
        self.generations
            .iter()
            .find(|(g, _, _)| *g == gen)
            .map(|(_, state, _)| state)
    }

    /// Allocate a new generation id, add it in BUILDING state.
    pub fn start_build(&mut self, spec: IndexSpec, base_lsn: u64) -> u32 {
        let gen = self.next_generation;
        self.next_generation += 1;
        let meta = IndexGeneration::new(gen, spec, base_lsn);
        self.generations.push((gen, IndexState::Building, meta));
        self.building_generation = Some(gen);
        gen
    }

    /// Mark a BUILDING generation as READY.
    pub fn mark_ready(&mut self, gen: u32) {
        if let Some((_, state, meta)) = self.generations.iter_mut().find(|(g, _, _)| *g == gen) {
            if *state == IndexState::Building {
                *state = IndexState::Ready;
                meta.completed_at = Some(now_unix());
            }
        }
    }

    /// Atomically promote READY generation to ACTIVE; retire the previous ACTIVE.
    /// Returns the previous active generation (now RETIRING), if any.
    pub fn activate(&mut self, gen: u32) -> Option<u32> {
        // Retire old active
        let old_active = self.active_generation;
        if let Some(old) = old_active {
            if let Some((_, state, _)) = self.generations.iter_mut().find(|(g, _, _)| *g == old) {
                *state = IndexState::Retiring;
            }
        }
        // Promote new generation
        if let Some((_, state, _)) = self.generations.iter_mut().find(|(g, _, _)| *g == gen) {
            *state = IndexState::Active;
        }
        self.active_generation = Some(gen);
        self.building_generation = None;
        old_active
    }

    /// Mark a BUILDING generation as FAILED (preserves any current ACTIVE).
    pub fn mark_failed(&mut self, gen: u32, reason: String) {
        if let Some((_, state, meta)) = self.generations.iter_mut().find(|(g, _, _)| *g == gen) {
            *state = IndexState::Failed;
            meta.error = Some(reason);
            meta.completed_at = Some(now_unix());
        }
        if self.building_generation == Some(gen) {
            self.building_generation = None;
        }
    }

    /// Remove NONE — transition: drop any active index, return to brute-force.
    pub fn set_none(&mut self) {
        // Retire the current active
        if let Some(old) = self.active_generation.take() {
            if let Some((_, state, _)) = self.generations.iter_mut().find(|(g, _, _)| *g == old) {
                *state = IndexState::Retiring;
            }
        }
        self.desired = None;
        self.building_generation = None;
    }

    /// Whether a build is currently in progress.
    pub fn is_building(&self) -> bool {
        self.building_generation.is_some()
    }

    /// The index type string of the active generation ("hnsw", "ivf", "bq", "none").
    pub fn active_type(&self) -> &str {
        if let Some(gen) = self.active_generation {
            if let Some(meta) = self.get_generation(gen) {
                return &meta.spec.index_type;
            }
        }
        "none"
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// The tuning knobs `POST /v1/namespaces/{name}/index` actually reads.
///
/// Phase API-3.3: [`IndexBuildRequest::parameters`] is a `serde_json::Value`,
/// which utoipa rendered as a schema with no `type` at all — `unknown` in
/// TypeScript, `Any` in Python, and nothing whatsoever for a user to discover
/// the knob names from. It was the only genuinely untyped field in the public
/// surface.
///
/// The runtime is not actually open-ended: both routers read exactly five
/// keys, all unsigned integers — `m`, `ef_construction`, `ef_search` for HNSW
/// (`server.rs` / `cluster_server.rs`, the `"hnsw"` arm) and `n_list`,
/// `n_probe` for IVF (the `"ivf"` arm). This type names them.
///
/// `additionalProperties` stays open because the documented behaviour is that
/// unknown keys are *ignored*, not rejected — so a client sending one is not
/// making an error, and the schema must not claim otherwise.
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexBuildParameters {
    /// HNSW: neighbours per node. `m_max0` is derived as `2 * m`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub m: Option<u64>,
    /// HNSW: candidate-list size during construction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ef_construction: Option<u64>,
    /// HNSW: candidate-list size during search.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ef_search: Option<u64>,
    /// IVF: centroid count. Omit to auto-scale to `max(16, sqrt(N))`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n_list: Option<u64>,
    /// IVF: probe count. Omit to auto-scale to `max(1, sqrt(n_list))`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n_probe: Option<u64>,
}

/// The index kinds `POST /v1/namespaces/{name}/index` will actually build.
///
/// Phase API-3.3: narrower than the project-wide `IndexKindInput`, and
/// deliberately so. The build task in both routers matches on exactly three
/// strings — `"hnsw"`, `"ivf"`, `"bq"` — and its `_` arm returns
/// `"unknown index type '<x>'"`. `brute` and `auto` are project-level
/// selections, not buildable per-collection ANN structures, so they are not
/// members here; sending one is an error, and the schema now says so instead
/// of advertising an open `string`.
///
/// `null` is the fourth valid value and means *drop the index*. It is carried
/// by the `Option`, not by a variant.
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildableIndexKind {
    /// Hierarchical navigable small world. Tuned by `m`, `ef_construction`, `ef_search`.
    Hnsw,
    /// Inverted file index. Tuned by `n_list`, `n_probe`.
    Ivf,
    /// Binary-quantised index. Takes no parameters.
    Bq,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
/// The request body for `POST /v1/namespaces/{name}/index`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexBuildRequest {
    /// `"hnsw"`, `"ivf"`, `"bq"`, or `null` (drop the index).
    #[serde(rename = "type")]
    #[cfg_attr(feature = "utoipa", schema(value_type = Option<BuildableIndexKind>))]
    pub index_type: Option<String>,
    /// Optional parameter overrides. Only the parameters the implementation
    /// actually reads are used; unknown keys are ignored.
    ///
    /// Stays a `serde_json::Value` at runtime — the handlers index into it by
    /// key and tolerate anything — but is *described* as
    /// [`IndexBuildParameters`] so a generated SDK can offer the real knobs.
    #[serde(default)]
    #[cfg_attr(feature = "utoipa", schema(value_type = IndexBuildParameters))]
    pub parameters: serde_json::Value,
}

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
/// Response to `POST /v1/namespaces/{name}/index` and
/// `GET /v1/namespaces/{name}/index`.
///
/// # Cluster vs standalone distinction
///
/// In cluster mode, `desired_type` is always populated from the Raft-
/// replicated desired spec (what the cluster wants), while `active_type`
/// and `status` reflect this **node's local** build state. They may
/// differ temporarily as builds propagate across replicas.
///
/// Example during a transition:
/// ```json
/// { "desired_type": "ivf", "active_type": "hnsw", "status": "building",
///   "building_generation": 2, "active_generation": 1 }
/// ```
///
/// In standalone mode, `desired_type` is always equal to `active_type` once
/// a build completes (there's only one node).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStatusResponse {
    pub collection: String,
    /// The currently serving index type ("hnsw", "ivf", "bq", "none").
    pub active_type: String,
    /// The active generation number, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_generation: Option<u32>,
    /// The type the user requested (may differ from active while building).
    /// In cluster mode, this comes from the Raft-replicated desired spec and
    /// is authoritative for the whole cluster, not just the responding node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired_type: Option<String>,
    /// Current lifecycle status of the active or building generation.
    pub status: String,
    /// If a build is in progress, its generation number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub building_generation: Option<u32>,
    /// The base LSN of the building generation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_lsn: Option<u64>,
    /// Unix seconds when the current build started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_started_at: Option<u64>,
    /// Human-readable failure reason, if the last build failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl IndexStatusResponse {
    pub fn from_state(collection: &str, state: &CollectionIndexState) -> Self {
        let active_type = state.active_type().to_string();
        let active_generation = state.active_generation;
        let desired_type = state.desired.as_ref().map(|s| s.index_type.clone());
        let building_generation = state.building_generation;

        // Determine overall status string
        let status = if let Some(bg) = building_generation {
            if let Some(st) = state.get_state(bg) {
                st.to_string()
            } else {
                "building".to_string()
            }
        } else if active_generation.is_some() {
            "active".to_string()
        } else {
            // Check if last generation failed
            let last_failed = state
                .generations
                .iter()
                .rev()
                .find(|(_, st, _)| *st == IndexState::Failed)
                .map(|(_, _, meta)| meta);
            if last_failed.is_some() {
                "failed".to_string()
            } else {
                "none".to_string()
            }
        };

        // Build progress metadata
        let (base_lsn, build_started_at, error) = if let Some(bg) = building_generation {
            if let Some(meta) = state.get_generation(bg) {
                (Some(meta.base_lsn), Some(meta.started_at), None)
            } else {
                (None, None, None)
            }
        } else {
            // Check for most recent failure
            let last_failed = state
                .generations
                .iter()
                .rev()
                .find(|(_, st, _)| *st == IndexState::Failed);
            if let Some((_, _, meta)) = last_failed {
                (None, None, meta.error.clone())
            } else {
                (None, None, None)
            }
        };

        Self {
            collection: collection.to_string(),
            active_type,
            active_generation,
            desired_type,
            status,
            building_generation,
            base_lsn,
            build_started_at,
            error,
        }
    }
}
