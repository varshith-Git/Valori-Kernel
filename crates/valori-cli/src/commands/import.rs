// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 3.7 — `valori import` subcommand.
//!
//! Imports vectors from an external source into a running Valori node.
//! Every run validates that the source dimension matches the target node's
//! declared dim before touching any data.
//!
//! Sources supported:
//!   - Qdrant (scroll API, cursor-based, resumable)
//!   - JSONL  (`{"vector": [...], "metadata": "...", "tag": 0}` lines)

use anyhow::{bail, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::{Duration, Instant};

// ── Random key ────────────────────────────────────────────────────────────────

/// 16 cryptographically random bytes from the OS — used as per-record
/// idempotency keys. Uses getrandom so this is safe on Windows/non-unix
/// (the old time+counter fallback was predictable and could false-dedup
/// on concurrent or resumed imports — H-1 fix).
fn random_key() -> [u8; 16] {
    let mut key = [0u8; 16];
    getrandom::getrandom(&mut key).expect("OS CSPRNG unavailable");
    key
}

// ── Sidecar (resumability state) ──────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct ImportState {
    source_kind: String,
    source_url: Option<String>,
    source_collection: Option<String>,
    source_file: Option<String>,
    target_url: String,
    target_collection: String,
    dim: usize,
    imported: u64,
    /// Qdrant: next_page_offset from the last successful scroll response.
    last_offset: Option<serde_json::Value>,
    started_at: String,
    updated_at: String,
}

/// M-3: write the sidecar to ~/.valori/ instead of the current working
/// directory. The cwd is world-readable on shared machines; ~/.valori/ is
/// chmod 0700 (set at creation). If home-dir resolution fails, fall back to cwd.
fn sidecar_path(target_collection: &str, source_kind: &str) -> PathBuf {
    let name = format!(".valori-import-{source_kind}-{target_collection}.json");
    let dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".valori");
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    dir.join(name)
}

fn load_state(path: &PathBuf) -> Option<ImportState> {
    let data = std::fs::read(path).ok()?;
    serde_json::from_slice(&data).ok()
}

fn save_state(path: &PathBuf, state: &ImportState) {
    if let Ok(json) = serde_json::to_vec_pretty(state) {
        // Write to a temp file then rename (atomic) and set 0600.
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, &json).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
            }
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

// ── Valori HTTP helpers ───────────────────────────────────────────────────────

struct ValoriClient {
    url: String,
    token: Option<String>,
}

impl ValoriClient {
    fn new(url: &str, token: Option<String>) -> Self {
        ValoriClient { url: url.trim_end_matches('/').to_string(), token }
    }

    fn agent(&self) -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout_read(Duration::from_secs(30))
            .timeout_write(Duration::from_secs(30))
            .build()
    }

    fn auth_header(&self, req: ureq::Request) -> ureq::Request {
        if let Some(ref tok) = self.token {
            req.set("Authorization", &format!("Bearer {tok}"))
        } else {
            req
        }
    }

    /// `GET /health` → dimension the node was started with.
    fn get_dim(&self) -> Result<usize> {
        let resp = self
            .auth_header(self.agent().get(&format!("{}/health", self.url)))
            .call()
            .context("GET /health failed — is Valori running?")?;
        let body: serde_json::Value = resp.into_json()?;
        body["dim"]
            .as_u64()
            .map(|d| d as usize)
            .context("/health response missing 'dim' field")
    }

    /// Create a collection (idempotent — 400 if already exists is swallowed).
    fn ensure_collection(&self, name: &str) -> Result<()> {
        if name == "default" {
            return Ok(());
        }
        let payload = serde_json::json!({ "name": name });
        match self
            .auth_header(self.agent().post(&format!("{}/v1/namespaces", self.url)))
            .send_json(payload)
        {
            Ok(_) => Ok(()),
            Err(ureq::Error::Status(400, _)) => Ok(()), // already exists
            Err(e) => Err(e).context("POST /v1/namespaces failed"),
        }
    }

    /// Insert one record; returns the assigned id.
    fn insert_one(&self, record: &ImportRecord, collection: &str) -> Result<u64> {
        let key_bytes: Vec<serde_json::Value> =
            record.key.iter().map(|b| serde_json::Value::Number((*b).into())).collect();

        let mut body = serde_json::json!({
            "values": record.vector,
            "request_id": key_bytes,
        });
        if collection != "default" {
            body["collection"] = serde_json::Value::String(collection.to_string());
        }
        if let Some(ref meta) = record.metadata {
            body["metadata"] = serde_json::Value::String(meta.clone());
        }
        if record.tag != 0 {
            body["tag"] = serde_json::Value::Number(record.tag.into());
        }

        let resp = self
            .auth_header(self.agent().post(&format!("{}/records", self.url)))
            .send_json(body)
            .context("POST /records failed")?;
        let resp_body: serde_json::Value = resp.into_json()?;
        Ok(resp_body["id"].as_u64().unwrap_or(0))
    }

    fn batch_insert(&self, batch: &[ImportRecord], collection: &str) -> Result<()> {
        for record in batch {
            self.insert_one(record, collection)?;
        }
        Ok(())
    }
}

