// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Workspace registry — the top grouping layer above projects (RFC-0006).
//!
//! A **workspace** is a named grouping of projects (cf. VS Code workspaces). In
//! D1.1 it is a minimal persisted record `{ name, created_at }`; the
//! project↔workspace wiring (membership, per-workspace views) is fleshed out in
//! D3. A `default` workspace always exists so every project has a home.
//!
//! Persistence is a single `<home>/workspaces.json` (write-then-rename).

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

use crate::error::{DaemonError, DaemonResult};

pub const DEFAULT_WORKSPACE: &str = "default";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workspace {
    /// Stable id (UUID). Never changes; `name` is a mutable label.
    #[serde(default = "crate::new_id")]
    pub id: String,
    pub name: String,
    pub created_at: u64,
}

pub struct JsonWorkspaceStore {
    file: PathBuf,
    workspaces: Vec<Workspace>,
}

impl JsonWorkspaceStore {
    /// Load (or initialize) the workspace registry under `home`. Guarantees a
    /// `default` workspace exists.
    pub fn new(home: impl AsRef<Path>) -> DaemonResult<Self> {
        let file = home.as_ref().join("workspaces.json");
        let mut workspaces: Vec<Workspace> = match std::fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        if !workspaces.iter().any(|w| w.name == DEFAULT_WORKSPACE) {
            workspaces.insert(0, Workspace { id: crate::new_id(), name: DEFAULT_WORKSPACE.into(), created_at: now_unix() });
        }
        let mgr = Self { file, workspaces };
        mgr.flush()?;
        Ok(mgr)
    }

    fn flush(&self) -> DaemonResult<()> {
        let tmp = self.file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&self.workspaces)?)?;
        std::fs::rename(&tmp, &self.file)?;
        Ok(())
    }
}

impl crate::store::WorkspaceStore for JsonWorkspaceStore {
    fn list(&self) -> Vec<Workspace> {
        self.workspaces.clone()
    }

    fn get(&self, name: &str) -> DaemonResult<Workspace> {
        self.workspaces
            .iter()
            .find(|w| w.name == name)
            .cloned()
            .ok_or_else(|| DaemonError::NotFound(format!("workspace '{name}'")))
    }

    fn exists(&self, name: &str) -> bool {
        self.workspaces.iter().any(|w| w.name == name)
    }

    fn create(&mut self, name: &str) -> DaemonResult<Workspace> {
        if !is_valid_name(name) {
            return Err(DaemonError::InvalidInput(format!("invalid workspace name '{name}'")));
        }
        if self.exists(name) {
            return Err(DaemonError::AlreadyExists(format!("workspace '{name}'")));
        }
        let ws = Workspace { id: crate::new_id(), name: name.to_string(), created_at: now_unix() };
        self.workspaces.push(ws.clone());
        self.flush()?;
        Ok(ws)
    }

    fn rename(&mut self, from: &str, to: &str) -> DaemonResult<Workspace> {
        if from == DEFAULT_WORKSPACE {
            return Err(DaemonError::InvalidInput("the 'default' workspace cannot be renamed".into()));
        }
        if !is_valid_name(to) {
            return Err(DaemonError::InvalidInput(format!("invalid workspace name '{to}'")));
        }
        if self.exists(to) {
            return Err(DaemonError::AlreadyExists(format!("workspace '{to}'")));
        }
        let ws = self
            .workspaces
            .iter_mut()
            .find(|w| w.name == from)
            .ok_or_else(|| DaemonError::NotFound(format!("workspace '{from}'")))?;
        ws.name = to.to_string();
        let renamed = ws.clone();
        self.flush()?;
        Ok(renamed)
    }

    fn delete(&mut self, name: &str) -> DaemonResult<()> {
        if name == DEFAULT_WORKSPACE {
            return Err(DaemonError::InvalidInput("the 'default' workspace cannot be deleted".into()));
        }
        if !self.exists(name) {
            return Err(DaemonError::NotFound(format!("workspace '{name}'")));
        }
        self.workspaces.retain(|w| w.name != name);
        self.flush()
    }

    fn count(&self) -> usize {
        self.workspaces.len()
    }
}

fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::WorkspaceStore;

    #[test]
    fn default_always_exists_and_crud() {
        let home = tempfile::tempdir().unwrap();
        let mut wm = JsonWorkspaceStore::new(home.path()).unwrap();
        assert!(wm.exists(DEFAULT_WORKSPACE));

        wm.create("healthcare").unwrap();
        assert_eq!(wm.count(), 2);
        assert!(matches!(wm.create("healthcare"), Err(DaemonError::AlreadyExists(_))));

        wm.rename("healthcare", "clinical").unwrap();
        assert!(wm.exists("clinical") && !wm.exists("healthcare"));

        assert!(wm.delete(DEFAULT_WORKSPACE).is_err());
        assert!(wm.rename(DEFAULT_WORKSPACE, "x").is_err());

        wm.delete("clinical").unwrap();
        assert_eq!(wm.count(), 1);
    }

    #[test]
    fn persists_across_reload() {
        let home = tempfile::tempdir().unwrap();
        {
            let mut wm = JsonWorkspaceStore::new(home.path()).unwrap();
            wm.create("finance").unwrap();
        }
        let wm2 = JsonWorkspaceStore::new(home.path()).unwrap();
        assert!(wm2.exists("finance"));
    }
}
