// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Collection — a named, isolated namespace of records within a project.
//!
//! Collections map a human-readable name (e.g. "research-paper") to a
//! `NamespaceId` (u16) used by the kernel for record isolation and shard routing.
//! `shard_for_namespace(namespace_id, shard_count) = namespace_id % shard_count`.
//!
//! # Ownership model (collection-index-lifecycle phase)
//!
//! Three concepts, deliberately not collapsed into one:
//!
//! - **Vector configuration** (`CollectionVectorConfig`: `dim`, `metric`) —
//!   what the data *is*. Required at creation, immutable forever.
//! - **Desired index** (`CollectionRegistry.index_kind`) — which ANN
//!   algorithm, if any, the user asked for. Optional, and (once Phase 5
//!   lands) mutable.
//! - **Index runtime state** (NONE/BUILDING/READY/ACTIVE/FAILED) — does not
//!   exist yet. `index_kind`'s presence in the map today only means "Engine
//!   built exactly one dedicated index for this namespace, once, at creation
//!   time" — there is no lifecycle, no rebuild, no swap. See
//!   `docs/phases/phase-collection-index-lifecycle.md` for what Phase 5
//!   replaces this stopgap with.
use serde::{Deserialize, Serialize};

/// The maximum number of collections per project, matching `MAX_NAMESPACES`.
pub const MAX_COLLECTIONS: u16 = 1024;

/// A collection's vector configuration — dimension and metric ONLY.
///
/// Both fields are **required** at collection creation and **immutable**
/// forever after. Index selection is a separate, optional, (eventually)
/// mutable concept — see the module doc — and deliberately does not appear
/// on this type: a dimension/metric change would be a data-migration
/// question, while an index change is routine acceleration-structure
/// maintenance. Conflating the two was an explicit anti-goal of this phase.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionVectorConfig {
    pub dim: u32,
    pub metric: valori_domain::Metric,
}

/// A collection record stored in the MetadataDb.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    /// Human-readable collection name.
    pub name: String,
    /// The project this collection belongs to.
    pub project: String,
    /// The kernel-level namespace ID, allocated in creation order starting
    /// at 0 — no id is reserved for any particular name (Phase 3.3).
    pub namespace_id: u16,
    /// Unix seconds when this collection was created.
    pub created_at: u64,
    /// Dimension and metric — **required**, set once at creation, never
    /// optional and never overwritten. There is no "inherits the project's
    /// config" fallback any more: Project does not own vector configuration
    /// (see `valori_domain::Project`'s doc comment).
    pub vector_config: CollectionVectorConfig,
}

impl Collection {
    /// Returns the shard this collection's records live on.
    pub fn shard_id(&self, shard_count: u8) -> u8 {
        (self.namespace_id % shard_count as u16) as u8
    }
}

/// In-memory registry of name→NamespaceId mappings for one project, plus
/// each collection's vector configuration and (optional) desired index.
///
/// This is the elevated form of `NamespaceRegistry`, already wired into
/// `valori_engine::Engine.namespaces`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CollectionRegistry {
    /// name → namespace_id
    pub map: std::collections::HashMap<String, u16>,
    /// Next ID to allocate. Starts at 1 (0 is reserved for "default").
    pub next_id: u16,
    /// namespace_id → required vector config, for every collection created
    /// through the collection-scoped path. `#[serde(default)]` covers a
    /// `namespaces.json` sidecar written before *this* phase existed, whose
    /// entries (if any) came from the transitional, now-removed
    /// project-level dim/index — such an entry has no vector_config to
    /// migrate forward automatically, so it is simply absent here (the
    /// namespace still exists in `map`, just with no known dim/metric,
    /// which is a genuine, disclosed gap for any pre-this-phase collection
    /// — see the phase report's Findings).
    #[serde(default)]
    pub configs: std::collections::HashMap<u16, CollectionVectorConfig>,
    /// namespace_id → desired index algorithm, if the collection was
    /// created with one. **Not a lifecycle** — see the module doc. Absent
    /// means `index = NONE`: the collection is searchable via the existing
    /// exact/brute-force path with no dedicated ANN structure, which is a
    /// deliberate, first-class, supported state, not a missing feature.
    #[serde(default)]
    pub index_kind: std::collections::HashMap<u16, valori_domain::IndexKind>,
}

