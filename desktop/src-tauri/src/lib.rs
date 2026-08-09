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

mod credential_service;
mod daemon_manager;
mod filesystem_service;
mod preferences_service;
mod project_registry_service;
mod session_service;
mod studio_storage;
use studio_storage::get_studio_recovery_status;
mod telemetry;
mod ui_server_manager;
use credential_service::{credential_delete, credential_exists, credential_get, credential_store};
use daemon_manager::{daemon_status, start_daemon, stop_daemon, stop_daemon_internal, DaemonState};
use preferences_service::{
    get_all_preferences, get_installation_id_command, get_preference,
    get_telemetry_consent_command, set_preference, set_telemetry_consent_command,
    StudioPreferencesService,
};
use project_registry_service::{
    registry_favorite_projects, registry_get_project, registry_list_projects,
    registry_recent_projects, registry_reconcile_legacy_names, registry_register_cloud_project,
    registry_register_local_project, registry_rename_project, registry_set_favorite,
    registry_set_local_path, registry_touch_last_opened, registry_unregister_project,
};
use session_service::{
    session_end_current, session_get_current, session_list_recent, SessionService,
};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;
use telemetry::{
    check_and_clear_crash_marker, enqueue_telemetry_event, get_app_info, get_rust_start_ms,
    get_session_id,
};
use ui_server_manager::UiServerState;
use valori_domain::SessionId;
use valori_studio_storage::StudioDatabase;

/// Runs the shared shutdown sequence exactly once, then exits.
async fn shutdown_and_exit(app: tauri::AppHandle) {
    // Record clean session end in studio.redb (S2b-2c).
    if let Some(db) = app.try_state::<Arc<StudioDatabase>>() {
        if let Ok(session_id) = get_session_id().parse::<SessionId>() {
            let session_service = SessionService::new(db.inner().clone());
            let now = chrono::Utc::now().timestamp_millis();
            let _ = session_service.end_session(session_id, now, false);
        }
    }
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
            Ok(HealthReport {
                url: base_url,
                reachable,
                body,
            })
        }
        Err(e) => Ok(HealthReport {
            url: base_url,
            reachable: false,
            body: e.to_string(),
        }),
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

    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let available_version = update.version.clone();
    let props = || serde_json::json!({ "current_version": current_version, "available_version": available_version });

    telemetry::enqueue_update_event(&app, "update_download_started", props());

    // The plugin's download_and_install bundles download + install into one
    // step with no separate observable boundary between them — the second
    // closure fires once the download itself finishes, which is the real
    // signal we have; install_started is approximated from that same point
    // rather than invented from nothing.
    let app_for_cb = app.clone();
    let props_for_cb = props();
    let start = std::time::Instant::now();
    let result = update
        .download_and_install(
            |_downloaded, _total| {},
            move || {
                telemetry::enqueue_update_event(
                    &app_for_cb,
                    "update_download_completed",
                    props_for_cb.clone(),
                );
                telemetry::enqueue_update_event(
                    &app_for_cb,
                    "update_install_started",
                    props_for_cb.clone(),
                );
            },
        )
        .await;

    match &result {
        Ok(()) => {
            let mut p = props();
            p["install_time_ms"] = serde_json::json!(start.elapsed().as_millis() as u64);
            telemetry::enqueue_update_event(&app, "update_install_success", p);
        }
        Err(e) => {
            let mut p = props();
            p["error"] = serde_json::json!(e.to_string());
            telemetry::enqueue_update_event(&app, "update_install_failed", p);
        }
    }

    result.map_err(|e| e.to_string())?;
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

