//! Free-tier hosting: one process, independent project engines and durable logs.
//! Management requires the operator token; data requests require the selected
//! project's token. No customer-supplied collection name determines a tenant.
use axum::{
    extract::{Path, Request, State},
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{any, get, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path as FsPath, PathBuf},
    sync::Arc,
};
use subtle::ConstantTimeEq;
use tokio::sync::RwLock;
use tower::ServiceExt;
use uuid::Uuid;

use crate::{
    crypto_vault::AesGcmVault,
    engine::{Engine, EngineConfig, Persistence, QuantizationKind},
    server::{build_router, SharedEngine},
};

type ApiResult<T> = Result<T, (StatusCode, Json<serde_json::Value>)>;
fn error(status: StatusCode, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({"error": message})))
}
fn internal(e: impl std::fmt::Display) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!(error = %e, "shared project operation failed");
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "shared project storage operation failed",
    )
}
fn authorized(headers: &HeaderMap, expected: &[u8; 32]) -> bool {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|token| bool::from(blake3::hash(token.as_bytes()).as_bytes().ct_eq(expected)))
}

#[derive(Deserialize)]
pub struct CreateProject {
    pub worker_auth_token: String,
    pub max_records: usize,
}

#[derive(Clone, Serialize, Deserialize)]
struct Manifest {
    token_hash: [u8; 32],
    max_records: usize,
    active: bool,
}
struct Project {
    manifest: Manifest,
    engine: SharedEngine,
    router: Router,
}

pub struct SharedHost {
    root: PathBuf,
    admin_hash: [u8; 32],
    max_projects: usize,
    projects: RwLock<BTreeMap<Uuid, Project>>,
}

