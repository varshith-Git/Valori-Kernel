// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `LocalRuntime` — orchestrates local `valori-node` processes.
//!
//! It does NOT spawn processes itself — that is the [`Launcher`]'s job. The
//! runtime owns orchestration: port allocation, health, [`RuntimeState`]
//! transitions, resource sampling. Swap `LocalLauncher` for `DockerLauncher`
//! and the runtime is unchanged.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use crate::error::{DaemonError, DaemonResult};
use crate::project::{ClusterConfig, Project, ProjectNode};
use crate::runtime::launcher::{LaunchSpec, Launcher, LocalLauncher, RunningProcess};
use crate::runtime::port::PortAllocator;
use crate::runtime::resource::{ResourceMonitor, ResourceStats};
use crate::runtime::state::RuntimeState;
use crate::runtime::{NodeExit, NodeInfo, Runtime};

const PORT_LO: u16 = 8100;
const PORT_HI: u16 = 8999;

struct RunningNode {
    process: Box<dyn RunningProcess>,
    port: u16,
    started: Instant,
    state: RuntimeState,
}

/// One physical process within a cluster project (RFC-0007). Kept as a
/// sibling to `RunningNode` rather than folding cluster multiplicity into the
/// single-node struct/map — the single-node path below is unchanged by this.
struct ClusterRunningNode {
    id: u32,
    process: Box<dyn RunningProcess>,
    port: u16,
    started: Instant,
    state: RuntimeState,
}

pub struct LocalRuntime {
    binary: PathBuf,
    launcher: Box<dyn Launcher>,
    ports: PortAllocator,
    running: HashMap<String, RunningNode>,
    /// Cluster projects (`replication > 1`) — keyed by project name, one
    /// entry per physical node process. A project name only ever appears in
    /// `running` XOR `cluster_running`, never both.
    cluster_running: HashMap<String, Vec<ClusterRunningNode>>,
}

impl LocalRuntime {
    /// Resolve the `valori-node` binary and build a runtime with the default
    /// [`LocalLauncher`].
    pub fn new() -> DaemonResult<Self> {
        Self::with_launcher(Box::new(LocalLauncher))
    }

    /// Inject a launcher (Docker/SSH in future, or a fake in tests).
    pub fn with_launcher(launcher: Box<dyn Launcher>) -> DaemonResult<Self> {
        Ok(Self {
            binary: Self::resolve_binary()?,
            launcher,
            ports: PortAllocator::new(PORT_LO, PORT_HI),
            running: HashMap::new(),
            cluster_running: HashMap::new(),
        })
    }

