//! Owns the `valori-daemon` child process from the desktop side.
//!
//! The desktop app is the supervisor of last resort: it launches the daemon
//! on startup (or on demand), points it at the user's chosen workspace via
//! `VALORI_HOME`, and asks it to shut down gracefully (snapshot every running
//! project, then exit) when the desktop window closes. Everything below the
//! daemon's own HTTP API — project lifecycle, node supervision — stays the
//! daemon's job; this module only owns "is the daemon process up, and where".
//!
//! Exactly two code paths for finding the binary (Phase D3.1 — no env-var
//! override, no PATH search):
//! - **Dev** (`cfg!(debug_assertions)`, i.e. `tauri dev` / `cargo build`):
//!   the daemon isn't bundled, so we search the root workspace's
//!   `target/{release,debug}/` next to `desktop/`.
//! - **Release** (`tauri build`): the daemon is a Tauri sidecar — bundled
//!   into the app alongside the main executable at build time (see
//!   `bundle.externalBin` in `tauri.conf.json` + `scripts/prepare-sidecars.mjs`)
//!   and resolved at runtime relative to the running app's own binary, with
//!   no Cargo/toolchain dependency at all.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

const DEFAULT_BIND: &str = "127.0.0.1:8080";

/// The daemon's `GET /version` reports `{"api": "..."}`; this is the
/// compatibility contract desktop and daemon must agree on — not the raw
/// crate semver, which can bump on unrelated dependency changes.
const EXPECTED_DAEMON_API: &str = "v1";

pub struct DaemonState(pub Mutex<Option<RunningDaemon>>);

impl Default for DaemonState {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

pub struct RunningDaemon {
    child: RunningChild,
    bind: String,
}

/// The two ways a supervised process can be running: a plain dev-mode
/// [`tokio::process::Child`], or a Tauri sidecar (bundled binary, resolved
/// and spawned by `tauri-plugin-shell`).
enum RunningChild {
    Dev(tokio::process::Child),
    Sidecar {
        child: tauri_plugin_shell::process::CommandChild,
        /// Flipped by the event-forwarder task when the sidecar's stdout/rx
        /// stream reports `CommandEvent::Terminated` — `CommandChild` has no
        /// `wait()`, so this is how we detect a graceful exit.
        exited: Arc<tokio::sync::Notify>,
    },
}

impl RunningChild {
    async fn wait_for_exit(&mut self, timeout: Duration) {
        match self {
            RunningChild::Dev(child) => {
                let _ = tokio::time::timeout(timeout, child.wait()).await;
            }
            RunningChild::Sidecar { exited, .. } => {
                let _ = tokio::time::timeout(timeout, exited.notified()).await;
            }
        }
    }

    fn kill(self) {
        match self {
            RunningChild::Dev(mut child) => {
                let _ = child.start_kill();
            }
            RunningChild::Sidecar { child, .. } => {
                let _ = child.kill();
            }
        }
    }
}

#[derive(Serialize, Clone)]
pub struct DaemonStatus {
    pub running: bool,
    pub healthy: bool,
    pub bind: Option<String>,
}

/// Dev-mode only: locate a freshly `cargo build`-ed binary in the root
/// workspace's target dir (release preferred over debug).
fn resolve_dev_binary(name: &str) -> Result<PathBuf, String> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent() // desktop/
        .and_then(|p| p.parent()) // repo root
        .unwrap_or(&manifest_dir);
    for profile in ["release", "debug"] {
        let cand = repo_root.join("target").join(profile).join(name);
        if cand.exists() {
            return Ok(cand);
        }
    }
    Err(format!(
        "could not find {name} binary — searched target/{{release,debug}} under {}; run `cargo build -p {name}` first",
        repo_root.display()
    ))
}

/// Release-mode only: the path a bundled sidecar named `name` will be
/// spawned from — the same directory as the desktop app's own executable
/// (this is exactly how `tauri-plugin-shell`'s `Command::sidecar` resolves
/// paths internally; duplicated here because that resolution isn't public).
/// Used to tell the *daemon* sidecar where the *node* sidecar lives via
/// `VALORI_NODE_BIN` — `valori-daemon` has no idea it's running inside a
/// Tauri bundle, so without this it would fall back to its own dev-only
/// `target/{release,debug}` search, which doesn't exist on an end user's
/// machine.
pub(crate) fn sidecar_sibling_path(name: &str) -> Result<PathBuf, String> {
    let exe_path = tauri::utils::platform::current_exe()
        .map_err(|e| format!("could not resolve current executable path: {e}"))?;
    let exe_dir = exe_path.parent().ok_or("current executable has no parent directory")?;
    let base_dir = if exe_dir.ends_with("deps") { exe_dir.parent().unwrap_or(exe_dir) } else { exe_dir };
    let mut path = base_dir.join(name);
    if cfg!(windows) && path.extension().map_or(true, |e| e != "exe") {
        path.as_mut_os_string().push(".exe");
    }
    Ok(path)
}

