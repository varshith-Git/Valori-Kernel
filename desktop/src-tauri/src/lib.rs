//! Valori Desktop — Tauri control-plane shell.
//!
//! Native capabilities wired here:
//!  - macOS app menu (File / Edit / View / Help)
//!  - System tray with daemon status dot and quick-open
//!  - Window state persistence across launches (plugin-window-state)
//!  - Daemon lifecycle: start / stop / health-check
//!
//! See RFC-0006 (`rfcs/0006-desktop-daemon-architecture.md`) for the full plan.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::Datelike;
use serde::Serialize;
use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;

mod daemon_manager;
mod ui_server_manager;
use daemon_manager::{daemon_status, start_daemon, stop_daemon, stop_daemon_internal, DaemonState};
use ui_server_manager::UiServerState;
use tauri_plugin_updater::UpdaterExt;

/// Runs the shared shutdown sequence exactly once, then exits.
async fn shutdown_and_exit(app: tauri::AppHandle) {
    let ui_state = app.state::<UiServerState>();
    ui_server_manager::stop(&ui_state);
    let daemon_state = app.state::<DaemonState>();
    stop_daemon_internal(&daemon_state).await;
    std::process::exit(0);
}

/// Result of probing a Valori node/daemon's `/health` endpoint.
#[derive(Serialize)]
pub struct HealthReport {
    pub url: String,
    pub reachable: bool,
    pub body: String,
}

/// Register a file path with the OS "Open Recent" document list (macOS only).
/// On non-macOS platforms this is a no-op so the JS call is always safe.
#[tauri::command]
fn add_recent_document(_app: tauri::AppHandle, path: String) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSDocumentController;
        use objc2_foundation::{MainThreadMarker, NSString, NSURL};
        // NSDocumentController must be used from the main thread.
        if let Some(mtm) = MainThreadMarker::new() {
            let ns_path = NSString::from_str(&path);
            let url = NSURL::fileURLWithPath(&ns_path);
            let dc = NSDocumentController::sharedDocumentController(mtm);
            dc.noteNewRecentDocumentURL(&url);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = path;
}