    fn resolve_binary() -> DaemonResult<PathBuf> {
        if let Ok(p) = std::env::var("VALORI_NODE_BIN") {
            let pb = PathBuf::from(&p);
            return if pb.exists() {
                Ok(pb)
            } else {
                Err(DaemonError::NodeBinaryMissing(format!(
                    "VALORI_NODE_BIN={p} does not exist"
                )))
            };
        }
        let root = std::env::var("VALORI_REPO_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default());
        for rel in ["target/release/valori-node", "target/debug/valori-node"] {
            let cand = root.join(rel);
            if cand.exists() {
                return Ok(cand);
            }
        }
        Err(DaemonError::NodeBinaryMissing(format!(
            "searched target/{{release,debug}}/valori-node under {}",
            root.display()
        )))
    }

    fn running_info(&self, name: &str, node: &RunningNode) -> NodeInfo {
        NodeInfo {
            name: name.to_string(),
            status: node.state,
            pid: node.process.pid(),
            port: Some(node.port),
            uptime_secs: Some(node.started.elapsed().as_secs()),
            node_id: None,
        }
    }

    fn launch_spec(&self, project: &Project, port: u16) -> LaunchSpec {
        let mut env = HashMap::new();
        env.insert("VALORI_BIND".into(), format!("127.0.0.1:{port}"));
        env.insert(
            "VALORI_EVENT_LOG_PATH".into(),
            project.event_log_path().display().to_string(),
        );
        env.insert(
            "VALORI_SNAPSHOT_PATH".into(),
            project.snapshot_path().display().to_string(),
        );
        // Phase 2.3: every daemon-spawned standalone node now also gets a
        // StorageProvider root + its durable project identity — this is
        // what makes the manifest-driven snapshot+WAL-tail recovery path
        // (`valori-engine::Engine::try_recover`) the one actually used by
        // a normally-created local project, with no separate opt-in step.
        // `project.dir` is already this project's own isolated directory
        // (`~/.valori/projects/<name>/`); `LocalStorageProvider` creates
        // its own `projects/<project_id>/...` layout underneath it, so a
        // legacy project's existing `events.log`/`snapshot.val`/
        // `namespaces.json` (set above/below) are untouched siblings, not
        // overwritten or migrated by this alone — see the phase report's
        // migration section for what happens to those.
        env.insert(
            "VALORI_STORAGE_ROOT".into(),
            project.dir.join("storage").display().to_string(),
        );
        env.insert("VALORI_PROJECT_ID".into(), project.config.id.clone());
        env.insert("VALORI_PROJECT_NAME".into(), project.config.name.clone());
        env.insert(
            "RUST_LOG".into(),
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        );
        LaunchSpec {
            program: self.binary.clone(),
            env,
            log_path: project.dir.join("node.log"),
        }
    }

    // ── Cluster (RFC-0007) ───────────────────────────────────────────────────

    /// `VALORI_CLUSTER_MEMBERS` value: `id=raft_addr/api_addr,...`. Mirrors
    /// `buildMembers()` in `ui/src/lib/server/cluster-config.ts` — same
    /// fallback (`3100 + id`) when a node's `raft_port` wasn't persisted.
    fn build_members(nodes: &[ProjectNode]) -> String {
        nodes
            .iter()
            .map(|n| {
                let raft_port = n.raft_port.unwrap_or(3100 + n.id as u16);
                format!("{}=127.0.0.1:{raft_port}/127.0.0.1:{}", n.id, n.http_port)
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    fn cluster_launch_spec(
        &self,
        project: &Project,
        node: &ProjectNode,
        members: &str,
        init: bool,
        shard_count: u32,
    ) -> LaunchSpec {
        let raft_port = node.raft_port.unwrap_or(3100 + node.id as u16);
        let mut env = HashMap::new();
        env.insert(
            "VALORI_BIND".into(),
            format!("127.0.0.1:{}", node.http_port),
        );
        env.insert(
            "VALORI_EVENT_LOG_PATH".into(),
            project.node_event_log_path(node.id).display().to_string(),
        );
        env.insert(
            "VALORI_SNAPSHOT_PATH".into(),
            project.node_snapshot_path(node.id).display().to_string(),
        );
        env.insert("VALORI_NODE_ID".into(), node.id.to_string());
        env.insert("VALORI_CLUSTER_MEMBERS".into(), members.to_string());
        env.insert("VALORI_RAFT_BIND".into(), format!("127.0.0.1:{raft_port}"));
        env.insert(
            "VALORI_RAFT_LOG_PATH".into(),
            project.node_raft_log_path(node.id).display().to_string(),
        );
        env.insert("VALORI_SHARD_COUNT".into(), shard_count.to_string());
        if init {
            env.insert("VALORI_CLUSTER_INIT".into(), "1".into());
        }
        env.insert(
            "RUST_LOG".into(),
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        );
        LaunchSpec {
            program: self.binary.clone(),
            env,
            log_path: project.node_log_path(node.id),
        }
    }

    /// Start (or top up) a cluster project: launch any `ProjectNode` that
    /// isn't already running, leave already-running peers untouched. This is
    /// what gives crash recovery "restart only the dead node" behavior for
    /// free — `Supervisor` just calls `start()` again on any crash, and this
    /// only relaunches what's missing.
    async fn start_cluster(
        &mut self,
        project: &Project,
        cfg: &ClusterConfig,
    ) -> DaemonResult<NodeInfo> {
        let name = project.config.name.clone();
        let members = Self::build_members(&cfg.nodes);
        let bootstrap_id = cfg.nodes.iter().map(|n| n.id).min().unwrap_or(1);

        let already: std::collections::HashSet<u32> = self
            .cluster_running
            .get(&name)
            .map(|nodes| nodes.iter().map(|n| n.id).collect())
            .unwrap_or_default();

        self.cluster_running.entry(name.clone()).or_default();
        for node in &cfg.nodes {
            if already.contains(&node.id) {
                continue;
            }
            let spec = self.cluster_launch_spec(
                project,
                node,
                &members,
                node.id == bootstrap_id,
                cfg.shard_count,
            );
            let process = self.launcher.launch(&spec)?;
            let mut state = RuntimeState::Stopped;
            state.transition(RuntimeState::Starting)?;
            self.cluster_running
                .get_mut(&name)
                .expect("just inserted above")
                .push(ClusterRunningNode {
                    id: node.id,
                    process,
                    port: node.http_port,
                    started: Instant::now(),
                    state,
                });
        }

        // Wait for every node to answer healthy — nodes that were already
        // running from a prior call answer immediately, so this only really
        // waits on the ones just launched above.
        for node in &cfg.nodes {
            if let Err(e) = wait_health(node.http_port, std::time::Duration::from_secs(15)).await {
                return Err(DaemonError::StartFailed(format!(
                    "cluster node {} did not become healthy: {e}",
                    node.id
                )));
            }
        }

        if let Some(nodes) = self.cluster_running.get_mut(&name) {
            for n in nodes.iter_mut() {
                if n.state == RuntimeState::Starting {
                    n.state.transition(RuntimeState::Running)?;
                }
            }
        }

        Ok(self.cluster_status_info(&name))
    }

    /// Graceful snapshot-then-terminate for every node in a cluster project —
    /// same sequence as the single-node `stop()`, just looped.
    async fn stop_cluster(&mut self, name: &str) -> NodeInfo {
        if let Some(nodes) = self.cluster_running.remove(name) {
            for mut n in nodes {
                let _ = n.state.transition(RuntimeState::Stopping);
                let _ = reqwest::Client::new()
                    .post(format!("http://127.0.0.1:{}/v1/snapshot/save", n.port))
                    .timeout(std::time::Duration::from_secs(5))
                    .send()
                    .await;
                n.process.terminate();
            }
        }
        NodeInfo::stopped(name)
    }

    /// Aggregate a cluster project's nodes into the one `NodeInfo` `status()`
    /// returns: `Running` iff every node is, `port`/`pid` from the lowest-id
    /// (bootstrap) node as the entry point.
    fn cluster_status_info(&self, name: &str) -> NodeInfo {
        let nodes = self
            .cluster_running
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        let status = if nodes.is_empty() {
            RuntimeState::Stopped
        } else if nodes.iter().all(|n| n.state == RuntimeState::Running) {
            RuntimeState::Running
        } else if nodes
            .iter()
            .any(|n| n.state == RuntimeState::Running || n.state == RuntimeState::Starting)
        {
            RuntimeState::Starting
        } else {
            RuntimeState::Failed
        };
        let bootstrap = nodes.iter().min_by_key(|n| n.id);
        NodeInfo {
            name: name.to_string(),
            status,
            pid: bootstrap.and_then(|n| n.process.pid()),
            port: bootstrap.map(|n| n.port),
            uptime_secs: bootstrap.map(|n| n.started.elapsed().as_secs()),
            node_id: None,
        }
    }
}

#[async_trait::async_trait]
impl Runtime for LocalRuntime {
    fn kind(&self) -> &'static str {
        "local"
    }

    async fn start(&mut self, project: &Project) -> DaemonResult<NodeInfo> {
        if let Some(cfg) = project
            .config
            .cluster
            .as_ref()
            .filter(|c| c.replication > 1)
        {
            return self.start_cluster(project, cfg).await;
        }

        let name = &project.config.name;
        if let Some(node) = self.running.get(name) {
            return Ok(self.running_info(name, node));
        }

        let taken = self.running.values().map(|n| n.port).collect();
        let port = self.ports.allocate(&taken)?;

        // Stopped → Starting → (Running | Failed).
        let mut state = RuntimeState::Stopped;
        state.transition(RuntimeState::Starting)?;

        let process = self.launcher.launch(&self.launch_spec(project, port))?;
        self.running.insert(
            name.clone(),
            RunningNode {
                process,
                port,
                started: Instant::now(),
                state,
            },
        );

        if let Err(e) = wait_health(port, std::time::Duration::from_secs(15)).await {
            if let Some(mut node) = self.running.remove(name) {
                node.process.terminate();
            }
            return Err(DaemonError::StartFailed(format!(
                "node did not become healthy: {e}"
            )));
        }

        let node = self.running.get_mut(name).expect("just inserted");
        node.state.transition(RuntimeState::Running)?;
        Ok(self.running_info(name, self.running.get(name).unwrap()))
    }

    async fn stop(&mut self, name: &str) -> DaemonResult<NodeInfo> {
        if self.cluster_running.contains_key(name) {
            return Ok(self.stop_cluster(name).await);
        }
        if let Some(mut node) = self.running.remove(name) {
            // Running → Stopping, best-effort snapshot, then terminate.
            let _ = node.state.transition(RuntimeState::Stopping);
            let _ = reqwest::Client::new()
                .post(format!("http://127.0.0.1:{}/v1/snapshot/save", node.port))
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await;
            node.process.terminate();
        }
        Ok(NodeInfo::stopped(name))
    }

    async fn restart(&mut self, project: &Project) -> DaemonResult<NodeInfo> {
        self.stop(&project.config.name).await?;
        self.start(project).await
    }

    fn status(&self, name: &str) -> NodeInfo {
        if self.cluster_running.contains_key(name) {
            return self.cluster_status_info(name);
        }
        match self.running.get(name) {
            Some(node) => self.running_info(name, node),
            None => NodeInfo::stopped(name),
        }
    }

    fn is_running(&self, name: &str) -> bool {
        self.running.contains_key(name)
            || self
                .cluster_running
                .get(name)
                .map(|v| !v.is_empty())
                .unwrap_or(false)
    }

    fn running_count(&self) -> usize {
        self.running.len()
            + self
                .cluster_running
                .values()
                .filter(|v| !v.is_empty())
                .count()
    }

    fn port_of(&self, name: &str) -> Option<u16> {
        if let Some(nodes) = self.cluster_running.get(name) {
            return nodes.iter().min_by_key(|n| n.id).map(|n| n.port);
        }
        self.running.get(name).map(|n| n.port)
    }

    fn resources(&self, name: &str) -> Option<ResourceStats> {
        if let Some(nodes) = self.cluster_running.get(name) {
            let bootstrap = nodes.iter().min_by_key(|n| n.id)?;
            let pid = bootstrap.process.pid()?;
            return ResourceMonitor::sample(pid, bootstrap.started.elapsed().as_secs());
        }
        let node = self.running.get(name)?;
        let pid = node.process.pid()?;
        ResourceMonitor::sample(pid, node.started.elapsed().as_secs())
    }

    fn poll_exits(&mut self) -> Vec<crate::runtime::NodeExit> {
        let mut exits = Vec::new();
        let mut dead = Vec::new();
        for (name, node) in self.running.iter_mut() {
            if let Some(reason) = node.process.has_exited() {
                exits.push(crate::runtime::NodeExit {
                    name: name.clone(),
                    reason,
                });
                dead.push(name.clone());
            }
        }
        for name in dead {
            self.running.remove(&name);
        }

        // Cluster projects: a dead node is dropped from its project's Vec
        // (not the whole project) — the survivors keep running, and the next
        // `start()` call (Supervisor's crash-recovery path) only relaunches
        // what's missing.
        let mut empty_projects = Vec::new();
        for (name, nodes) in self.cluster_running.iter_mut() {
            let mut dead_ids = Vec::new();
            for n in nodes.iter_mut() {
                if let Some(reason) = n.process.has_exited() {
                    exits.push(NodeExit {
                        name: name.clone(),
                        reason: format!("node {}: {reason}", n.id),
                    });
                    dead_ids.push(n.id);
                }
            }
            nodes.retain(|n| !dead_ids.contains(&n.id));
            if nodes.is_empty() {
                empty_projects.push(name.clone());
            }
        }
        for name in empty_projects {
            self.cluster_running.remove(&name);
        }

        exits
    }

    async fn stop_all(&mut self) {
        // Same graceful snapshot-then-terminate as `stop()` — previously this
        // hard-killed every running node with no snapshot at all, so a daemon
        // shutdown (Ctrl-C, or the desktop app closing) silently skipped the
        // "save a snapshot before the process dies" step that `stop()` always
        // does. One node's snapshot POST failing/hanging no longer blocks the
        // rest — each is bounded by its own 5s timeout and they don't share state.
        let nodes: Vec<_> = self.running.drain().collect();
        for (_, mut node) in nodes {
            let _ = node.state.transition(RuntimeState::Stopping);
            let _ = reqwest::Client::new()
                .post(format!("http://127.0.0.1:{}/v1/snapshot/save", node.port))
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await;
            node.process.terminate();
        }

        let cluster_projects: Vec<_> = self.cluster_running.drain().collect();
        for (_, cluster_nodes) in cluster_projects {
            for mut n in cluster_nodes {
                let _ = n.state.transition(RuntimeState::Stopping);
                let _ = reqwest::Client::new()
                    .post(format!("http://127.0.0.1:{}/v1/snapshot/save", n.port))
                    .timeout(std::time::Duration::from_secs(5))
                    .send()
                    .await;
                n.process.terminate();
            }
        }
    }

    fn describe(&self) -> serde_json::Value {
        let (lo, hi) = self.ports.range();
        serde_json::json!({
            "kind": "local",
            "node_binary": self.binary.display().to_string(),
            "node_port_range": { "from": lo, "to": hi },
        })
    }

    fn cluster_nodes(&self, name: &str) -> Vec<NodeInfo> {
        match self.cluster_running.get(name) {
            Some(nodes) => nodes
                .iter()
                .map(|n| NodeInfo {
                    name: name.to_string(),
                    status: n.state,
                    pid: n.process.pid(),
                    port: Some(n.port),
                    uptime_secs: Some(n.started.elapsed().as_secs()),
                    node_id: Some(n.id),
                })
                .collect(),
            None => Vec::new(),
        }
    }
}

/// Poll `GET /health` until it answers 2xx or `timeout` elapses.
async fn wait_health(port: u16, timeout: std::time::Duration) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{port}/health");
    let client = reqwest::Client::new();
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Ok(resp) = client
            .get(&url)
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
        {
            if resp.status().is_success() {
                return Ok(());
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("timed out after {timeout:?}"));
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
}
