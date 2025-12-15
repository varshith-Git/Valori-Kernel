// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use std::collections::HashMap;
use serde_json::Value;
use std::sync::RwLock;

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
}
