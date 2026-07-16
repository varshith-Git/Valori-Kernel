// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! HTTP API for the daemon — Milestone 1 project lifecycle.
//!
//! `GET  /health`
//! `GET  /v1/projects`                 list projects + node status
//! `POST /v1/projects`                 create  {name, dim, index?}
//! `GET  /v1/projects/:name`           detail (config + status)
//! `DELETE /v1/projects/:name`         delete (must be stopped)
//! `POST /v1/projects/:name/start`
//! `POST /v1/projects/:name/stop`
//! `POST /v1/projects/:name/restart`

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::daemon::Daemon;
use crate::error::DaemonResult;
use crate::project::{ClusterConfig, EmbeddingConfig, Project, ProjectManifest, StorageConfig};
use crate::runtime::NodeInfo;

pub type SharedDaemon = Arc<Mutex<Daemon>>;

pub fn router(daemon: SharedDaemon) -> Router {
    Router::new()
        // ── system / discovery ──────────────────────────────────────────────
        .route("/health", get(health))
        .route("/version", get(version))
        .route("/v1/system", get(system))
        .route("/v1/config", get(config))
        .route("/v1/events", get(events))
        .route("/v1/shutdown", post(shutdown))
        // ── workspaces ──────────────────────────────────────────────────────
        .route("/v1/workspaces", get(list_workspaces).post(create_workspace))
        .route("/v1/workspaces/:name", patch(rename_workspace).delete(delete_workspace))
        // ── projects ────────────────────────────────────────────────────────
        .route("/v1/projects", get(list_projects).post(create_project))
        .route("/v1/projects/:name", get(project_detail).patch(rename_project).delete(delete_project))
        .route("/v1/projects/:name/start", post(start_project))
        .route("/v1/projects/:name/stop", post(stop_project))
        .route("/v1/projects/:name/restart", post(restart_project))
        .route("/v1/projects/:name/logs", get(project_logs))
        .route("/v1/projects/:name/runtime", get(project_runtime))
        // ── collections (proxied to the running node) ───────────────────────
        .route("/v1/projects/:name/collections", get(list_collections).post(create_collection))
        .route("/v1/projects/:name/collections/:collection", axum::routing::delete(delete_collection))
        // ── models (E1-lite) ────────────────────────────────────────────────
        .route("/v1/models", get(list_models))
        .route("/v1/models/install", post(install_model))
        // catch-all: model ids contain slashes (e.g. "openai/text-embedding-3-small")
        .route("/v1/models/*id", get(model_detail).delete(delete_model))
        .with_state(daemon)
}

fn project_json(
    project: &Project,
    status: &NodeInfo,
    supervision: Option<&crate::supervisor::SupervisionInfo>,
) -> Value {
    let mut v = json!({
        "id": project.config.id,
        "name": project.config.name,
        "dim": project.config.dim,
        "index": project.config.index,
        "workspace": project.config.workspace,
        "restart_policy": project.config.restart_policy,
        "created_at": project.config.created_at,
        "last_opened_at": project.config.last_opened_at,
        "cluster": project.config.cluster,
        "embedding": project.config.embedding,
        "storage": project.config.storage,
        "status": status,
    });
    if let Some(sup) = supervision {
        v["supervision"] = serde_json::to_value(sup).unwrap_or(Value::Null);
    }
    v
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "service": "valori-daemon", "version": env!("CARGO_PKG_VERSION") }))
}

async fn version() -> Json<Value> {
    Json(json!({ "version": env!("CARGO_PKG_VERSION"), "api": "v1" }))
}

async fn system(State(d): State<SharedDaemon>) -> Json<Value> {
    Json(d.lock().await.system())
}

async fn config(State(d): State<SharedDaemon>) -> Json<Value> {
    Json(d.lock().await.config())
}

/// Graceful daemon shutdown, over HTTP — the cross-platform-safe way for a
/// supervisor (the desktop app) to ask the daemon to stop. OS signals
/// (SIGTERM) work for a CLI-launched daemon on Unix, but a process spawned
/// and supervised by the desktop shell can't rely on signal semantics being
/// uniform across macOS/Linux/Windows; an HTTP call is. Stops every running
/// project's node gracefully (snapshot-then-terminate, same as `stop_all()`
/// on Ctrl-C) before the daemon process itself exits.
async fn shutdown(State(d): State<SharedDaemon>) -> Json<Value> {
    tracing::info!("shutdown requested via HTTP — stopping supervised nodes");
    d.lock().await.shutdown().await;
    let resp = json!({ "ok": true });
    // Exit after the response has a chance to flush, not before — a caller
    // blocked on this request should see 200 rather than a connection reset.
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        std::process::exit(0);
    });
    Json(resp)
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default = "default_events_limit")]
    limit: usize,
}