struct ImportRecord {
    vector: Vec<f32>,
    metadata: Option<String>,
    tag: u64,
    key: [u8; 16],
}

// ── Progress bar ──────────────────────────────────────────────────────────────

fn make_progress(total_hint: Option<u64>) -> ProgressBar {
    let pb = if let Some(n) = total_hint {
        ProgressBar::new(n)
    } else {
        ProgressBar::new_spinner()
    };
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} records  ({per_sec})"
        )
        .unwrap_or_else(|_| ProgressStyle::default_bar())
        .progress_chars("=>-"),
    );
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}

// ── Qdrant source ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct QdrantCollectionInfo {
    result: QdrantCollectionResult,
}

#[derive(Deserialize)]
struct QdrantCollectionResult {
    config: QdrantCollectionConfig,
    #[serde(default)]
    vectors_count: Option<u64>,
}

#[derive(Deserialize)]
struct QdrantCollectionConfig {
    params: QdrantCollectionParams,
}

#[derive(Deserialize)]
struct QdrantCollectionParams {
    vectors: serde_json::Value,
}

fn qdrant_dim(params: &QdrantCollectionParams) -> Option<usize> {
    // Single unnamed vector: {"size": 384, ...}
    if let Some(n) = params.vectors.get("size").and_then(|v| v.as_u64()) {
        return Some(n as usize);
    }
    // Named vectors: {"my-vec": {"size": 384, ...}, ...} — pick the first.
    if let Some(map) = params.vectors.as_object() {
        for (_name, spec) in map {
            if let Some(n) = spec.get("size").and_then(|v| v.as_u64()) {
                return Some(n as usize);
            }
        }
    }
    None
}

#[derive(Deserialize)]
struct QdrantScrollResponse {
    result: QdrantScrollResult,
}

#[derive(Deserialize)]
struct QdrantScrollResult {
    points: Vec<QdrantPoint>,
    next_page_offset: serde_json::Value,
}

#[derive(Deserialize)]
struct QdrantPoint {
    #[allow(dead_code)]
    id: serde_json::Value,
    #[serde(default)]
    vector: serde_json::Value,
    #[serde(default)]
    payload: serde_json::Value,
}

fn qdrant_vector(point: &QdrantPoint) -> Option<Vec<f32>> {
    if let Some(arr) = point.vector.as_array() {
        return arr.iter().map(|x| x.as_f64().map(|f| f as f32)).collect();
    }
    if let Some(map) = point.vector.as_object() {
        for (_name, val) in map {
            if let Some(arr) = val.as_array() {
                let v: Option<Vec<f32>> = arr.iter().map(|x| x.as_f64().map(|f| f as f32)).collect();
                if v.is_some() { return v; }
            }
        }
    }
    None
}

fn qdrant_metadata(point: &QdrantPoint) -> Option<String> {
    if point.payload.is_null()
        || point.payload == serde_json::Value::Object(Default::default())
    {
        return None;
    }
    serde_json::to_string(&point.payload).ok()
}