/// Same as `nav_to`, but JSON-escapes `path` before interpolating it into
/// the JS string literal — required here (unlike `nav_to`'s callers above,
/// which only ever pass hardcoded paths or a urlencoded project name) since
/// this carries a Supabase access/refresh token straight from a deep link.
fn nav_to_safe(app: &tauri::AppHandle, path: &str) {
    if let Some(w) = app.get_webview_window("main") {
        let js = format!(
            "window.location.href={}",
            serde_json::to_string(path).unwrap_or_default()
        );
        let _ = w.eval(&js);
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Opens Valori Cloud's login page in the user's default system browser —
/// never inside the embedded webview, which is capability-locked to
/// 127.0.0.1 only (see capabilities/default.json). `?desktop=1` tells the
/// website to hand the session back via a valori://auth-callback deep link
/// (see its /desktop-handoff page) instead of redirecting into its own
/// dashboard.
#[tauri::command]
fn open_cloud_login(app: tauri::AppHandle, provider: Option<String>) -> Result<(), String> {
    let mut url = "https://valori.systems/login?desktop=1".to_string();
    if let Some(p) = provider {
        url.push_str(&format!("&provider={p}"));
    }
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| e.to_string())
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
    let preferences = MenuItem::with_id(
        app,
        "preferences",
        "Preferences…",
        true,
        Some("CmdOrCtrl+,"),
    )?;
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

    Menu::with_items(
        app,
        &[
            &app_submenu,
            &file_submenu,
            &edit_submenu,
            &view_submenu,
            &help_submenu,
        ],
    )
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
        .icon(
            app.default_window_icon()
                .expect("no default window icon")
                .clone(),
        )
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
    // Absolute first line — the earliest possible mark for the startup
    // waterfall (`startup_completed` event). Everything else, even logging
    // init, happens after this.
    telemetry::init_rust_start_ms();

    // S7 (`docs/phases/phase-studio-S7-persistence-boundary.md`): logs now
    // go to stdout/stderr (unchanged — every prior phase's "confirmed no
    // file sink exists" finding stays true for that half) **and** to a
    // real, bounded, rotating file under the canonical
    // `$VALORI_HOME/logs/studio.log` — `logs_dir()` finally has a writer.
    // Non-fatal: if the log directory can't be created (permissions,
    // read-only filesystem), the app still runs with the stdout layer
    // alone — a working app with console-only logs beats a failed launch.
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let env_filter = || {
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            "valori_desktop_lib=debug,tauri=debug,wry=debug,tao=debug,warn".into()
        })
    };

    let logs_dir = valori_studio_storage::StudioPaths::from_env().logs_dir();
    let file_layer_guard = match filesystem_service::FileSystemService::new().create_dir(&logs_dir)
    {
        Ok(()) => {
            let file_appender = tracing_appender::rolling::daily(&logs_dir, "studio.log");
            let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
            let file_layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(non_blocking);
            tracing_subscriber::registry()
                .with(env_filter())
                .with(tracing_subscriber::fmt::layer())
                .with(file_layer)
                .init();
            Some(guard)
        }
        Err(_) => {
            // logs_dir couldn't be created — fall back to stdout only,
            // exactly the pre-S7 behavior.
            tracing_subscriber::registry()
                .with(env_filter())
                .with(tracing_subscriber::fmt::layer())
                .init();
            None
        }
    };
    // Leaked deliberately: `non_blocking`'s worker thread must outlive this
    // function (the whole app), and `run()` never returns until exit —
    // there is no later point to drop this guard at.
    std::mem::forget(file_layer_guard);

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
            install_update,
            open_cloud_login,
            get_app_info,
            get_session_id,
            get_rust_start_ms,
            enqueue_telemetry_event,
            check_and_clear_crash_marker,
            get_preference,
            set_preference,
            get_all_preferences,
            get_installation_id_command,
            get_telemetry_consent_command,
            set_telemetry_consent_command,
            registry_list_projects,
            registry_get_project,
            registry_recent_projects,
            registry_favorite_projects,
            registry_register_local_project,
            registry_register_cloud_project,
            registry_rename_project,
            registry_set_local_path,
            registry_set_favorite,
            registry_touch_last_opened,
            registry_unregister_project,
            registry_reconcile_legacy_names,
            session_get_current,
            session_list_recent,
            session_end_current,
            get_studio_recovery_status,
            credential_store,
            credential_get,
            credential_exists,
            credential_delete
        ])
        .setup(move |app| {
            let shutting_down = shutting_down_setup;

            // As early as possible — a panic hook installed later could
            // miss a panic during the rest of setup(). Only ever writes a
            // local crash marker (no network call is safe mid-panic); the
            // actual report is sent on the *next* startup by the frontend,
            // after checking consent. See telemetry.rs's module doc.
            telemetry::install_panic_hook(app.handle().clone());

            // Session id, generated once, right after the panic hook — before
            // anything else in setup() (the background update check below,
            // in particular) can emit an event. Every event from this process,
            // native or JS-originated, carries this same value. See
            // telemetry.rs's module doc.
            let session_id_str = telemetry::init_session_id();

            // Studio storage (studio.redb) initialization & legacy data migration (S2b-1).
            // Opens studio.redb, imports legacy preferences.json and events.jsonl if needed,
            // and manages Arc<StudioDatabase> in application state.
            //
            // Deliberately non-fatal: `studio.redb` is Studio's own local
            // convenience state, not core app functionality. A corrupt or
            // unopenable database must not prevent the app from launching —
            // see `studio_storage::init_studio_storage`'s doc comment. Every
            // consumer already degrades gracefully when this state isn't
            // managed (`app.try_state::<Arc<StudioDatabase>>()` is `None`).
            if let Some(studio_db) = studio_storage::init_studio_storage(app.handle()) {
                app.manage(studio_db.clone());
                // StudioPreferencesService is managed so that telemetry's
                // analytics_consent() can reach it without bypassing the service
                // layer (S2b-2d.1 consent boundary cleanup).
                app.manage(StudioPreferencesService::new(studio_db.clone()));

                // Installation identity (Studio Installation Identity phase):
                // get-or-init unconditionally, independent of telemetry
                // consent, Cloud login, or project state. This is the one
                // place per app startup that guarantees `installation_id`
                // exists before anything else (sessions, telemetry) reads
                // it. Reuses the service instance just managed above — see
                // `preferences_service.rs::get_or_init_installation_id` for
                // the canonical implementation.
                let installation_id = app
                    .state::<StudioPreferencesService>()
                    .get_or_init_installation_id()
                    .ok();
                if installation_id.is_none() {
                    tracing::warn!("failed to get or init installation_id at startup");
                }

                // Session lifecycle (S2b-2c) + retention (S5), in the order
                // the S5 phase doc requires: DB open → installation identity
                // (both above) → crash reconciliation → prune old history →
                // start the current session. Reconciliation and pruning only
                // need `session_id` (not yet a row in `sessions`), so running
                // them before `start_session` is safe — see
                // `valori-studio-storage`'s `SessionStore::prune` doc comment
                // ("current_session_id is never touched, regardless of its
                // state") for why the ordering relative to `start_session`
                // doesn't matter for correctness, only for matching this
                // documented lifecycle exactly.
                if let Ok(session_id) = session_id_str.parse::<SessionId>() {
                    let session_service = SessionService::new(studio_db.clone());
                    let app_info = telemetry::get_app_info();
                    let now = chrono::Utc::now().timestamp_millis();

                    let _ = session_service.reconcile_crashed_sessions(session_id, now);

                    // Session retention (S5): never fatal to startup — a
                    // pruning failure must never trigger `studio.redb`
                    // recovery or block launch, only get logged. See
                    // `docs/phases/phase-studio-S5-session-retention.md`.
                    match session_service.prune_sessions(
                        session_id,
                        &valori_studio_storage::session::SessionRetentionPolicy::default(),
                        now,
                    ) {
                        Ok(stats) => {
                            if stats.deleted > 0 {
                                tracing::info!(
                                    scanned = stats.scanned,
                                    deleted = stats.deleted,
                                    retained = stats.retained,
                                    "session retention: pruned old session history"
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!("session retention pruning failed (non-fatal): {e}");
                        }
                    }

                    let _ = session_service.start_session(
                        session_id,
                        installation_id,
                        &app_info.version,
                        &app_info.platform,
                        now,
                    );
                }

                // Stale temp-file cleanup (S6 — Desktop Filesystem
                // Consolidation): removes Studio-owned files under
                // `$VALORI_HOME/temp/` older than 24h. A no-op if `temp/`
                // doesn't exist yet — never created merely to be cleaned.
                // Never fatal to startup, matching the session-retention
                // pruning above; see
                // `docs/phases/phase-studio-S6-filesystem-management.md`.
                let temp_dir = valori_studio_storage::StudioPaths::from_env().temp_dir();
                match filesystem_service::FileSystemService::new()
                    .cleanup_stale_temp_files(&temp_dir, std::time::Duration::from_secs(24 * 3600))
                {
                    Ok(removed) if removed > 0 => {
                        tracing::info!(removed, "startup: cleaned up stale temp files");
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("stale temp-file cleanup failed (non-fatal): {e}");
                    }
                }

                // Old log-file cleanup (S7 — bounded retention for
                // `logs_dir()`'s new tracing-appender file sink, daily
                // rotation): removes rotated log files older than 7 days.
                // Same non-fatal, no-op-if-absent contract as the temp
                // cleanup above.
                let logs_dir = valori_studio_storage::StudioPaths::from_env().logs_dir();
                match filesystem_service::FileSystemService::new()
                    .cleanup_old_logs(&logs_dir, std::time::Duration::from_secs(7 * 24 * 3600))
                {
                    Ok(removed) if removed > 0 => {
                        tracing::info!(removed, "startup: cleaned up old log files");
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("old log-file cleanup failed (non-fatal): {e}");
                    }
                }

                // Old crash-archive cleanup (S7 — bounded retention for
                // `crashes_dir()`'s new archival copies; see
                // `telemetry.rs::archive_crash`). 30-day window — crash
                // history is comparatively rare and higher-signal, kept
                // longer than temp/log files, matching S5's precedent for
                // crashed sessions.
                let crashes_dir = valori_studio_storage::StudioPaths::from_env().crashes_dir();
                match filesystem_service::FileSystemService::new()
                    .cleanup_old_crash_archives(&crashes_dir, std::time::Duration::from_secs(30 * 24 * 3600))
                {
                    Ok(removed) if removed > 0 => {
                        tracing::info!(removed, "startup: cleaned up old crash archives");
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("old crash-archive cleanup failed (non-fatal): {e}");
                    }
                }

                // Background sender for the durable telemetry queue (S2b-2d) —
                // drains studio.redb's `telemetry_queue` immediately (flushing
                // anything left over from a previous offline session), then every
                // 60 s. events.jsonl is now a read-only legacy artifact.
                telemetry::spawn_sender(app.handle().clone(), std::time::Duration::from_secs(60));
            } else {
                tracing::warn!(
                    "Studio storage unavailable — preferences, session history, and \
                     telemetry queueing are disabled for this run"
                );
            }

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
                    // valori://auth-callback?access_token=...&refresh_token=...
                    // — the "sign in to sync" handoff (open_cloud_login above
                    // + the website's /desktop-handoff page). Tokens go
                    // straight through to the embedded webview's own
                    // /auth/desktop-received page, which hands them to
                    // Supabase and flips this app into cloud mode — they're
                    // never read or stored on the Rust side.
                    if url.host_str() == Some("auth-callback") {
                        let query = url.query().unwrap_or("");
                        show_main(&handle_for_links);
                        nav_to_safe(&handle_for_links, &format!("/auth/desktop-received?{query}"));
                        continue;
                    }

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
                    let current_version = env!("CARGO_PKG_VERSION").to_string();
                    telemetry::enqueue_update_event(
                        &handle,
                        "update_check_started",
                        serde_json::json!({ "current_version": current_version }),
                    );
                    match handle.updater() {
                        Ok(updater) => match updater.check().await {
                            Ok(Some(update)) => {
                                telemetry::enqueue_update_event(
                                    &handle,
                                    "update_available",
                                    serde_json::json!({ "current_version": current_version, "available_version": update.version }),
                                );
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