#[tauri::command]
async fn node_health(base_url: String) -> Result<HealthReport, String> {
    let url = format!("{}/health", base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    match client.get(&url).send().await {
        Ok(resp) => {
            let reachable = resp.status().is_success();
            let body = resp.text().await.unwrap_or_default();
            Ok(HealthReport { url: base_url, reachable, body })
        }
        Err(e) => Ok(HealthReport { url: base_url, reachable: false, body: e.to_string() }),
    }
}

/// Download and apply the pending update, then restart.
/// Called from the JS "Install & Restart" button after an `update-available` event.
#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(());
    };
    update
        .download_and_install(|_downloaded, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

/// Navigate the main window to an in-app path.
fn nav_to(app: &tauri::AppHandle, path: &str) {
    if let Some(w) = app.get_webview_window("main") {
        let js = format!("window.location.href='{path}'");
        let _ = w.eval(&js);
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Show and focus the main window (used by tray click / "Open" menu item).
fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Build the macOS / Windows application menu.
///
/// macOS renders this as the global menu bar. Windows renders it inside the
/// window frame. The menu items fire `on_menu_event` below for app-specific
/// actions; standard editing (cut/copy/paste/select-all/undo/redo) is handled
/// by the webview via `PredefinedMenuItem` so the JS text fields work without
/// any extra wiring.
fn build_app_menu(app: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    // ── Valori (macOS only — the leftmost menu is the app name on macOS) ─────
    let app_submenu = Submenu::new(app, "Valori", true)?;
    let about_meta = AboutMetadata {
        name: Some("Valori".into()),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        short_version: None,
        authors: Some(vec!["Valori Team".into()]),
        comments: Some("Verifiable memory system for AI agents.\nDeterministic, BLAKE3-audited, built for production.".into()),
        copyright: Some(format!("© {} Valori", chrono::Local::now().year())),
        license: Some("MIT".into()),
        website: Some("https://github.com/valori-ai/valori".into()),
        website_label: Some("github.com/valori-ai/valori".into()),
        credits: None,
        icon: app.default_window_icon().cloned(),
    };
    app_submenu.append_items(&[
        &PredefinedMenuItem::about(app, Some("About Valori"), Some(about_meta))?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::services(app, None)?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::hide(app, None)?,
        &PredefinedMenuItem::hide_others(app, None)?,
        &PredefinedMenuItem::show_all(app, None)?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::quit(app, None)?,
    ])?;

    // ── File ─────────────────────────────────────────────────────────────────
    let file_submenu = Submenu::new(app, "File", true)?;
    let new_project =
        MenuItem::with_id(app, "new-project", "New Project", true, Some("CmdOrCtrl+N"))?;
    file_submenu.append(&new_project)?;

    // ── Edit ─────────────────────────────────────────────────────────────────
    let edit_submenu = Submenu::new(app, "Edit", true)?;
    edit_submenu.append_items(&[
        &PredefinedMenuItem::undo(app, None)?,
        &PredefinedMenuItem::redo(app, None)?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::cut(app, None)?,
        &PredefinedMenuItem::copy(app, None)?,
        &PredefinedMenuItem::paste(app, None)?,
        &PredefinedMenuItem::select_all(app, None)?,
    ])?;

    // ── View ─────────────────────────────────────────────────────────────────
    let view_submenu = Submenu::new(app, "View", true)?;
    let preferences =
        MenuItem::with_id(app, "preferences", "Preferences…", true, Some("CmdOrCtrl+,"))?;
    let reload = MenuItem::with_id(app, "reload", "Reload", true, Some("CmdOrCtrl+R"))?;
    view_submenu.append_items(&[
        &preferences,
        &reload,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::fullscreen(app, None)?,
    ])?;

    // ── Help ─────────────────────────────────────────────────────────────────
    let help_submenu = Submenu::new(app, "Help", true)?;
    let help_item = MenuItem::with_id(app, "help", "Valori Help", true, None::<&str>)?;
    help_submenu.append(&help_item)?;

    Menu::with_items(app, &[&app_submenu, &file_submenu, &edit_submenu, &view_submenu, &help_submenu])
}

/// Build the system tray icon with its context menu.
///
/// Left-click (or single click on Windows/Linux) shows the main window.
/// The menu provides quick access to the main window and a clean quit path.
fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let open_item = MenuItem::with_id(app, "tray-open", "Open Valori", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "tray-quit", "Quit Valori", true, None::<&str>)?;
    let tray_menu = Menu::with_items(app, &[&open_item, &sep, &quit_item])?;

    TrayIconBuilder::new()
        .icon(app.default_window_icon().expect("no default window icon").clone())
        .tooltip("Valori")
        .menu(&tray_menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            // Left-click the tray dot → show/focus the main window.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "tray-open" => show_main(app),
            "tray-quit" => {
                tauri::async_runtime::spawn(shutdown_and_exit(app.clone()));
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "valori_desktop_lib=debug,tauri=debug,wry=debug,tao=debug,warn".into()),
        )
        .init();
    let _ = tracing_log::LogTracer::init();

    let shutting_down = Arc::new(AtomicBool::new(false));
    let shutting_down_setup = shutting_down.clone();
    let shutting_down_run = shutting_down;

    tauri::Builder::default()
        // Must be registered first (Tauri docs). Without this, launching a
        // second instance spawns its own daemon/ui-server sidecars that
        // collide on the same fixed ports as the first instance's — the
        // health probes can't tell "my child" from "another instance's
        // process", so the second instance silently believes it started its
        // own backend when it's actually talking to the first one's. Instead,
        // a second launch just focuses the existing window.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_shell::init())
        // Persist window size and position across launches.
        .plugin(tauri_plugin_window_state::Builder::default().build())
        // Register the valori:// URL scheme so the OS routes deep links here.
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(DaemonState::default())
        .manage(UiServerState::default())
        .invoke_handler(tauri::generate_handler![
            node_health,
            start_daemon,
            stop_daemon,
            daemon_status,
            add_recent_document,
            install_update
        ])
        .setup(move |app| {
            let shutting_down = shutting_down_setup;

            // Build and set the application menu.
            let menu = build_app_menu(app.handle())?;
            app.set_menu(menu)?;

            // Wire app-menu item events.
            let handle_for_menu = app.handle().clone();
            app.on_menu_event(move |_app, event| {
                match event.id().as_ref() {
                    "new-project" => {
                        // Dispatch a custom JS event the React sidebar listens to.
                        if let Some(w) = handle_for_menu.get_webview_window("main") {
                            let _ = w.eval(
                                "window.dispatchEvent(new CustomEvent('valori:new-project'))",
                            );
                        }
                    }
                    "preferences" => nav_to(&handle_for_menu, "/settings"),
                    "reload" => {
                        if let Some(w) = handle_for_menu.get_webview_window("main") {
                            let _ = w.eval("window.location.reload()");
                        }
                    }
                    "help" => nav_to(&handle_for_menu, "/help"),
                    _ => {}
                }
            });

            // Build the system tray.
            build_tray(app.handle())?;

            // Handle valori:// deep links — e.g. valori://projects/my-project
            // opens the app and navigates to that project.
            let handle_for_links = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    let path = match (url.host_str(), url.path().trim_matches('/')) {
                        // valori://projects/my-project
                        (Some("projects"), name) if !name.is_empty() => {
                            format!("/projects/{}", urlencoding::encode(name))
                        }
                        // valori://search?q=... (future)
                        (Some(host), _) => format!("/{host}"),
                        _ => "/".to_string(),
                    };
                    show_main(&handle_for_links);
                    nav_to(&handle_for_links, &path);
                }
            });

            // Release builds: start bundled Next.js server and navigate to it.
            eprintln!("[setup] debug_assertions={}", cfg!(debug_assertions));
            if !cfg!(debug_assertions) {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<UiServerState>();
                    if let Err(e) = ui_server_manager::start_and_navigate(&handle, &state).await {
                        eprintln!("[setup] failed to start bundled ui-server: {e}");
                        tracing::error!("failed to start bundled ui-server: {e}");
                        // The main window is still on the static loading page at
                        // this point (navigate() only runs after a successful
                        // health check) — show the error there instead of
                        // leaving the "Starting services…" spinner frozen.
                        if let Some(w) = handle.get_webview_window("main") {
                            let js = format!(
                                "window.showStartupError && window.showStartupError({})",
                                serde_json::to_string(&e).unwrap_or_default()
                            );
                            let _ = w.eval(&js);
                        }
                    }
                });
            }

            // Background update check — runs 8 s after startup so the UI is
            // visible first. Emits `update-available` to the frontend if a new
            // version exists; the frontend banner calls `install_update` to apply.
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                    match handle.updater() {
                        Ok(updater) => match updater.check().await {
                            Ok(Some(update)) => {
                                let _ = handle.emit(
                                    "update-available",
                                    serde_json::json!({
                                        "version": update.version,
                                        "body": update.body.clone().unwrap_or_default(),
                                    }),
                                );
                            }
                            Ok(None) => {}
                            Err(e) => tracing::debug!("update check: {e}"),
                        },
                        Err(e) => tracing::debug!("updater init: {e}"),
                    }
                });
            }

            // SIGTERM handler (macOS/Linux) — graceful shutdown bypassing Tauri's
            // window-close path which doesn't fire on external kills.
            #[cfg(unix)]
            {
                let handle = app.handle().clone();
                let shutting_down = shutting_down.clone();
                tauri::async_runtime::spawn(async move {
                    let mut sigterm = match tokio::signal::unix::signal(
                        tokio::signal::unix::SignalKind::terminate(),
                    ) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::error!("failed to install SIGTERM handler: {e}");
                            return;
                        }
                    };
                    sigterm.recv().await;
                    if !shutting_down.swap(true, Ordering::SeqCst) {
                        shutdown_and_exit(handle).await;
                    }
                });
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the Valori desktop application")
        .run(move |app_handle, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if shutting_down_run.swap(true, Ordering::SeqCst) {
                    return;
                }
                api.prevent_exit();
                let handle = app_handle.clone();
                tauri::async_runtime::spawn(shutdown_and_exit(handle));
            }
        });
}
