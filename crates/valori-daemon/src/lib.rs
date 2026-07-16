// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! # valori-daemon
//!
//! The Valori Daemon — a long-running control plane that owns project and
//! workspace lifecycle and runs `valori-node` instances through a pluggable
//! [`runtime::Runtime`]. The Rust successor to the TypeScript process manager
//! in `ui/src/lib/server/`.
//!
//! Layers (no manager knows another; the daemon composes them):
//! - [`workspace::WorkspaceManager`] — grouping above projects
//! - [`project::ProjectManager`]     — the project catalog
//! - [`runtime::Runtime`]            — where/how a node runs (Local today)
//! - [`events::EventLog`]            — Docker-style lifecycle event stream
//!
//! See `rfcs/0006-desktop-daemon-architecture.md`.

pub mod daemon;
pub mod error;
pub mod events;
pub mod http;
pub mod migration;
pub mod policy;
pub mod project;
pub mod runtime;
pub mod store;
pub mod supervisor;
pub mod workspace;

pub use daemon::{Daemon, DaemonDeps};
pub use error::{DaemonError, DaemonResult};
pub use events::{Event, EventStore, MemoryEventStore};
pub use http::{router, SharedDaemon};
pub use policy::RestartPolicy;
pub use project::{
    ClusterConfig, EmbeddingConfig, JsonProjectStore, Project, ProjectManifest, ProjectNode,
    StorageConfig,
};
pub use runtime::{
    LaunchSpec, Launcher, LocalLauncher, LocalRuntime, NodeInfo, ResourceStats, RunningProcess,
    Runtime, RuntimeState,
};
pub use store::{ProjectStore, WorkspaceStore};
pub use supervisor::{SupervisionInfo, Supervisor};
pub use workspace::{JsonWorkspaceStore, Workspace, DEFAULT_WORKSPACE};

/// A fresh stable resource id (UUID v4). Names are mutable; ids never change.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Default daemon data root: `$VALORI_HOME` or `~/.valori`.
pub fn default_home() -> std::path::PathBuf {
    if let Ok(h) = std::env::var("VALORI_HOME") {
        return std::path::PathBuf::from(h);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(home).join(".valori")
}