fn default_events_limit() -> usize {
    100
}

async fn events(State(d): State<SharedDaemon>, Query(q): Query<EventsQuery>) -> Json<Value> {
    Json(json!({ "events": d.lock().await.events(q.limit) }))
}

async fn project_runtime(
    State(d): State<SharedDaemon>,
    Path(name): Path<String>,
) -> DaemonResult<Json<Value>> {
    let stats = d.lock().await.project_resources(&name)?;
    Ok(Json(serde_json::to_value(stats).unwrap_or_else(|_| json!({}))))
}

// ── Workspaces ──────────────────────────────────────────────────────────────

async fn list_workspaces(State(d): State<SharedDaemon>) -> Json<Value> {
    Json(json!({ "workspaces": d.lock().await.list_workspaces() }))
}

#[derive(Deserialize)]
struct CreateWorkspaceRequest {
    name: String,
}

async fn create_workspace(
    State(d): State<SharedDaemon>,
    Json(req): Json<CreateWorkspaceRequest>,
) -> DaemonResult<Json<Value>> {
    let ws = d.lock().await.create_workspace(&req.name)?;
    Ok(Json(json!({ "name": ws.name, "created_at": ws.created_at, "projects": 0 })))
}

#[derive(Deserialize)]
struct RenameWorkspaceRequest {
    name: String,
}

async fn rename_workspace(
    State(d): State<SharedDaemon>,
    Path(name): Path<String>,
    Json(req): Json<RenameWorkspaceRequest>,
) -> DaemonResult<Json<Value>> {
    let ws = d.lock().await.rename_workspace(&name, &req.name)?;
    Ok(Json(json!({ "name": ws.name, "created_at": ws.created_at })))
}

async fn delete_workspace(
    State(d): State<SharedDaemon>,
    Path(name): Path<String>,
) -> DaemonResult<Json<Value>> {
    d.lock().await.delete_workspace(&name)?;
    Ok(Json(json!({ "deleted": name })))
}

// ── Collections (proxied) ─────────────────────────────────────────────────────

async fn list_collections(
    State(d): State<SharedDaemon>,
    Path(name): Path<String>,
) -> DaemonResult<Json<Value>> {
    Ok(Json(d.lock().await.list_collections(&name).await?))
}

#[derive(Deserialize)]
struct CreateCollectionRequest {
    name: String,
}

async fn create_collection(
    State(d): State<SharedDaemon>,
    Path(name): Path<String>,
    Json(req): Json<CreateCollectionRequest>,
) -> DaemonResult<Json<Value>> {
    Ok(Json(d.lock().await.create_collection(&name, &req.name).await?))
}

async fn delete_collection(
    State(d): State<SharedDaemon>,
    Path((name, collection)): Path<(String, String)>,
) -> DaemonResult<Json<Value>> {
    Ok(Json(d.lock().await.delete_collection(&name, &collection).await?))
}

// ── Models (stubs) ────────────────────────────────────────────────────────────

async fn list_models(State(d): State<SharedDaemon>) -> Json<Value> {
    Json(d.lock().await.models_catalog())
}

async fn model_detail(
    State(d): State<SharedDaemon>,
    Path(id): Path<String>,
) -> DaemonResult<Json<Value>> {
    let model = d.lock().await.model_detail(&id)?;
    Ok(Json(serde_json::to_value(model).unwrap_or(Value::Null)))
}

#[derive(Deserialize)]
struct InstallModelRequest {
    id: String,
}

async fn install_model(
    State(d): State<SharedDaemon>,
    Json(req): Json<InstallModelRequest>,
) -> DaemonResult<Json<Value>> {
    let model = d.lock().await.install_model(&req.id).await?;
    Ok(Json(serde_json::to_value(model).unwrap_or(Value::Null)))
}

async fn delete_model(
    State(d): State<SharedDaemon>,
    Path(id): Path<String>,
) -> DaemonResult<Json<Value>> {
    d.lock().await.remove_model(&id)?;
    Ok(Json(json!({ "removed": id })))
}