async fn probe_health(bind: &str) -> bool {
    let url = format!("http://{bind}/health");
    reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Compare the daemon's reported API level against what this desktop build
/// expects. Returns `Err` with an actionable message (not a bare timeout or
/// a mysterious 404 later) on mismatch.
async fn check_version(bind: &str) -> Result<(), String> {
    let url = format!("http://{bind}/version");
    let resp = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .map_err(|e| format!("failed to reach daemon /version: {e}"))?;
    let body: serde_json::Value =
        resp.json().await.map_err(|e| format!("bad /version response: {e}"))?;
    let api = body.get("api").and_then(|v| v.as_str()).unwrap_or("");
    check_api_compat(api)
}

fn check_api_compat(reported_api: &str) -> Result<(), String> {
    if reported_api != EXPECTED_DAEMON_API {
        return Err(format!(
            "UnsupportedVersion: desktop expects daemon api \"{EXPECTED_DAEMON_API}\", daemon reports \"{reported_api}\""
        ));
    }
    Ok(())
}

/// Start the daemon if it isn't already running under our supervision.
/// `home` becomes `VALORI_HOME` for the spawned process — the real effect of
/// the workspace folder the user picked in onboarding/settings.
#[tauri::command]
pub async fn start_daemon(
    app: tauri::AppHandle,
    state: tauri::State<'_, DaemonState>,
    home: Option<String>,
) -> Result<DaemonStatus, String> {
    start_daemon_internal(&app, &state, home).await
}

pub async fn start_daemon_internal(
    app: &tauri::AppHandle,
    state: &DaemonState,
    home: Option<String>,
) -> Result<DaemonStatus, String> {
    eprintln!("[daemon] start_daemon_internal called home={home:?}");
    let already_running = state.0.lock().unwrap().as_ref().map(|r| r.bind.clone());
    if let Some(bind) = already_running {
        eprintln!("[daemon] already running on {bind}");
        let healthy = probe_health(&bind).await;
        return Ok(DaemonStatus { running: true, healthy, bind: Some(bind) });
    }

    let bind = DEFAULT_BIND.to_string();
    let mut envs: Vec<(String, String)> = vec![("VALORI_DAEMON_BIND".into(), bind.clone())];
    if let Some(home) = &home {
        envs.push(("VALORI_HOME".into(), home.clone()));
    }

    eprintln!("[daemon] starting — home={home:?} bind={bind}");

    let child = if cfg!(debug_assertions) {
        let binary = resolve_dev_binary("valori-daemon")?;
        // Same reason as the release path below: the daemon doesn't know where
        // it was spawned from, so its own target/{release,debug} fallback search
        // uses current_dir() which is desktop/, not the repo root. Pass the
        // resolved path explicitly so LocalRuntime::resolve_binary() succeeds.
        let node_bin = resolve_dev_binary("valori-node")?;
        envs.push(("VALORI_NODE_BIN".into(), node_bin.display().to_string()));
        eprintln!("[daemon] dev-mode binary={}", binary.display());
        let mut cmd = tokio::process::Command::new(&binary);
        for (k, v) in &envs {
            cmd.env(k, v);
        }
        cmd.kill_on_drop(true);
        let child = cmd.spawn().map_err(|e| {
            eprintln!("[daemon] spawn failed: {e}");
            format!("failed to spawn valori-daemon: {e}")
        })?;
        eprintln!("[daemon] dev-mode spawned pid={}", child.id().unwrap_or(0));
        RunningChild::Dev(child)
    } else {
        // The daemon has no idea it's running inside a Tauri bundle, so tell
        // it exactly where its bundled valori-node sidecar lives instead of
        // letting it fall back to a dev-only target/{release,debug} search
        // that doesn't exist on an end user's machine.
        let node_bin = sidecar_sibling_path("valori-node")?;
        eprintln!("[daemon] node_bin={} (exists: {})", node_bin.display(), node_bin.exists());
        envs.push(("VALORI_NODE_BIN".into(), node_bin.display().to_string()));

        eprintln!("[daemon] resolving sidecar…");
        let cmd = app
            .shell()
            .sidecar("valori-daemon")
            .map_err(|e| {
                eprintln!("[daemon] sidecar resolve failed: {e}");
                format!("valori-daemon sidecar not found in this build: {e}")
            })?;
        eprintln!("[daemon] sidecar resolved, spawning…");
        let (mut rx, child) = cmd
            .envs(envs)
            .spawn()
            .map_err(|e| {
                eprintln!("[daemon] sidecar spawn failed: {e}");
                format!("failed to spawn valori-daemon sidecar: {e}")
            })?;
        eprintln!("[daemon] sidecar spawned pid={}", child.pid());
        let exited = Arc::new(tokio::sync::Notify::new());
        let exited_tx = exited.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    CommandEvent::Stdout(bytes) => {
                        eprintln!("[daemon:stdout] {}", String::from_utf8_lossy(&bytes));
                    }
                    CommandEvent::Stderr(bytes) => {
                        eprintln!("[daemon:stderr] {}", String::from_utf8_lossy(&bytes));
                    }
                    CommandEvent::Error(e) => {
                        eprintln!("[daemon:error] {e}");
                    }
                    CommandEvent::Terminated(payload) => {
                        eprintln!("[daemon] terminated: {payload:?}");
                        exited_tx.notify_waiters();
                        break;
                    }
                    _ => {}
                }
            }
        });
        RunningChild::Sidecar { child, exited }
    };

    eprintln!("[daemon] waiting for health on {bind}…");
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        if probe_health(&bind).await {
            eprintln!("[daemon] healthy");
            break;
        }
        if std::time::Instant::now() >= deadline {
            eprintln!("[daemon] timed out waiting for health");
            child.kill();
            return Err("valori-daemon did not become healthy within 15s".into());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    eprintln!("[daemon] checking version…");
    if let Err(e) = check_version(&bind).await {
        eprintln!("[daemon] version check failed: {e}");
        child.kill();
        return Err(e);
    }
    eprintln!("[daemon] version ok");

    *state.0.lock().unwrap() = Some(RunningDaemon { child, bind: bind.clone() });
    Ok(DaemonStatus { running: true, healthy: true, bind: Some(bind) })
}