impl CollectionRegistry {
    pub fn new() -> Self {
        Self {
            map: std::collections::HashMap::new(),
            // Phase 3.3: starts at 1, NOT because id 0 means "default" —
            // "default" has no architectural meaning any more (see
            // `resolve`/`create` below, neither special-cases any name). id
            // 0 stays unallocated because `KernelState::apply_event_ns`'s
            // `DropNamespace` branch hard-rejects `namespace_id == 0`
            // unconditionally (`crates/valori-kernel/src/state/kernel.rs`) —
            // a pre-existing, unrelated kernel invariant this registry must
            // respect, not something this phase changes. Allocating a real
            // collection to id 0 would make it permanently undroppable
            // regardless of its name, which is a kernel-level landmine, not
            // a naming concern. Confirmed by a failing test
            // (`collection_create_and_drop`) before this was found.
            next_id: 1,
            configs: std::collections::HashMap::new(),
            index_kind: std::collections::HashMap::new(),
        }
    }

    /// The required vector config for `namespace_id`, if this registry knows
    /// it. `None` can only mean "unknown namespace" or "a pre-this-phase
    /// collection with no migrated config" (see the field's doc comment) —
    /// never "inherits some other default," because there is no other
    /// default any more.
    pub fn config(&self, namespace_id: u16) -> Option<CollectionVectorConfig> {
        self.configs.get(&namespace_id).copied()
    }

    /// Record the vector config for `namespace_id`. Idempotent when the
    /// dimension matches an existing entry; rejects a conflicting
    /// dimension — mirrors `KernelState::configure_namespace`'s semantics so
    /// the two never disagree on whether/how a namespace is configured.
    pub fn set_config(
        &mut self,
        namespace_id: u16,
        cfg: CollectionVectorConfig,
    ) -> Result<(), CollectionVectorConfig> {
        if let Some(existing) = self.configs.get(&namespace_id) {
            if existing.dim != cfg.dim {
                return Err(*existing);
            }
            return Ok(());
        }
        self.configs.insert(namespace_id, cfg);
        Ok(())
    }

    /// The desired index algorithm for `namespace_id`, if it has one.
    /// `None` means `index = NONE` — no dedicated ANN structure, exact
    /// search only. See the module doc for why this is not the config.
    pub fn desired_index(&self, namespace_id: u16) -> Option<valori_domain::IndexKind> {
        self.index_kind.get(&namespace_id).copied()
    }

    /// Record the desired index algorithm for `namespace_id`. No lifecycle
    /// semantics — see the module doc. Overwrites any previous value; Phase
    /// 5's `IndexManager` is what will give this real build/swap/failure
    /// handling instead of a bare overwrite.
    pub fn set_desired_index(&mut self, namespace_id: u16, kind: valori_domain::IndexKind) {
        self.index_kind.insert(namespace_id, kind);
    }

    /// Resolve a collection name to its `NamespaceId`.
    ///
    /// Phase 3.3: `"default"` has no special meaning here any more — it
    /// resolves like any other name, which means it only succeeds if a
    /// collection literally named `"default"` was explicitly created via
    /// `create`. `None` (no name given) never resolves — there is no
    /// implicit collection to fall back to.
    pub fn resolve(&self, name: Option<&str>) -> Option<u16> {
        match name {
            Some(n) => self.map.get(n).copied(),
            None => None,
        }
    }

    /// Register a new collection, allocating the next available NamespaceId.
    /// Idempotent — returns the existing id if already registered.
    /// Returns `None` if `MAX_COLLECTIONS` (1024) would be exceeded.
    ///
    /// Phase 3.3: `"default"` is allocated an id exactly like any other
    /// name — no reserved id 0, no special casing.
    pub fn create(&mut self, name: &str) -> Option<u16> {
        if let Some(&id) = self.map.get(name) {
            return Some(id);
        }
        if self.next_id >= MAX_COLLECTIONS {
            return None;
        }
        let allocated = self.next_id;
        self.next_id += 1;
        self.map.insert(name.to_string(), allocated);
        Some(allocated)
    }

    /// Remove a collection from the registry. Returns the released NamespaceId
    /// if the name was registered, `None` otherwise.
    pub fn drop(&mut self, name: &str) -> Option<u16> {
        let id = self.map.remove(name);
        if let Some(id) = id {
            self.configs.remove(&id);
            self.index_kind.remove(&id);
        }
        id
    }

    /// List all registered collection names in insertion-stable order.
    pub fn names(&self) -> Vec<&str> {
        let mut pairs: Vec<_> = self.map.iter().collect();
        pairs.sort_by_key(|(_, &id)| id);
        pairs.into_iter().map(|(n, _)| n.as_str()).collect()
    }

