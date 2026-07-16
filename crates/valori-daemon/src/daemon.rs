// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! The daemon orchestrator — composes ProjectManager + WorkspaceManager +
//! Supervisor and enforces lifecycle rules. This is the single object the HTTP
//! layer drives; it owns everything outside the engine (RFC-0006).

use std::path::PathBuf;
use std::time::Instant;

use serde_json::{json, Value};

use crate::error::{DaemonError, DaemonResult};
use crate::events::{Event, EventStore, MemoryEventStore};
use crate::project::{JsonProjectStore, Project, ProjectManifest};
use crate::runtime::{LocalRuntime, NodeInfo, ResourceStats, Runtime};
use crate::store::{ProjectStore, WorkspaceStore};
use crate::supervisor::SupervisionInfo;
use crate::workspace::{JsonWorkspaceStore, Workspace};
use valori_models::{JsonModelStore, ModelManager, ModelManifest, ModelStore};

/// The daemon's injected dependencies. Everything the `Daemon` needs is a trait
/// object supplied at construction — nothing durable is built internally, so
/// any layer (store, runtime, event sink) can be swapped without touching the
/// daemon (Dependency Inversion).
pub struct DaemonDeps {
    pub projects: Box<dyn ProjectStore>,
    pub workspaces: Box<dyn WorkspaceStore>,
    pub runtime: Box<dyn Runtime>,
    pub events: Box<dyn EventStore>,
    pub models: Box<dyn ModelStore>,
}

pub struct Daemon {
    home: PathBuf,
    started: Instant,
    projects: Box<dyn ProjectStore>,
    workspaces: Box<dyn WorkspaceStore>,
    runtime: Box<dyn Runtime>,
    events: Box<dyn EventStore>,
    models: ModelManager,
    supervisor: crate::supervisor::Supervisor,
}

impl Daemon {
    /// Build a daemon rooted at `home` (e.g. `~/.valori`) with the default
    /// implementations: JSON stores, `LocalRuntime`, in-memory event store.
    pub fn new(home: impl AsRef<std::path::Path>) -> DaemonResult<Self> {
        let home_ref = home.as_ref();
        let deps = DaemonDeps {
            projects: Box::new(JsonProjectStore::new(home_ref)?),
            workspaces: Box::new(JsonWorkspaceStore::new(home_ref)?),
            runtime: Box::new(LocalRuntime::new()?),
            events: Box::new(MemoryEventStore::new()),
            models: Box::new(JsonModelStore::new(home_ref)?),
        };
        Self::with_deps(home_ref, deps)
    }

    /// Build a daemon from fully-injected dependencies (Docker runtime, SQLite
    /// store, etc.). The daemon constructs nothing durable itself.
    pub fn with_deps(home: impl AsRef<std::path::Path>, deps: DaemonDeps) -> DaemonResult<Self> {
        let home = home.as_ref().to_path_buf();
        // One-time, idempotent startup migrations (e.g. importing ui/'s legacy
        // project registry) — see `crate::migration`. Never blocks startup on
        // failure; a failed migration just retries next launch.
        crate::migration::run_all(&home, deps.projects.as_ref());
        Ok(Self {
            models: ModelManager::new(&home, deps.models)?,
            projects: deps.projects,
            workspaces: deps.workspaces,
            runtime: deps.runtime,
            events: deps.events,
            supervisor: crate::supervisor::Supervisor::new(),
            started: Instant::now(),
            home,
        })
    }

    // ── Models (E1-lite) ───────────────────────────────────────────────────────

    pub fn models_catalog(&self) -> Value {
        self.models.catalog_json()
    }

    pub fn model_detail(&self, id: &str) -> DaemonResult<ModelManifest> {
        Ok(self.models.get(id)?)
    }

    pub async fn install_model(&mut self, id: &str) -> DaemonResult<ModelManifest> {
        let model = self.models.install(id).await?;
        self.events.record("model.installed", Some(id));
        Ok(model)
    }

    pub fn remove_model(&mut self, id: &str) -> DaemonResult<()> {
        self.models.remove(id)?;
        self.events.record("model.removed", Some(id));
        Ok(())
    }

    // ── Projects ───────────────────────────────────────────────────────────────

    pub fn create_project(&self, config: ProjectManifest) -> DaemonResult<Project> {
        if !self.workspaces.exists(&config.workspace) {
            return Err(DaemonError::InvalidInput(format!(
                "workspace '{}' does not exist",
                config.workspace
            )));
        }
        let project = self.projects.create(config)?;
        self.events
            .record("project.created", Some(project.config.name.as_str()));
        Ok(project)
    }