impl SharedHost {
    /// The data directory must be dedicated to this process. Existing projects
    /// are validated before serving; corrupt logs never silently start empty.
    pub fn open(
        root: &FsPath,
        admin_token: &str,
        max_projects: usize,
    ) -> Result<Arc<Self>, String> {
        if admin_token.len() < 32 || max_projects == 0 {
            return Err("shared admin token must contain at least 32 characters and capacity must be positive".into());
        }
        std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let mut projects = BTreeMap::new();
        for entry in std::fs::read_dir(&root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let Ok(id) = Uuid::parse_str(&entry.file_name().to_string_lossy()) else {
                continue;
            };
            let path = root.join(id.to_string());
            if path.canonicalize().map_err(|e| e.to_string())?.parent() != Some(root.as_path()) {
                return Err("project directory escapes shared root".into());
            }
            // A directory without a manifest is an interrupted, unregistered
            // creation; a retry can register it using the same project UUID.
            if !path.join("project.json").exists() {
                continue;
            }
            let manifest: Manifest = serde_json::from_slice(
                &std::fs::read(path.join("project.json")).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let project = open_project(&path, manifest)?;
            projects.insert(id, project);
        }
        Ok(Arc::new(Self {
            root,
            admin_hash: *blake3::hash(admin_token.as_bytes()).as_bytes(),
            max_projects,
            projects: RwLock::new(projects),
        }))
    }

    pub fn router(self: &Arc<Self>) -> Router {
        Router::new()
            .route("/health", get(|| async { "ok" }))
            .route(
                "/shared/projects/:id",
                put(create_project).delete(delete_project),
            )
            .route("/shared/projects/:id/active", put(set_active))
            .route("/shared/projects/:id/export", get(export_project))
            .route("/shared/projects/:id/node/*path", any(dispatch))
            .with_state(self.clone())
    }

    fn check_admin(&self, headers: &HeaderMap) -> ApiResult<()> {
        if authorized(headers, &self.admin_hash) {
            Ok(())
        } else {
            Err(error(
                StatusCode::UNAUTHORIZED,
                "invalid management credentials",
            ))
        }
    }
}

fn persist_manifest(path: &FsPath, manifest: &Manifest) -> Result<(), String> {
    use std::io::Write;
    let temporary = path.join("project.json.tmp");
    let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(manifest).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    std::fs::rename(temporary, path.join("project.json")).map_err(|e| e.to_string())
}

fn open_project(path: &FsPath, manifest: Manifest) -> Result<Project, String> {
    let log_path = path.join("events.log");
    if log_path.exists() {
        valori_state::bootstrap::recover_from_events(&log_path).map_err(|e| e.to_string())?;
    }
    let mut engine = Engine::with_config(EngineConfig {
        max_records: manifest.max_records,
        max_nodes: manifest.max_records,
        max_edges: manifest.max_records.saturating_mul(5),
        quantization_kind: QuantizationKind::None,
        hnsw_m: None,
        hnsw_ef_construction: None,
        hnsw_ef_search: None,
        ivf_n_list: None,
        ivf_n_probe: None,
        bq_pool_factor: None,
        bq_min_candidates: None,
        snapshot_path: Some(path.join("snapshot.bin")),
        wal_path: None,
        event_log_path: Some(log_path),
        event_log_rotation_bytes: Some(0),
        decay_half_life_secs: None,
        shard_count: 1,
        object_store_keep: 3,
        object_store: None,
        vault: Arc::new(
            AesGcmVault::with_shred_log(&path.join("shred.log")).map_err(|e| e.to_string())?,
        ),
        embed_config: None,
    });
    engine.try_recover();
    let persistence = std::mem::replace(&mut engine.persistence, Persistence::Ephemeral);
    engine.persistence = match persistence {
        Persistence::EventLog(c) => {
            Persistence::EventLog(c.with_flush_every(1).with_rotation_bytes(None))
        }
        _ => return Err("shared projects require a writable event log".into()),
    };
    let engine = Arc::new(RwLock::new(engine));
    let router = build_router(engine.clone(), None, None);
    Ok(Project {
        manifest,
        engine,
        router,
    })
}

async fn create_project(
    State(host): State<Arc<SharedHost>>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(req): Json<CreateProject>,
) -> ApiResult<StatusCode> {
    host.check_admin(&headers)?;
    if req.worker_auth_token.len() < 32 || req.max_records == 0 || req.max_records > 1_000_000 {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "invalid project credentials or record capacity",
        ));
    }
    let token_hash = *blake3::hash(req.worker_auth_token.as_bytes()).as_bytes();
    let mut projects = host.projects.write().await;
    if let Some(project) = projects.get(&id) {
        if project.manifest.token_hash != token_hash
            || project.manifest.max_records != req.max_records
        {
            return Err(error(
                StatusCode::CONFLICT,
                "project already exists with different configuration",
            ));
        }
        return Ok(StatusCode::OK);
    }
    if projects.len() >= host.max_projects {
        return Err(error(
            StatusCode::CONFLICT,
            "shared worker project capacity reached",
        ));
    }
    let path = host.root.join(id.to_string());
    std::fs::create_dir_all(&path).map_err(internal)?;
    if path.canonicalize().map_err(internal)?.parent() != Some(host.root.as_path()) {
        return Err(error(StatusCode::BAD_REQUEST, "invalid project directory"));
    }
    let manifest = Manifest {
        token_hash,
        max_records: req.max_records,
        active: true,
    };
    let project = open_project(&path, manifest.clone()).map_err(internal)?;
    persist_manifest(&path, &manifest).map_err(internal)?;
    projects.insert(id, project);
    Ok(StatusCode::CREATED)
}

