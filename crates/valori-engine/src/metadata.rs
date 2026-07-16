// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! In-process JSON key-value sidecar for per-record and per-document metadata.
//!
//! Keys are namespaced strings (e.g. `"record:123"`, `"document:50"`).
//! Values are arbitrary JSON. The store is write-then-rename safe: [`flush_to`]
//! writes a `.tmp` file and renames atomically so a crash mid-write never leaves
//! a half-written file.

use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;

use serde_json::Value;

pub struct MetadataStore {
    data: RwLock<HashMap<String, Value>>,
}

impl MetadataStore {
    pub fn new() -> Self {
        Self { data: RwLock::new(HashMap::new()) }
    }

    pub fn set(&self, key: String, value: Value) {
        self.data.write().unwrap().insert(key, value);
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        self.data.read().unwrap().get(key).cloned()
    }

    pub fn snapshot(&self) -> Vec<u8> {
        serde_json::to_vec(&*self.data.read().unwrap()).unwrap_or_default()
    }

    pub fn restore(&self, data: &[u8]) {
        if let Ok(map) = serde_json::from_slice(data) {
            *self.data.write().unwrap() = map;
        }
    }

    /// Atomically persist to `path` (write `.tmp`, then rename).
    pub fn flush_to(&self, path: &Path) -> std::io::Result<()> {
        let data = self.snapshot();
        let tmp = path.with_extension("metadata.json.tmp");
        std::fs::write(&tmp, &data)?;
        std::fs::rename(&tmp, path)
    }

    /// Load from a JSON file. A missing file is silently ignored.
    pub fn load_from(&self, path: &Path) -> std::io::Result<()> {
        if !path.exists() { return Ok(()); }
        self.restore(&std::fs::read(path)?);
        Ok(())
    }
}

impl Default for MetadataStore {
    fn default() -> Self { Self::new() }
}
