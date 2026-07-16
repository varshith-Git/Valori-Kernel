// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Node Runtime — the abstraction over *where and how* a project's node runs.
//!
//! D2 introduces the [`Runtime`] trait so the daemon depends on a capability,
//! not a concrete process launcher. Today the only implementor is
//! [`LocalRuntime`] (a supervised local `valori-node` process); tomorrow
//! `DockerRuntime` / `SshRuntime` / `RemoteRuntime` slot in with **no change**
//! to the daemon, the API, or the desktop.
//!
//! The runtime is intentionally decomposed (SRP — each has one reason to change):
//! - [`port::PortAllocator`]     — pick a free port
//! - [`resource::ResourceMonitor`] — CPU / RAM / threads for a PID
//! - [`policy::RestartPolicy`]   — Always / OnFailure / Never
//! - health polling + log capture live in [`local`]
//!
//! `Runtime` knows lifecycle + status only. It does not know about workspaces,
//! HTTP, or the event log.

pub mod launcher;
pub mod local;
pub mod port;
pub mod resource;
pub mod state;

pub use launcher::{LaunchSpec, Launcher, LocalLauncher, RunningProcess};
pub use local::LocalRuntime;
pub use resource::ResourceStats;
pub use state::RuntimeState;

use serde::Serialize;

use crate::error::DaemonResult;
use crate::project::Project;

/// Public status snapshot for one project's node. `status` is the explicit
/// [`RuntimeState`] machine, not a free-form string.
#[derive(Debug, Clone, Serialize)]
pub struct NodeInfo {
    pub name: String,
    pub status: RuntimeState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_secs: Option<u64>,
}

impl NodeInfo {
    pub(crate) fn stopped(name: &str) -> Self {
        Self {
            name: name.to_string(),
            status: RuntimeState::Stopped,
            pid: None,
            port: None,
            uptime_secs: None,
        }
    }
}

/// An unexpected node exit detected by [`Runtime::poll_exits`].
#[derive(Debug, Clone)]
pub struct NodeExit {
    pub name: String,
    pub reason: String,
}

/// A place a project's node can run. One implementor per backend (local,
/// docker, ssh, remote). The daemon owns a `Box<dyn Runtime>`.
#[async_trait::async_trait]
pub trait Runtime: Send + Sync {
    /// Human-readable kind, e.g. `"local"` (surfaced in `/v1/config`).
    fn kind(&self) -> &'static str;

    /// Start (or return the already-running) node for `project`.
    async fn start(&mut self, project: &Project) -> DaemonResult<NodeInfo>;

    /// Stop the node for `name`. Idempotent.
    async fn stop(&mut self, name: &str) -> DaemonResult<NodeInfo>;

    /// Stop then start.
    async fn restart(&mut self, project: &Project) -> DaemonResult<NodeInfo>;

    /// Current status (never fails — a missing node is simply `Stopped`).
    fn status(&self, name: &str) -> NodeInfo;

    fn is_running(&self, name: &str) -> bool;
    fn running_count(&self) -> usize;

    /// The node's port, if running (used for collection proxying).
    fn port_of(&self, name: &str) -> Option<u16>;

    /// Live resource sample for a running node, if available.
    fn resources(&self, name: &str) -> Option<ResourceStats>;

    /// Non-blocking sweep for nodes that exited unexpectedly since the last
    /// call. Detected nodes are dropped from the running set and returned so
    /// the operational supervisor can apply its restart policy.
    fn poll_exits(&mut self) -> Vec<NodeExit>;

    /// Terminate every node (daemon shutdown) — gracefully: each node gets the
    /// same snapshot-then-terminate treatment as a single `stop()` call.
    async fn stop_all(&mut self);

    /// Backend detail for `/v1/config` (e.g. binary path, port range).
    fn describe(&self) -> serde_json::Value;
}