#[derive(Deserialize)]
struct Active {
    active: bool,
}
async fn set_active(
    State(host): State<Arc<SharedHost>>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(req): Json<Active>,
) -> ApiResult<StatusCode> {
    host.check_admin(&headers)?;
    let mut projects = host.projects.write().await;
    let project = projects
        .get_mut(&id)
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "project not found"))?;
    let mut manifest = project.manifest.clone();
    manifest.active = req.active;
    persist_manifest(&host.root.join(id.to_string()), &manifest).map_err(internal)?;
    project.manifest = manifest;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_project(
    State(host): State<Arc<SharedHost>>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    host.check_admin(&headers)?;
    // The write lock waits for all in-flight dispatches, including response
    // generation, before releasing the engine and its open log files.
    let mut projects = host.projects.write().await;
    if let Some(project) = projects.get_mut(&id) {
        let mut manifest = project.manifest.clone();
        manifest.active = false;
        persist_manifest(&host.root.join(id.to_string()), &manifest).map_err(internal)?;
        project.manifest = manifest;
    }
    projects.remove(&id);
    let path = host.root.join(id.to_string());
    if path.exists() {
        if path.canonicalize().map_err(internal)?.parent() != Some(host.root.as_path()) {
            return Err(error(StatusCode::BAD_REQUEST, "invalid project directory"));
        }
        std::fs::remove_dir_all(path).map_err(internal)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn dispatch(
    State(host): State<Arc<SharedHost>>,
    Path((id, path)): Path<(Uuid, String)>,
    mut req: Request,
) -> Response {
    let projects = host.projects.read().await;
    let Some(project) = projects.get(&id) else {
        return error(StatusCode::NOT_FOUND, "project not found").into_response();
    };
    if !authorized(req.headers(), &project.manifest.token_hash) {
        return error(StatusCode::UNAUTHORIZED, "invalid project credentials").into_response();
    }
    if !project.manifest.active {
        return error(StatusCode::CONFLICT, "project is stopped").into_response();
    }
    // These are process/file administration, not tenant data operations.
    // Snapshot export and synchronous data operations stay scoped to
    // this project's engine. No global metrics or local model paths escape.
    if path == "metrics"
        || path.starts_with("v1/keys")
        || path.starts_with("v1/models/")
        || path.starts_with("v1/storage/")
        || path.starts_with("v1/replication/")
        || path.starts_with("internal/")
        || path.starts_with("v1/crypto/")
        || path == "v1/records/encrypted"
        || path == "v1/snapshot/save"
        || path == "v1/snapshot/restore"
        || path == "v1/snapshot/upload"
        || (path.ends_with("/index") && req.method() != axum::http::Method::GET)
        || path == "v1/index/rebuild"
    {
        return error(
            StatusCode::FORBIDDEN,
            "operation unavailable on shared hosting",
        )
        .into_response();
    }
    let uri = match req.uri().query() {
        Some(query) => format!("/{path}?{query}"),
        None => format!("/{path}"),
    };
    let Ok(uri) = uri.parse::<Uri>() else {
        return error(StatusCode::BAD_REQUEST, "invalid path").into_response();
    };
    *req.uri_mut() = uri;
    // The outer router captured (project UUID, wildcard path). Those captures
    // must not be appended to the inner handler's record/node ID parameters.
    req.extensions_mut().clear();
    let response = project.router.clone().oneshot(req).await.unwrap();
    // Also flush bulk writes and snapshot restores before acknowledging them.
    if let Some(c) = project.engine.write().await.event_committer_mut() {
        if let Err(e) = c.flush_log() {
            return internal(e).into_response();
        }
    }
    response
}

/// Complete durable data needed for transfer; never accepts filesystem paths.
/// Logs (rather than snapshots alone) retain the audit chain and mixed-dimension
/// collections. Transient query caches are intentionally reconstructed.
#[derive(Serialize, Deserialize)]
pub struct ProjectExport {
    pub event_log: String,
    pub metadata: Option<String>,
    pub namespaces: Option<String>,
    pub state_hash: String,
}

async fn export_project(
    State(host): State<Arc<SharedHost>>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> ApiResult<Json<ProjectExport>> {
    use base64::Engine as _;
    host.check_admin(&headers)?;
    let projects = host.projects.read().await;
    let project = projects
        .get(&id)
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "project not found"))?;
    if project.manifest.active {
        return Err(error(StatusCode::CONFLICT, "stop project before export"));
    }
    let mut engine = project.engine.write().await;
    engine.flush_pending_events().map_err(internal)?;
    engine.flush_metadata().map_err(internal)?;
    let path = host.root.join(id.to_string());
    let encode = |name: &str| -> Result<String, std::io::Error> {
        Ok(base64::engine::general_purpose::STANDARD.encode(std::fs::read(path.join(name))?))
    };
    let optional = |name: &str| -> ApiResult<Option<String>> {
        if path.join(name).exists() {
            Ok(Some(encode(name).map_err(internal)?))
        } else {
            Ok(None)
        }
    };
    Ok(Json(ProjectExport {
        event_log: encode("events.log").map_err(internal)?,
        metadata: optional("events.metadata.json")?,
        namespaces: optional("events.namespaces.json")?,
        state_hash: valori_kernel::snapshot::blake3::hash_state_blake3(&engine.state)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    }))
}

/// Control-plane transfer target on dedicated standalone nodes. Requires their
/// existing admin-scoped node credential via the normal router middleware.
pub async fn import_project(
    State(state): State<SharedEngine>,
    Json(bundle): Json<ProjectExport>,
) -> ApiResult<Json<serde_json::Value>> {
    use base64::Engine as _;
    use std::io::Write;
    let decode = |v: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(v)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "invalid transfer encoding"))
    };
    let log = decode(&bundle.event_log)?;
    let metadata = bundle.metadata.as_deref().map(decode).transpose()?;
    let namespaces = bundle.namespaces.as_deref().map(decode).transpose()?;
    let mut engine = state.write().await;
    let log_path = engine
        .event_committer()
        .ok_or_else(|| {
            error(
                StatusCode::CONFLICT,
                "destination requires event-log persistence",
            )
        })?
        .event_log()
        .path()
        .to_path_buf();
    // Only empty nodes or exact retries may accept an import. A paid project
    // with existing writes can never be overwritten by a delayed retry.
    engine.flush_pending_events().map_err(internal)?;
    let current_log = std::fs::read(&log_path).map_err(internal)?;
    let parent = log_path
        .parent()
        .ok_or_else(|| internal("missing data directory"))?;
    let last_import = parent.join("shared-import.hash");
    let previous_import = std::fs::read_to_string(&last_import).ok();
    let retry_destination =
        previous_import.as_deref() == Some(blake3::hash(&current_log).to_hex().as_str());
    if engine.state.version() != 0 && current_log != log && !retry_destination {
        return Err(error(StatusCode::CONFLICT, "destination is not empty"));
    }
    let stage = parent.join("shared-transfer");
    std::fs::create_dir_all(&stage).map_err(internal)?;
    let staged_log = stage.join("events.log");
    std::fs::write(&staged_log, &log).map_err(internal)?;
    let (recovered, _, _) = valori_state::bootstrap::recover_from_events(&staged_log)
        .map_err(|_| error(StatusCode::BAD_REQUEST, "invalid transfer event log"))?;
    let expected: String = valori_kernel::snapshot::blake3::hash_state_blake3(&recovered)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if expected != bundle.state_hash {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "transfer state hash mismatch",
        ));
    }
    // Sidecars are validated before any live files change.
    if let Some(bytes) = &metadata {
        serde_json::from_slice::<std::collections::HashMap<String, serde_json::Value>>(bytes)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "invalid metadata sidecar"))?;
    }
    if let Some(bytes) = &namespaces {
        serde_json::from_slice::<valori_metadata::collection::CollectionRegistry>(bytes)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "invalid namespace sidecar"))?;
    }
    let metadata_path = log_path.with_extension("metadata.json");
    let namespaces_path = log_path.with_extension("namespaces.json");
    // Close the destination file handle before replacing it (Windows too).
    engine.persistence = Persistence::Ephemeral;
    let write = |path: &FsPath, bytes: &[u8]| -> ApiResult<()> {
        let mut file = std::fs::File::create(path).map_err(internal)?;
        file.write_all(bytes).map_err(internal)?;
        file.sync_all().map_err(internal)
    };
    if let Some(bytes) = metadata {
        write(&metadata_path, &bytes)?;
    }
    if let Some(bytes) = namespaces {
        write(&namespaces_path, &bytes)?;
    }
    write(&log_path, &log)?;
    let writer = valori_storage::events::event_log::EventLogWriter::open(&log_path, None)
        .map_err(internal)?;
    engine.persistence =
        Persistence::EventLog(valori_storage::events::event_commit::EventCommitter::new(
            writer,
            valori_storage::events::EventJournal::new(),
            valori_kernel::state::kernel::KernelState::new(),
        ));
    engine.try_recover();
    let actual: String = valori_kernel::snapshot::blake3::hash_state_blake3(&engine.state)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual != expected {
        return Err(internal("imported state did not match validated export"));
    }
    write(&last_import, blake3::hash(&log).to_hex().as_bytes())?;
    Ok(Json(serde_json::json!({"state_hash": actual})))
}
