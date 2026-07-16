// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `Launcher` — one job: turn a [`LaunchSpec`] into a running process.
//!
//! The [`Runtime`](super::Runtime) *orchestrates* (health, state, resources,
//! restart); the `Launcher` only *launches*. This split is what lets
//! `DockerLauncher` (bollard) or `SshLauncher` (openssh) drop in without the
//! runtime knowing how a process is spawned.
//!
//! A launched process is itself abstract ([`RunningProcess`]) — for local it
//! wraps `std::process::Child`; for Docker it would wrap a container id.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::error::{DaemonError, DaemonResult};

/// Everything needed to launch a node, with zero knowledge of *how*.
pub struct LaunchSpec {
    pub program: PathBuf,
    pub env: HashMap<String, String>,
    /// File to which stdout+stderr are redirected (for `GET .../logs`).
    pub log_path: PathBuf,
}

/// A launched process, abstracted over the backend.
pub trait RunningProcess: Send + Sync {
    /// OS pid if the backend has one (local: yes; remote: maybe not).
    fn pid(&self) -> Option<u32>;
    /// Terminate and reap.
    fn terminate(&mut self);
    /// Non-blocking liveness check: `Some(reason)` if the process has exited
    /// (crash detection), `None` if still running.
    fn has_exited(&mut self) -> Option<String>;
}

/// Launches processes for a runtime. One impl per backend.
pub trait Launcher: Send + Sync {
    fn launch(&self, spec: &LaunchSpec) -> DaemonResult<Box<dyn RunningProcess>>;
}

// ── Local implementation (std::process) ────────────────────────────────────────

/// Wraps a local child process.
pub struct LocalProcess {
    child: std::process::Child,
}

impl RunningProcess for LocalProcess {
    fn pid(&self) -> Option<u32> {
        Some(self.child.id())
    }
    fn terminate(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
    fn has_exited(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(format!("process exited: {status}")),
            Ok(None) => None,
            Err(e) => Some(format!("wait error: {e}")),
        }
    }
}

/// Launches nodes as local OS processes.
pub struct LocalLauncher;

impl Launcher for LocalLauncher {
    fn launch(&self, spec: &LaunchSpec) -> DaemonResult<Box<dyn RunningProcess>> {
        let log_out = std::fs::File::create(&spec.log_path)?;
        let log_err = log_out.try_clone()?;
        let child = Command::new(&spec.program)
            .envs(&spec.env)
            .stdout(Stdio::from(log_out))
            .stderr(Stdio::from(log_err))
            .spawn()
            .map_err(|e| DaemonError::StartFailed(e.to_string()))?;
        Ok(Box::new(LocalProcess { child }))
    }
}