pub struct QdrantImportArgs {
    pub qdrant_url: String,
    pub source_collection: String,
    pub target_url: String,
    pub target_collection: String,
    pub batch_size: usize,
    pub resume: bool,
    pub token: Option<String>,
}

pub fn run_qdrant(args: QdrantImportArgs) -> Result<()> {
    let qdrant_base = args.qdrant_url.trim_end_matches('/');
    let valori = ValoriClient::new(&args.target_url, args.token.clone());
    let sidecar = sidecar_path(&args.target_collection, "qdrant");

    // ── Dim validation ──────────────────────────────────────────────────────────
    let valori_dim = valori.get_dim()?;

    let coll_info: QdrantCollectionInfo = ureq::get(&format!(
        "{qdrant_base}/collections/{}",
        args.source_collection
    ))
    .call()
    .context("GET Qdrant collection info failed — is Qdrant running?")?
    .into_json()
    .context("Failed to parse Qdrant collection info")?;

    let qdrant_dim_val = qdrant_dim(&coll_info.result.config.params)
        .context("Could not determine source vector dimension from Qdrant collection info")?;

    if qdrant_dim_val != valori_dim {
        bail!(
            "Dimension mismatch: Qdrant source has dim={qdrant_dim_val} but \
             Valori node is configured with dim={valori_dim}.\n\
             Restart Valori with VALORI_DIM={qdrant_dim_val} before importing."
        );
    }

    let total_hint = coll_info.result.vectors_count;
    println!(
        "Source: qdrant://{}/{} (dim={qdrant_dim_val}{})",
        qdrant_base,
        args.source_collection,
        total_hint.map(|n| format!(", ~{n} vectors")).unwrap_or_default()
    );
    println!("Target: {}/{}", args.target_url, args.target_collection);

    // ── Resume or fresh start ───────────────────────────────────────────────────
    let mut state: ImportState = if args.resume {
        match load_state(&sidecar) {
            Some(s) => {
                println!(
                    "Resuming from offset {:?} ({} already imported)",
                    s.last_offset, s.imported
                );
                s
            }
            None => {
                println!("No resume sidecar found — starting fresh.");
                fresh_qdrant_state(qdrant_base, &args, valori_dim)
            }
        }
    } else {
        fresh_qdrant_state(qdrant_base, &args, valori_dim)
    };

    valori.ensure_collection(&args.target_collection)?;

    let pb = make_progress(total_hint.map(|n| n.saturating_sub(state.imported)));
    let start = Instant::now();
    let mut offset = state.last_offset.clone();

    loop {
        let mut scroll_body = serde_json::json!({
            "limit": args.batch_size,
            "with_vector": true,
            "with_payload": true,
        });
        if let Some(ref off) = offset {
            if !off.is_null() {
                scroll_body["offset"] = off.clone();
            }
        }

        let scroll_resp: QdrantScrollResponse =
            ureq::post(&format!(
                "{qdrant_base}/collections/{}/points/scroll",
                args.source_collection
            ))
            .send_json(scroll_body)
            .context("Qdrant scroll failed")?
            .into_json()
            .context("Failed to parse Qdrant scroll response")?;

        let points = &scroll_resp.result.points;
        if points.is_empty() {
            break;
        }

        for point in points {
            let Some(vector) = qdrant_vector(point) else { continue };
            if vector.len() != valori_dim {
                eprintln!(
                    "Warning: point has dim={} (expected {}), skipping",
                    vector.len(), valori_dim
                );
                continue;
            }
            let record = ImportRecord {
                vector,
                metadata: qdrant_metadata(point),
                tag: 0,
                key: random_key(),
            };
            valori.insert_one(&record, &args.target_collection)?;
            state.imported += 1;
            pb.inc(1);
        }

        let next = scroll_resp.result.next_page_offset.clone();
        state.last_offset = Some(next.clone());
        state.updated_at = now_iso();
        save_state(&sidecar, &state);

        if next.is_null() {
            break;
        }
        offset = Some(next);
    }

    pb.finish_with_message(format!(
        "Done — imported {} records in {:.1}s",
        state.imported,
        start.elapsed().as_secs_f64()
    ));
    println!("State hash: {}", valori_get_hash(&valori)?);
    let _ = std::fs::remove_file(&sidecar);
    Ok(())
}

