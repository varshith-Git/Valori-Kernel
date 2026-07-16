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
use crate::project::Project;
use crate::runtime::launcher::{LaunchSpec, Launcher, LocalLauncher, RunningProcess};
use crate::runtime::port::PortAllocator;
use crate::runtime::resource::{ResourceMonitor, ResourceStats};
use crate::runtime::state::RuntimeState;
use crate::runtime::{NodeInfo, Runtime};

const PORT_LO: u16 = 8100;
const PORT_HI: u16 = 8999;

struct RunningNode {
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
        })
    }

    fn resolve_binary() -> DaemonResult<PathBuf> {
        if let Ok(p) = std::env::var("VALORI_NODE_BIN") {
            let pb = PathBuf::from(&p);
            return if pb.exists() {
                Ok(pb)
            } else {
                Err(DaemonError::NodeBinaryMissing(format!("VALORI_NODE_BIN={p} does not exist")))
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
        }
    }

    fn launch_spec(&self, project: &Project, port: u16) -> LaunchSpec {
        let mut env = HashMap::new();
        env.insert("VALORI_BIND".into(), format!("127.0.0.1:{port}"));
        env.insert("VALORI_DIM".into(), project.config.dim.to_string());
        env.insert("VALORI_INDEX".into(), project.config.index.clone());
        env.insert("VALORI_EVENT_LOG_PATH".into(), project.event_log_path().display().to_string());
        env.insert("VALORI_SNAPSHOT_PATH".into(), project.snapshot_path().display().to_string());
        env.insert("RUST_LOG".into(), std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()));
        LaunchSpec { program: self.binary.clone(), env, log_path: project.dir.join("node.log") }
    }
}

#[async_trait::async_trait]
impl Runtime for LocalRuntime {
    fn kind(&self) -> &'static str {
        "local"
    }

    async fn start(&mut self, project: &Project) -> DaemonResult<NodeInfo> {
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
            RunningNode { process, port, started: Instant::now(), state },
        );

        if let Err(e) = wait_health(port, std::time::Duration::from_secs(15)).await {
            if let Some(mut node) = self.running.remove(name) {
                node.process.terminate();
            }
            return Err(DaemonError::StartFailed(format!("node did not become healthy: {e}")));
        }

        let node = self.running.get_mut(name).expect("just inserted");
        node.state.transition(RuntimeState::Running)?;
        Ok(self.running_info(name, self.running.get(name).unwrap()))
    }

    async fn stop(&mut self, name: &str) -> DaemonResult<NodeInfo> {
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
        match self.running.get(name) {
            Some(node) => self.running_info(name, node),
            None => NodeInfo::stopped(name),
        }
    }

    fn is_running(&self, name: &str) -> bool {
        self.running.contains_key(name)
    }

    fn running_count(&self) -> usize {
        self.running.len()
    }

    fn port_of(&self, name: &str) -> Option<u16> {
        self.running.get(name).map(|n| n.port)
    }

    fn resources(&self, name: &str) -> Option<ResourceStats> {
        let node = self.running.get(name)?;
        let pid = node.process.pid()?;
        ResourceMonitor::sample(pid, node.started.elapsed().as_secs())
    }

    fn poll_exits(&mut self) -> Vec<crate::runtime::NodeExit> {
        let mut exits = Vec::new();
        let mut dead = Vec::new();
        for (name, node) in self.running.iter_mut() {
            if let Some(reason) = node.process.has_exited() {
                exits.push(crate::runtime::NodeExit { name: name.clone(), reason });
                dead.push(name.clone());
            }
        }
        for name in dead {
            self.running.remove(&name);
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
    }

    fn describe(&self) -> serde_json::Value {
        let (lo, hi) = self.ports.range();
        serde_json::json!({
            "kind": "local",
            "node_binary": self.binary.display().to_string(),
            "node_port_range": { "from": lo, "to": hi },
        })
    }
}

/// Poll `GET /health` until it answers 2xx or `timeout` elapses.
async fn wait_health(port: u16, timeout: std::time::Duration) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{port}/health");
    let client = reqwest::Client::new();
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Ok(resp) = client.get(&url).timeout(std::time::Duration::from_secs(2)).send().await {
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
