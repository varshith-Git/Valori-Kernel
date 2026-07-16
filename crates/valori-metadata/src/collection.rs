// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Collection — a named, isolated namespace of records within a project.
//!
//! Collections map a human-readable name (e.g. "research-paper") to a
//! `NamespaceId` (u16) used by the kernel for record isolation and shard routing.
//! `shard_for_namespace(namespace_id, shard_count) = namespace_id % shard_count`.
use serde::{Deserialize, Serialize};

/// The maximum number of collections per project, matching `MAX_NAMESPACES`.
pub const MAX_COLLECTIONS: u16 = 1024;

/// A collection record stored in the MetadataDb.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    /// Human-readable collection name.
    pub name: String,
    /// The project this collection belongs to.
    pub project: String,
    /// The kernel-level namespace ID. `0` = the "default" collection.
    pub namespace_id: u16,
    /// Unix seconds when this collection was created.
    pub created_at: u64,
}

impl Collection {
    /// Returns the shard this collection's records live on.
    pub fn shard_id(&self, shard_count: u8) -> u8 {
        (self.namespace_id % shard_count as u16) as u8
    }
}

/// In-memory registry of name→NamespaceId mappings for one project.
///
/// This is the elevated form of `NamespaceRegistry` currently in
/// `valori-node/src/engine.rs`. Future phases will replace the engine's
/// inline registry with this type.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CollectionRegistry {
    /// name → namespace_id
    pub map: std::collections::HashMap<String, u16>,
    /// Next ID to allocate. Starts at 1 (0 is reserved for "default").
    pub next_id: u16,
}

impl CollectionRegistry {
    pub fn new() -> Self {
        Self {
            map: std::collections::HashMap::new(),
            next_id: 1,
        }
    }

    /// Resolve a collection name to its `NamespaceId`.
    /// Returns `Some(0)` for `None` or `"default"`, `Some(id)` for known
    /// collections, `None` for unknown names.
    pub fn resolve(&self, name: Option<&str>) -> Option<u16> {
        match name {
            None | Some("default") => Some(0),
            Some(n) => self.map.get(n).copied(),
        }
    }

    /// Register a new collection, allocating the next available NamespaceId.
    /// Idempotent — returns the existing id if already registered.
    /// Returns `None` if `MAX_COLLECTIONS` (1024) would be exceeded.
    pub fn create(&mut self, name: &str) -> Option<u16> {
        if name == "default" {
            return Some(0);
        }
        if let Some(&id) = self.map.get(name) {
            return Some(id);
        }
        if self.next_id >= MAX_COLLECTIONS {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.map.insert(name.to_string(), id);
        Some(id)
    }

    /// Remove a collection from the registry. Returns the released NamespaceId
    /// if the name was registered, `None` otherwise.
    pub fn drop(&mut self, name: &str) -> Option<u16> {
        self.map.remove(name)
    }

    /// List all registered collection names in insertion-stable order.
    pub fn names(&self) -> Vec<&str> {
        let mut pairs: Vec<_> = self.map.iter().collect();
        pairs.sort_by_key(|(_, &id)| id);
        pairs.into_iter().map(|(n, _)| n.as_str()).collect()
    }

    /// All collections including the implicit "default", sorted by id.
    /// Mirrors `NamespaceRegistry::list` for Engine compatibility.
    pub fn list(&self) -> Vec<(String, u16)> {
        let mut out = vec![("default".to_string(), 0u16)];
        let mut rest: Vec<_> = self.map.iter().map(|(k, &v)| (k.clone(), v)).collect();
        rest.sort_by_key(|&(_, id)| id);
        out.extend(rest);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_create_and_resolve() {
        let mut reg = CollectionRegistry::new();
        assert_eq!(reg.resolve(None), Some(0));
        assert_eq!(reg.resolve(Some("default")), Some(0));
        assert_eq!(reg.resolve(Some("papers")), None);

        let id = reg.create("papers").unwrap();
        assert_eq!(id, 1);
        assert_eq!(reg.resolve(Some("papers")), Some(1));

        // Idempotent
        assert_eq!(reg.create("papers"), Some(1));
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
    fn collection_shard_routing() {
        let c = Collection {
            name: "x".into(),
            project: "p".into(),
            namespace_id: 5,
            created_at: 0,
        };
        assert_eq!(c.shard_id(4), 1); // 5 % 4 = 1
        assert_eq!(c.shard_id(1), 0); // everything on shard 0 when count=1
    }
}
