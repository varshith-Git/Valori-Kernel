// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use std::collections::HashMap;
use serde_json::Value;
use std::sync::RwLock;
use std::path::Path;

/// Simple Key-Value store for Metadata.
/// Keys are namespaced strings (e.g. "rec:123", "node:50").
/// Values are arbitrary JSON.
pub struct MetadataStore {
    data: RwLock<HashMap<String, Value>>,
}

impl MetadataStore {
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
        }
    }

    pub fn set(&self, key: String, value: Value) {
        let mut guard = self.data.write().unwrap();
        guard.insert(key, value);
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        let guard = self.data.read().unwrap();
        guard.get(key).cloned()
    }
    
    pub fn snapshot(&self) -> Vec<u8> {
        let guard = self.data.read().unwrap();
        serde_json::to_vec(&*guard).unwrap_or_default()
    }
    
    pub fn restore(&self, data: &[u8]) {
        if let Ok(map) = serde_json::from_slice(data) {
            let mut guard = self.data.write().unwrap();
            *guard = map;
        }
    }

    /// Atomically persist the store to `path` using write-then-rename.
    ///
    /// Writes to `<path>.tmp` first, then renames to `path` so a crash mid-write
    /// never leaves a half-written file.
    pub fn flush_to(&self, path: &Path) -> std::io::Result<()> {
        let data = self.snapshot();
        let tmp = path.with_extension("metadata.json.tmp");
        std::fs::write(&tmp, &data)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Load and replace the store from a JSON file.  A missing file is silently
    /// ignored (not an error — fresh start is valid).
    pub fn load_from(&self, path: &Path) -> std::io::Result<()> {
        if !path.exists() {
            return Ok(());
        }
        let data = std::fs::read(path)?;
        self.restore(&data);
        Ok(())
    }
}
