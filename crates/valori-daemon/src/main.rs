// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `valori-daemon` binary — serves the project-lifecycle HTTP API.
//!
//! Env:
//!   VALORI_HOME        data root (default ~/.valori)
//!   VALORI_DAEMON_BIND listen address (default 127.0.0.1:8080)
//!   VALORI_NODE_BIN    path to the valori-node binary to supervise
//!   VALORI_REPO_ROOT   where to find target/{release,debug}/valori-node

use std::sync::Arc;
use tokio::sync::Mutex;

use valori_daemon::{default_home, router, Daemon};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "valori_daemon=info,warn".into()),
        )
        .init();

    let home = default_home();
    let daemon = match Daemon::new(&home) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("failed to initialize daemon at {}: {e}", home.display());
            std::process::exit(1);
        }
    };
    let shared = Arc::new(Mutex::new(daemon));

    let bind = std::env::var("VALORI_DAEMON_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let listener = match tokio::net::TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("failed to bind {bind}: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!(
        "valori-daemon listening on {bind}  (home: {})",
        home.display()
    );

    // Supervision monitor: every 2s, detect crashed nodes and restart per policy.
    let monitor = shared.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            tick.tick().await;
            let n = monitor.lock().await.supervise_tick().await;
            if n > 0 {
                tracing::info!("supervisor restarted {n} node(s)");
            }
        }
    });

    // Terminate supervised nodes on Ctrl-C so we don't leak child processes.
    let shutdown = shared.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("shutting down — stopping supervised nodes");
        shutdown.lock().await.shutdown().await;
        std::process::exit(0);
    });

    if let Err(e) = axum::serve(listener, router(shared)).await {
        eprintln!("server error: {e}");
        std::process::exit(1);
    }
}