    /// All registered collections, sorted by id. A brand-new registry
    /// (nothing explicitly created yet) returns an empty list — Phase 3.3:
    /// no synthetic `"default"` entry is injected any more.
    pub fn list(&self) -> Vec<(String, u16)> {
        let mut out: Vec<_> = self.map.iter().map(|(k, &v)| (k.clone(), v)).collect();
        out.sort_by_key(|&(_, id)| id);
        out
    }

    /// Number of registered collections in the registry.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_create_and_resolve() {
        let mut reg = CollectionRegistry::new();
        // Phase 3.3: a fresh registry resolves nothing — no implicit
        // collection, "default" included.
        assert_eq!(reg.resolve(None), None);
        assert_eq!(reg.resolve(Some("default")), None);
        assert_eq!(reg.resolve(Some("papers")), None);

        let id = reg.create("papers").unwrap();
        assert_eq!(
            id, 1,
            "id 0 is permanently reserved — see `new`'s doc comment"
        );
        assert_eq!(reg.resolve(Some("papers")), Some(1));

        // Idempotent
        assert_eq!(reg.create("papers"), Some(1));
    }

    #[test]
    fn registry_default_has_no_special_meaning() {
        let mut reg = CollectionRegistry::new();
        // "default" must be explicitly created like any other name, and
        // gets whatever id creation order assigns it — no special-casing.
        reg.create("papers");
        let id = reg.create("default").unwrap();
        assert_eq!(id, 2);
        assert_eq!(reg.resolve(Some("default")), Some(2));
    }

    #[test]
    fn registry_drop() {
        let mut reg = CollectionRegistry::new();
        reg.create("alpha");
        reg.create("beta");
        assert_eq!(reg.drop("alpha"), Some(1));
        assert_eq!(reg.resolve(Some("alpha")), None);
        assert_eq!(reg.resolve(Some("beta")), Some(2));
    }

    #[test]
    fn fresh_registry_has_zero_collections() {
        let reg = CollectionRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert_eq!(reg.list(), Vec::new());
    }

    #[test]
    fn collection_shard_routing() {
        let c = Collection {
            name: "x".into(),
            project: "p".into(),
            namespace_id: 5,
            created_at: 0,
            vector_config: CollectionVectorConfig {
                dim: 384,
                metric: valori_domain::Metric::SquaredL2,
            },
        };
        assert_eq!(c.shard_id(4), 1); // 5 % 4 = 1
        assert_eq!(c.shard_id(1), 0); // everything on shard 0 when count=1
    }

    #[test]
    fn set_config_is_idempotent_and_rejects_conflicting_dim() {
        let mut reg = CollectionRegistry::new();
        let id = reg.create("images").unwrap();
        let cfg = CollectionVectorConfig {
            dim: 768,
            metric: valori_domain::Metric::SquaredL2,
        };
        assert!(reg.set_config(id, cfg).is_ok());
        assert!(reg.set_config(id, cfg).is_ok(), "same config is idempotent");
        assert_eq!(reg.config(id), Some(cfg));

        let conflicting = CollectionVectorConfig { dim: 1536, ..cfg };
        assert_eq!(reg.set_config(id, conflicting), Err(cfg));
    }

    #[test]
    fn unconfigured_collection_has_no_vector_config() {
        let mut reg = CollectionRegistry::new();
        let id = reg.create("legacy").unwrap();
        assert_eq!(reg.config(id), None);
    }

    #[test]
    fn desired_index_is_separate_from_vector_config() {
        let mut reg = CollectionRegistry::new();
        let id = reg.create("images").unwrap();
        reg.set_config(
            id,
            CollectionVectorConfig {
                dim: 768,
                metric: valori_domain::Metric::SquaredL2,
            },
        )
        .unwrap();
        // A collection can have vector config with NO desired index at all —
        // "index = NONE" is a first-class, supported state.
        assert_eq!(reg.desired_index(id), None);

        reg.set_desired_index(id, valori_domain::IndexKind::Hnsw);
        assert_eq!(reg.desired_index(id), Some(valori_domain::IndexKind::Hnsw));
        // The vector config is untouched by setting an index.
        assert_eq!(reg.config(id).unwrap().dim, 768);
    }

    #[test]
    fn old_sidecar_without_configs_key_deserializes_with_empty_map() {
        // Simulates a namespaces.json written before this phase.
        let old_json = r#"{"map":{"docs":1},"next_id":2}"#;
        let reg: CollectionRegistry = serde_json::from_str(old_json).unwrap();
        assert!(reg.configs.is_empty());
        assert!(reg.index_kind.is_empty());
        assert_eq!(reg.config(1), None);
    }
}