    pub fn delete_project(&self, name: &str) -> DaemonResult<()> {
        if self.runtime.is_running(name) {
            return Err(DaemonError::Running(name.to_string()));
        }
        self.projects.delete(name)?;
        self.events.record("project.deleted", Some(name));
        Ok(())
    }

    pub fn rename_project(&self, old_name: &str, new_name: &str) -> DaemonResult<Project> {
        if self.runtime.is_running(old_name) {
            return Err(DaemonError::Running(old_name.to_string()));
        }
        let project = self.projects.rename(old_name, new_name)?;
        self.events.record("project.renamed", Some(new_name));
        Ok(project)
    }

    pub async fn start_project(&mut self, name: &str) -> DaemonResult<NodeInfo> {
        let project = self.projects.get(name)?;
        let info = self.runtime.start(&project).await?;
        self.supervisor
            .on_started(name, project.config.restart_policy);
        self.events.record("project.started", Some(name));
        Ok(info)
    }

    pub async fn stop_project(&mut self, name: &str) -> DaemonResult<NodeInfo> {
        self.projects.get(name)?;
        let info = self.runtime.stop(name).await?;
        self.supervisor.on_stopped(name); // operator stop → no auto-restart
        self.events.record("project.stopped", Some(name));
        Ok(info)
    }

    /// Supervision tick (called periodically by the daemon's monitor task):
    /// detect crashes, then restart any node whose policy + backoff allow it.
    /// Returns the number of restarts performed this tick (for tests/metrics).
    pub async fn supervise_tick(&mut self) -> usize {
        for exit in self.runtime.poll_exits() {
            tracing::warn!(project = %exit.name, reason = %exit.reason, "node exited unexpectedly");
            self.events.record("project.crashed", Some(&exit.name));
            self.supervisor.on_crash(&exit.name, exit.reason);
        }

        let mut restarted = 0;
        for name in self.supervisor.due_for_restart() {
            let Ok(project) = self.projects.get(&name) else {
                continue;
            };
            self.supervisor.set_recovering(&name);
            self.events.record("project.recovering", Some(&name));
            match self.runtime.start(&project).await {
                Ok(_) => {
                    self.supervisor.on_restart_success(&name);
                    self.events.record("project.restarted", Some(&name));
                    restarted += 1;
                }
                Err(e) => {
                    self.supervisor.on_restart_failure(&name, e.to_string());
                }
            }
        }
        restarted
    }

    pub async fn restart_project(&mut self, name: &str) -> DaemonResult<NodeInfo> {
        let project = self.projects.get(name)?;
        self.runtime.restart(&project).await
    }

    pub fn list_projects(&self) -> DaemonResult<Vec<(Project, NodeInfo, Option<SupervisionInfo>)>> {
        let mut out = Vec::new();
        for project in self.projects.list()? {
            let (status, sup) = self.node_status(&project.config.name);
            out.push((project, status, sup));
        }
        Ok(out)
    }

    pub fn project_detail(
        &self,
        name: &str,
    ) -> DaemonResult<(Project, NodeInfo, Option<SupervisionInfo>)> {
        let project = self.projects.get(name)?;
        let (status, sup) = self.node_status(name);
        Ok((project, status, sup))
    }

    /// Merge runtime status with the supervisor's overlay: the supervisor owns
    /// crash-derived states (Failed/Recovering) the runtime has already dropped.
    fn node_status(&self, name: &str) -> (NodeInfo, Option<SupervisionInfo>) {
        let mut info = self.runtime.status(name);
        let sup = self.supervisor.info(name);
        if let Some(ref s) = sup {
            if let Some(state) = s.state {
                info.status = state;
            }
        }
        (info, sup)
    }

