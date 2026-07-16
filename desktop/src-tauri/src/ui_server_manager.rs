//! Owns the bundled `ui/` (Next.js standalone) server — release builds only.
//!
//! `tauri dev` still runs `ui/`'s own `next dev` server via `beforeDevCommand`
//! (`scripts/dev.mjs`) exactly as before; nothing here runs in dev mode. In a
//! packaged app there is no static frontend to embed (`ui/` still has
//! server-side API routes — RFC-0006's static-export migration hasn't landed
//! yet), so instead the desktop bundles the Next.js standalone build as a
//! resource (`prepare-ui-server.mjs`) and a copy of the Node runtime as a
//! sidecar (`prepare-sidecars.mjs`) to execute it. The main window starts on
//! a small local loading page (`frontendDist`) and is navigated to the real
//! server once it's healthy.

use std::sync::Mutex;
use std::time::Duration;

use tauri::path::BaseDirectory;
use tauri::{Manager, Url};
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

/// Fixed — this is a loopback server only the desktop itself ever talks to,
/// so there's no port-conflict risk worth dynamic allocation for.
pub const UI_SERVER_PORT: u16 = 17862;

pub struct UiServerState(Mutex<Option<tauri_plugin_shell::process::CommandChild>>);

impl Default for UiServerState {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

async fn probe(url: &str) -> bool {
    reqwest::Client::new()
        .get(url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .is_ok()
}

/// Spawn the bundled `ui/` server and wait for it to answer, then navigate
/// the main window to it. Release builds only — call from `setup()`.
pub async fn start_and_navigate(app: &tauri::AppHandle, state: &UiServerState) -> Result<(), String> {
    eprintln!("[ui-server] resolving bundled resource path…");
    let server_js = app
        .path()
        .resolve("ui-server/server.js", BaseDirectory::Resource)
        .map_err(|e| format!("could not resolve bundled ui-server resource: {e}"))?;
    eprintln!("[ui-server] resolved server.js at {}", server_js.display());
    eprintln!("[ui-server] exists on disk: {}", server_js.exists());

    // `ui/`'s legacy cluster-project launcher (`process-manager.ts`) still
    // spawns `valori-node` itself (the daemon can't launch clusters yet — see
    // RFC-0006) and, before this, only knew how to find it via a
    // dev-checkout `target/` search relative to its own cwd — which resolves
    // to nowhere useful once `ui/` is running as this bundled sidecar. Same
    // fix as the daemon: hand it the bundled sidecar's real path directly.
    let valori_node_bin = crate::daemon_manager::sidecar_sibling_path("valori-node")?;
    eprintln!("[ui-server] VALORI_NODE_BIN={} (exists: {})", valori_node_bin.display(), valori_node_bin.exists());

    // Node is launched from inside ValoriUIServer.app — a minimal helper
    // app bundle whose Info.plist has LSUIElement=YES. When macOS resolves
    // the executable's bundle and finds LSUIElement=YES, _RegisterApplication()
    // registers it as a UIElement instead of a Foreground app, so no Dock icon
    // appears. This is the only reliable macOS approach: pre-exec() API calls
    // (CGSSetConnectionProperty, TransformProcessType) are overridden by
    // libuv's own _RegisterApplication() call in the new process image.
    let helper_node_bin = app
        .path()
        .resolve(
            "ValoriUIServer.app/Contents/MacOS/node",
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|e| format!("could not resolve helper node binary: {e}"))?;
    eprintln!("[ui-server] helper node bin={} (exists: {})", helper_node_bin.display(), helper_node_bin.exists());

    eprintln!("[ui-server] spawning helper node…");
    let (mut rx, child) = app
        .shell()
        .command(helper_node_bin.to_str().ok_or("helper node path is not valid UTF-8")?)
        .args([server_js.display().to_string()])
        .env("PORT", UI_SERVER_PORT.to_string())
        .env("HOSTNAME", "127.0.0.1")
        .env("VALORI_NODE_BIN", valori_node_bin.display().to_string())
        .spawn()
        .map_err(|e| format!("failed to spawn ui-server: {e}"))?;
    eprintln!("[ui-server] spawned helper node, pid={}", child.pid());

    // Surface anything unexpected on stdout/stderr instead of failing silently.
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stderr(bytes) => {
                    eprintln!("[ui-server:stderr] {}", String::from_utf8_lossy(&bytes));
                }
                CommandEvent::Stdout(bytes) => {
                    eprintln!("[ui-server:stdout] {}", String::from_utf8_lossy(&bytes));
                }
                CommandEvent::Error(e) => {
                    eprintln!("[ui-server:error] {e}");
                }
                CommandEvent::Terminated(payload) => {
                    eprintln!("[ui-server:node] terminated: {payload:?}");
                }
                _ => {}
            }
        }
    });

    let url = format!("http://127.0.0.1:{UI_SERVER_PORT}");
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        if probe(&url).await {
            eprintln!("[ui-server] healthy at {url}");
            break;
        }
        if std::time::Instant::now() >= deadline {
            eprintln!("[ui-server] never became healthy within 20s");
            let _ = child.kill();
            return Err("bundled ui-server did not become healthy within 20s".into());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    *state.0.lock().unwrap() = Some(child);

    let window = app.get_webview_window("main").ok_or("main window not found")?;
    eprintln!("[ui-server] navigating main window to {url}");
    window
        .navigate(Url::parse(&url).expect("valid loopback URL"))
        .map_err(|e| format!("failed to navigate main window: {e}"))
}

/// Kill the ui-server sidecar — call alongside daemon shutdown on app exit.
pub fn stop(state: &UiServerState) {
    if let Some(child) = state.0.lock().unwrap().take() {
        let _ = child.kill();
    }
}