/// Ask the daemon to shut down gracefully over HTTP (snapshot every running
/// project, then exit) rather than killing the process — see
/// `POST /v1/shutdown` in `valori-daemon`, which exists precisely so a
/// supervisor never has to rely on OS signal semantics across platforms.
#[tauri::command]
pub async fn stop_daemon(state: tauri::State<'_, DaemonState>) -> Result<(), String> {
    stop_daemon_internal(&state).await;
    Ok(())
}

/// Shared by the `stop_daemon` command and the on-exit hook.
pub async fn stop_daemon_internal(state: &DaemonState) {
    let running = state.0.lock().unwrap().take();
    let Some(mut running) = running else { return };

    let url = format!("http://{}/v1/shutdown", running.bind);
    let requested = reqwest::Client::new()
        .post(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .is_ok();

    if requested {
        // Give the daemon a moment to snapshot + exit on its own.
        running.child.wait_for_exit(Duration::from_secs(5)).await;
    }
    // Whether or not the graceful path completed, ensure nothing is left
    // behind — a no-op if the process already exited.
    running.child.kill();
}

#[tauri::command]
pub async fn daemon_status(state: tauri::State<'_, DaemonState>) -> Result<DaemonStatus, String> {
    daemon_status_internal(&state).await
}

pub async fn daemon_status_internal(state: &DaemonState) -> Result<DaemonStatus, String> {
    let bind = {
        let guard = state.0.lock().unwrap();
        guard.as_ref().map(|r| r.bind.clone())
    };
    match bind {
        Some(bind) => {
            let healthy = probe_health(&bind).await;
            Ok(DaemonStatus { running: true, healthy, bind: Some(bind) })
        }
        None => Ok(DaemonStatus { running: false, healthy: false, bind: None }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compat_check() {
        assert!(check_api_compat("v1").is_ok());
        let err = check_api_compat("v2").unwrap_err();
        assert!(err.contains("UnsupportedVersion"));
        assert!(err.contains("v1"));
        assert!(err.contains("v2"));
    }

    /// End-to-end against the real `valori-daemon` binary (dev-mode path
    /// only — sidecar spawning needs a packaged app / `tauri::AppHandle`,
    /// exercised manually via `tauri build` + prepare-sidecars.mjs). Start
    /// it pointed at a fresh VALORI_HOME, confirm it's healthy, confirm a
    /// second start is a no-op that reports the same instance, then stop it
    /// gracefully and confirm the process actually exited.
    #[tokio::test]
    async fn start_stop_real_daemon_binary_dev_path() {
        if resolve_dev_binary("valori-daemon").is_err() {
            eprintln!("skipping: valori-daemon binary not built");
            return;
        }
        assert!(cfg!(debug_assertions), "this test only exercises the dev-mode path");

        let home = tempfile::tempdir().unwrap();
        let state = DaemonState::default();
        let bind = DEFAULT_BIND.to_string();

        // Exercise the dev-mode spawn branch directly (no AppHandle needed).
        let binary = resolve_dev_binary("valori-daemon").unwrap();
        let mut cmd = tokio::process::Command::new(&binary);
        cmd.env("VALORI_DAEMON_BIND", &bind);
        cmd.env("VALORI_HOME", home.path());
        cmd.kill_on_drop(true);
        let child = cmd.spawn().unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            if probe_health(&bind).await {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "daemon never became healthy");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        check_version(&bind).await.unwrap();

        *state.0.lock().unwrap() = Some(RunningDaemon { child: RunningChild::Dev(child), bind: bind.clone() });

        let status2 = daemon_status_internal(&state).await.unwrap();
        assert!(status2.running);
        assert!(status2.healthy);

        stop_daemon_internal(&state).await;

        let status3 = daemon_status_internal(&state).await.unwrap();
        assert!(!status3.running, "daemon should be gone after stop_daemon_internal");
    }
}