fn fresh_qdrant_state(qdrant_base: &str, args: &QdrantImportArgs, dim: usize) -> ImportState {
    ImportState {
        source_kind: "qdrant".into(),
        source_url: Some(qdrant_base.to_string()),
        source_collection: Some(args.source_collection.clone()),
        source_file: None,
        target_url: args.target_url.clone(),
        target_collection: args.target_collection.clone(),
        dim,
        imported: 0,
        last_offset: None,
        started_at: now_iso(),
        updated_at: now_iso(),
    }
}

fn valori_get_hash(valori: &ValoriClient) -> Result<String> {
    let resp = valori
        .auth_header(valori.agent().get(&format!("{}/v1/proof/state", valori.url)))
        .call()
        .context("GET /v1/proof/state failed")?;
    let body: serde_json::Value = resp.into_json()?;
    Ok(body["final_state_hash"].as_str().unwrap_or("<unknown>").to_string())
}

// ── JSONL source ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct JsonlRecord {
    #[serde(alias = "embedding", alias = "values")]
    vector: Vec<f32>,
    #[serde(default, alias = "text", alias = "content", alias = "payload")]
    metadata: Option<String>,
    #[serde(default)]
    tag: u64,
}

pub struct JsonlImportArgs {
    pub file: PathBuf,
    pub target_url: String,
    pub target_collection: String,
    pub batch_size: usize,
    pub token: Option<String>,
}

pub fn run_jsonl(args: JsonlImportArgs) -> Result<()> {
    let valori = ValoriClient::new(&args.target_url, args.token.clone());
    let valori_dim = valori.get_dim()?;

    let file = std::fs::File::open(&args.file)
        .with_context(|| format!("Cannot open {:?}", args.file))?;
    let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let reader = BufReader::new(file);

    println!("Source: {:?} (JSONL, dim expected={})", args.file, valori_dim);
    println!("Target: {}/{}", args.target_url, args.target_collection);

    valori.ensure_collection(&args.target_collection)?;

    // Use file size as a byte-level progress hint (not exact record count, but useful).
    let pb = make_progress(if file_size > 0 { Some(file_size) } else { None });
    let start = Instant::now();
    let mut batch: Vec<ImportRecord> = Vec::with_capacity(args.batch_size);
    let mut total: u64 = 0;
    let mut skipped: u64 = 0;
    let mut bytes_read: u64 = 0;

    for (line_no, line_result) in reader.lines().enumerate() {
        let line = line_result.with_context(|| format!("Line {} read error", line_no + 1))?;
        bytes_read += line.len() as u64 + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let rec: JsonlRecord = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Warning: line {} parse error ({e}), skipping", line_no + 1);
                skipped += 1;
                continue;
            }
        };

        if rec.vector.len() != valori_dim {
            eprintln!(
                "Warning: line {} has dim={} (expected {}), skipping",
                line_no + 1, rec.vector.len(), valori_dim
            );
            skipped += 1;
            continue;
        }

        batch.push(ImportRecord {
            vector: rec.vector,
            metadata: rec.metadata,
            tag: rec.tag,
            key: random_key(),
        });

        if batch.len() >= args.batch_size {
            let n = batch.len() as u64;
            valori.batch_insert(&batch, &args.target_collection)?;
            total += n;
            batch.clear();
            pb.set_position(bytes_read.min(file_size));
        }
    }

    if !batch.is_empty() {
        let n = batch.len() as u64;
        valori.batch_insert(&batch, &args.target_collection)?;
        total += n;
    }

    pb.finish_with_message(format!(
        "Done — imported {total} records ({skipped} skipped) in {:.1}s",
        start.elapsed().as_secs_f64()
    ));
    println!("State hash: {}", valori_get_hash(&valori)?);
    Ok(())
}