// ── Logs ──────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct LogsQuery {
    #[serde(default = "default_tail")]
    tail: usize,
}

fn default_tail() -> usize {
    200
}

async fn project_logs(
    State(d): State<SharedDaemon>,
    Path(name): Path<String>,
    Query(q): Query<LogsQuery>,
) -> DaemonResult<Json<Value>> {
    let logs = d.lock().await.project_logs(&name, q.tail)?;
    Ok(Json(json!({ "project": name, "logs": logs })))
}

async fn list_projects(State(d): State<SharedDaemon>) -> DaemonResult<Json<Value>> {
    let daemon = d.lock().await;
    let projects: Vec<Value> = daemon
        .list_projects()?
        .iter()
        .map(|(p, s, sup)| project_json(p, s, sup.as_ref()))
        .collect();
    Ok(Json(json!({ "projects": projects })))
}

#[derive(Deserialize)]
struct CreateProjectRequest {
    name: String,
    dim: usize,
    #[serde(default = "default_index")]
    index: String,
    #[serde(default = "default_workspace")]
    workspace: String,
    #[serde(default)]
    restart_policy: crate::policy::RestartPolicy,
    /// Persisted only — see [`ClusterConfig`]. Omit for a single-node project.
    #[serde(default)]
    cluster: Option<ClusterConfig>,
    /// Persisted only — see [`EmbeddingConfig`].
    #[serde(default)]
    embedding: EmbeddingConfig,
    /// Persisted only — see [`StorageConfig`].
    #[serde(default)]
    storage: StorageConfig,
}

fn default_index() -> String {
    "brute".to_string()
}

fn default_workspace() -> String {
    crate::workspace::DEFAULT_WORKSPACE.to_string()
}

async fn create_project(
    State(d): State<SharedDaemon>,
    Json(req): Json<CreateProjectRequest>,
) -> DaemonResult<Json<Value>> {
    let config = ProjectManifest {
        id: crate::new_id(),
        name: req.name,
        dim: req.dim,
        index: req.index,
        workspace: req.workspace,
        restart_policy: req.restart_policy,
        created_at: now_unix(),
        last_opened_at: None,
        cluster: req.cluster,
        embedding: req.embedding,
        storage: req.storage,
    };
    let daemon = d.lock().await;
    let project = daemon.create_project(config)?;
    let status = NodeInfo::stopped(&project.config.name);
    Ok(Json(project_json(&project, &status, None)))
}

async fn project_detail(
    State(d): State<SharedDaemon>,
    Path(name): Path<String>,
) -> DaemonResult<Json<Value>> {
    let daemon = d.lock().await;
    let (project, status, sup) = daemon.project_detail(&name)?;
    Ok(Json(project_json(&project, &status, sup.as_ref())))
}

#[derive(Deserialize)]
struct RenameProjectRequest {
    name: String,
}

async fn rename_project(
    State(d): State<SharedDaemon>,
    Path(old_name): Path<String>,
    Json(req): Json<RenameProjectRequest>,
) -> DaemonResult<Json<Value>> {
    let daemon = d.lock().await;
    let project = daemon.rename_project(&old_name, &req.name)?;
    Ok(Json(json!({ "project": project_json(&project, &crate::runtime::NodeInfo::stopped(&req.name), None) })))
}

async fn delete_project(
    State(d): State<SharedDaemon>,
    Path(name): Path<String>,
) -> DaemonResult<Json<Value>> {
    let daemon = d.lock().await;
    daemon.delete_project(&name)?;
    Ok(Json(json!({ "deleted": name })))
}

async fn start_project(
    State(d): State<SharedDaemon>,
    Path(name): Path<String>,
) -> DaemonResult<Json<NodeInfo>> {
    let mut daemon = d.lock().await;
    Ok(Json(daemon.start_project(&name).await?))
}

async fn stop_project(
    State(d): State<SharedDaemon>,
    Path(name): Path<String>,
) -> DaemonResult<Json<NodeInfo>> {
    let mut daemon = d.lock().await;
    Ok(Json(daemon.stop_project(&name).await?))
}

async fn restart_project(
    State(d): State<SharedDaemon>,
    Path(name): Path<String>,
) -> DaemonResult<Json<NodeInfo>> {
    let mut daemon = d.lock().await;
    Ok(Json(daemon.restart_project(&name).await?))
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