    /// Tail of a project's node log (last `lines` lines).
    pub fn project_logs(&self, name: &str, lines: usize) -> DaemonResult<String> {
        let project = self.projects.get(name)?;
        let log_path = project.dir.join("node.log");
        match std::fs::read_to_string(&log_path) {
            Ok(text) => {
                let all: Vec<&str> = text.lines().collect();
                let start = all.len().saturating_sub(lines);
                Ok(all[start..].join("\n"))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(e.into()),
        }
    }

    // ── Workspaces ─────────────────────────────────────────────────────────────

    pub fn list_workspaces(&self) -> Vec<Value> {
        let projects = self.projects.list().unwrap_or_default();
        self.workspaces
            .list()
            .iter()
            .map(|w| {
                let n = projects
                    .iter()
                    .filter(|p| p.config.workspace == w.name)
                    .count();
                json!({ "id": w.id, "name": w.name, "created_at": w.created_at, "projects": n })
            })
            .collect()
    }

    pub fn create_workspace(&mut self, name: &str) -> DaemonResult<Workspace> {
        let ws = self.workspaces.create(name)?;
        self.events.record("workspace.created", Some(name));
        Ok(ws)
    }

    pub fn rename_workspace(&mut self, from: &str, to: &str) -> DaemonResult<Workspace> {
        self.workspaces.rename(from, to)
    }

    pub fn delete_workspace(&mut self, name: &str) -> DaemonResult<()> {
        let in_use = self
            .projects
            .list()?
            .iter()
            .any(|p| p.config.workspace == name);
        if in_use {
            return Err(DaemonError::InvalidInput(format!(
                "workspace '{name}' still has projects — move or delete them first"
            )));
        }
        self.workspaces.delete(name)?;
        self.events.record("workspace.deleted", Some(name));
        Ok(())
    }

    // ── Collections (proxied to the running node) ──────────────────────────────

    /// Base URL of a project's node, or an error if it isn't running.
    fn node_base(&self, name: &str) -> DaemonResult<String> {
        self.projects.get(name)?; // 404 if the project doesn't exist
        match self.runtime.port_of(name) {
            Some(port) => Ok(format!("http://127.0.0.1:{port}")),
            None => Err(DaemonError::InvalidInput(format!(
                "project '{name}' is not running — start it before managing collections"
            ))),
        }
    }

    pub async fn list_collections(&self, name: &str) -> DaemonResult<Value> {
        let base = self.node_base(name)?;
        proxy_get(&format!("{base}/v1/namespaces")).await
    }

    pub async fn create_collection(&self, name: &str, collection: &str) -> DaemonResult<Value> {
        let base = self.node_base(name)?;
        proxy_post(
            &format!("{base}/v1/namespaces"),
            json!({ "name": collection }),
        )
        .await
    }

    pub async fn delete_collection(&self, name: &str, collection: &str) -> DaemonResult<Value> {
        let base = self.node_base(name)?;
        proxy_delete(&format!("{base}/v1/namespaces/{collection}")).await
    }

    // ── System / discovery ─────────────────────────────────────────────────────

    /// `GET /v1/system` — the discovery endpoint every client calls first.
    pub fn system(&self) -> Value {
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "api": "v1",
            "platform": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "daemon_pid": std::process::id(),
            "uptime_secs": self.started.elapsed().as_secs(),
            "home": self.home.display().to_string(),
            "projects": self.projects.list().map(|p| p.len()).unwrap_or(0),
            "running": self.runtime.running_count(),
            "workspaces": self.workspaces.count(),
            "models": self.models.count(),
        })
    }

    /// `GET /v1/config` — the daemon's effective configuration.
    pub fn config(&self) -> Value {
        json!({
            "home": self.home.display().to_string(),
            "runtime": self.runtime.describe(),
            "version": env!("CARGO_PKG_VERSION"),
        })
    }

    /// Live resource sample for a running node.
    pub fn project_resources(&self, name: &str) -> DaemonResult<ResourceStats> {
        self.projects.get(name)?; // 404 if unknown
        self.runtime
            .resources(name)
            .ok_or_else(|| DaemonError::InvalidInput(format!("project '{name}' is not running")))
    }

    /// Recent daemon lifecycle events (Docker-style).
    pub fn events(&self, limit: usize) -> Vec<Event> {
        self.events.recent(limit)
    }

    pub async fn shutdown(&mut self) {
        self.runtime.stop_all().await;
    }
}

// ── HTTP proxy helpers (daemon → running node) ─────────────────────────────────

async fn proxy_get(url: &str) -> DaemonResult<Value> {
    forward(reqwest::Client::new().get(url)).await
}
async fn proxy_post(url: &str, body: Value) -> DaemonResult<Value> {
    forward(reqwest::Client::new().post(url).json(&body)).await
}
async fn proxy_delete(url: &str) -> DaemonResult<Value> {
    forward(reqwest::Client::new().delete(url)).await
}

async fn forward(req: reqwest::RequestBuilder) -> DaemonResult<Value> {
    let resp = req
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| DaemonError::StartFailed(format!("node request failed: {e}")))?;
    if !resp.status().is_success() {
        let msg = resp.text().await.unwrap_or_default();
        return Err(DaemonError::InvalidInput(format!("node error: {msg}")));
    }
    resp.json::<Value>()
        .await
        .map_err(|e| DaemonError::StartFailed(format!("node returned invalid JSON: {e}")))
}
