// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Persistence seams — `ProjectStore` and `WorkspaceStore`.
//!
//! The daemon depends on these traits, not on the filesystem. Today the only
//! implementors are `JsonProjectStore` / `JsonWorkspaceStore` (in `project.rs`
//! / `workspace.rs`); tomorrow a `SqliteProjectStore` or `CloudProjectStore`
//! drops in with no daemon change (Open/Closed).

use crate::error::DaemonResult;
use crate::project::{Project, ProjectManifest};
use crate::workspace::Workspace;

/// Durable catalog of projects.
pub trait ProjectStore: Send + Sync {
    fn create(&self, config: ProjectManifest) -> DaemonResult<Project>;
    fn get(&self, name: &str) -> DaemonResult<Project>;
    fn list(&self) -> DaemonResult<Vec<Project>>;
    fn delete(&self, name: &str) -> DaemonResult<()>;
    /// Rename a stopped project: moves its directory and updates the manifest.
    /// Returns the project under its new name.
    fn rename(&self, old_name: &str, new_name: &str) -> DaemonResult<Project>;
    /// Adopt a manifest into an **already-existing** project directory
    /// without erasing whatever data files are already there — unlike
    /// [`create`](ProjectStore::create), which refuses if the directory
    /// exists. Used only by migrations (see `crate::migration`) to bring a
    /// legacy, non-daemon-managed project under daemon management.
    fn import(&self, config: ProjectManifest) -> DaemonResult<Project>;
}

/// Durable catalog of workspaces.
pub trait WorkspaceStore: Send + Sync {
    fn list(&self) -> Vec<Workspace>;
    fn get(&self, name: &str) -> DaemonResult<Workspace>;
    fn exists(&self, name: &str) -> bool;
    fn create(&mut self, name: &str) -> DaemonResult<Workspace>;
    fn rename(&mut self, from: &str, to: &str) -> DaemonResult<Workspace>;
    fn delete(&mut self, name: &str) -> DaemonResult<()>;
    fn count(&self) -> usize;
}

