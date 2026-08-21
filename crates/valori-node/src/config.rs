// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

// IndexKind and Metric come from canonical valori-domain; QuantizationKind from valori-engine.
pub use valori_domain::{IndexKind, Metric};
pub use valori_engine::QuantizationKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeMode {
    Leader,
    Follower { leader_url: String },
}

impl Default for NodeMode {
    fn default() -> Self {
        Self::Leader
    }
}

#[derive(Clone)]
pub struct NodeConfig {
    pub max_records: usize,
    pub quantization_kind: QuantizationKind,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub bind_addr: SocketAddr,

    // Persistence
    pub snapshot_path: Option<PathBuf>,
    pub wal_path: Option<PathBuf>,
    pub event_log_path: Option<PathBuf>, // Added explicit config

    // ── Phase 2.3: StorageProvider-backed recovery ──────────────────────────
    // Env: VALORI_STORAGE_ROOT — a directory `LocalStorageProvider` owns
    // (see crates/valori-storage/src/provider/local.rs). When both this AND
    // `project_id` are set, `main.rs` constructs the provider and configures
    // `Engine` with it BEFORE `try_recover()`, making the manifest-driven
    // snapshot+WAL-tail path the one actually used — not an opt-in call a
    // caller has to remember. Absent (the case for every node that predates
    // this env var, or that was never given one) means the pre-existing
    // whole-process event-log/WAL/snapshot path remains in effect,
    // unchanged — the explicit, disclosed compatibility boundary; see
    // `docs/phases/phase-collection-storage-runtime-integration.md`.
    pub storage_root: Option<PathBuf>,
    // Env: VALORI_PROJECT_ID — the project's durable identity (a UUID,
    // matching `valori_domain::ProjectId`; the daemon already mints one per
    // project in `project.json`'s `id` field and now also exports it here).
    // Required alongside `storage_root` — a `StorageProvider` with no
    // `ProjectId` has nothing to scope its `StorageKey`s to.
    pub project_id: Option<valori_domain::ProjectId>,
    // Env: VALORI_PROJECT_NAME — display name published into `ProjectManifest`
    // the first time a `StorageProvider` is configured. Purely cosmetic
    // (never part of storage identity — `project_id` is); falls back to a
    // safe synthesized name if unset or invalid.
    pub project_name: Option<String>,

    // Env: VALORI_EVENT_LOG_ROTATION_BYTES (default: 256 MiB in standalone, config-dependent in cluster)
    // Trigger an audit log rotation after this many bytes.
    pub event_log_rotation_bytes: Option<u64>,

    /// Deprecated: use snapshot_every_events / snapshot_every_bytes instead.
    /// Retained for backward compatibility; triggers a startup warning if set
    /// without the new cadence knobs. Will be removed in Phase 3.
    pub auto_snapshot_interval_secs: Option<u64>,

    // ── Phase 1.8 storage policy ──────────────────────────────────────────────
    // Env: VALORI_SNAPSHOT_EVERY_EVENTS
    // Trigger a snapshot after this many events since the last snapshot.
    pub snapshot_every_events: Option<u64>,

    // Env: VALORI_SNAPSHOT_EVERY_BYTES (default: 64 MiB)
    // Trigger a snapshot after this many bytes of log have been appended.
    pub snapshot_every_bytes: Option<u64>,

    // Env: VALORI_SNAPSHOT_KEEP (default: 3)
    // Number of most recent snapshot files to retain.
    pub snapshot_keep: Option<u32>,

    // Env: VALORI_ZSTD_LEVEL (default: 3)
    // zstd compression level applied to sealed (rotated) segment files.
    // Implementation: Phase 1.7/1.8 (seam reads the value; compressor wired later).
    pub zstd_compression_level: Option<i32>,

    // Env: VALORI_GENESIS_REPLAY=1
    // If true, skip snapshots and replay from genesis on startup (audit mode).
    pub genesis_replay: bool,

    // ── Phase 1.10 / 1.11 ────────────────────────────────────────────────────
    // Env: VALORI_NODE_ID
    // Stable numeric identity for this node. Phase 2: openraft NodeId.
    pub node_id: Option<u32>,

    // Set by --health-check CLI argument (Phase 1.11).
    // Runs a single GET /v1/health and exits 0/1. Used by distroless Docker HEALTHCHECK.
    pub health_check_mode: bool,

    // Security
    pub auth_token: Option<String>,
    /// Path to the JSON file persisting API keys (Phase 3.5).
    /// Env: `VALORI_KEYS_PATH`. Absent = key store is in-memory only (resets on restart).
    pub keys_path: Option<PathBuf>,

    // Phase 3.6: Crypto-shredding
    // Env: VALORI_SHRED_LOG_PATH
    // Append-only file of shredded key_ids (hex). Absent = in-memory only.
    pub shred_log_path: Option<PathBuf>,

    // Clustering
    pub mode: NodeMode,

    // ── Phase 3.1: object store ───────────────────────────────────────────────
    // Env: VALORI_OBJECT_STORE_URL
    // s3://bucket/prefix  or  file:///local/path
    // Absent = object store disabled (local-only mode).
    pub object_store_url: Option<String>,

    // Env: VALORI_OBJECT_STORE_KEEP (default: 7)
    // Number of snapshots to retain in the object store after pruning.
    pub object_store_keep: u32,

    // Env: VALORI_CORS_ORIGIN
    // Absent = no CORS headers (API-only, no browser access).
    // "*"    = permissive (all origins allowed — dev only).
    // "https://app.example.com" = single origin (production).
    pub cors_origin: Option<String>,

    // ── Phase 3.13: HNSW parameter exposure ──────────────────────────────────
    // Only take effect when VALORI_INDEX=hnsw. Absent = use HnswConfig defaults.
    // Env: VALORI_HNSW_M (default 16) — max edges per node per layer
    pub hnsw_m: Option<usize>,
    // Env: VALORI_HNSW_EF_CONSTRUCTION (default 100) — beam width during index build
    pub hnsw_ef_construction: Option<usize>,
    // Env: VALORI_HNSW_EF_SEARCH (default 50) — beam width during query
    pub hnsw_ef_search: Option<usize>,

    // ── IVF parameter overrides ───────────────────────────────────────────────
    // Only take effect when VALORI_INDEX=ivf. When absent, auto-scaling applies:
    // n_list = max(16, sqrt(N)), n_probe = max(1, sqrt(n_list)).
    // Env: VALORI_IVF_N_LIST  — fix centroid count (disables auto-scale)
    pub ivf_n_list: Option<usize>,
    // Env: VALORI_IVF_N_PROBE — fix probe count (disables auto-scale)
    pub ivf_n_probe: Option<usize>,

    // ── BQ parameter overrides (S11.3) ────────────────────────────────────────
    // Only take effect when VALORI_INDEX=bq. Absent = BqConfig defaults
    // (pool_factor=10, min_candidates=200 — unchanged from pre-S11 behavior).
    // Env: VALORI_BQ_POOL_FACTOR — candidate pool = max(pool_factor*k, min_candidates)
    pub bq_pool_factor: Option<usize>,
    // Env: VALORI_BQ_MIN_CANDIDATES — floor on the candidate pool size
    pub bq_min_candidates: Option<usize>,

    // ── Standalone sharding ──────────────────────────────────────────────────
    // Number of independent shards in standalone mode.
    // Namespaces are routed to shards via `namespace_id % shard_count`.
    // Each shard gets its own event-log file: events-shard0.log, events-shard1.log, ...
    // Env: VALORI_SHARD_COUNT (default: 1 = no sharding, byte-identical to pre-sharding)
    pub shard_count: usize,

    // ── Phase C4.1: time-decay re-ranking ────────────────────────────────────
    // Default half-life (seconds) applied to search ranking when a request does
    // not specify its own. Absent or 0 = decay off (pure distance ranking).
    // Env: VALORI_DECAY_HALF_LIFE_SECS
    pub decay_half_life_secs: Option<u64>,

    // ── Phase I2: on-node embedding ───────────────────────────────────────────
    // When set, /v1/ingest calls the embedding provider and inserts vectors
    // without the client needing to run its own embed step.
    //
    // VALORI_EMBED_PROVIDER: ollama | openai | custom   (absent = embedding disabled)
    // VALORI_EMBED_MODEL:    e.g. nomic-embed-text, text-embedding-3-small
    // VALORI_EMBED_URL:      base URL of the provider  (default per provider)
    // VALORI_EMBED_API_KEY:  API key (required for openai/custom if auth needed)
    pub embed_provider: Option<String>,
    pub embed_model: Option<String>,
    pub embed_url: Option<String>,
    pub embed_api_key: Option<String>,
}

// Hand-written Debug (not derived): the derived impl printed `auth_token`
// and `embed_api_key` in plaintext on every node startup at INFO level
// (`main.rs`'s "Initializing Valori Node with config: {:?}"), which is
// exactly the worker's own credential (VALORI_AUTH_TOKEN — the same
// secret Cloud attaches to every proxied request) landing in real
// container logs. Found via the Local Cloud E2E suite's log sweep, not
// by inspection. Every other field is printed exactly as `derive(Debug)`
// would — only these two are redacted.
impl std::fmt::Debug for NodeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn redacted(v: &Option<String>) -> &'static str {
            if v.is_some() {
                "Some(\"[REDACTED]\")"
            } else {
                "None"
            }
        }
        f.debug_struct("NodeConfig")
            .field("max_records", &self.max_records)
            .field("quantization_kind", &self.quantization_kind)
            .field("max_nodes", &self.max_nodes)
            .field("max_edges", &self.max_edges)
            .field("bind_addr", &self.bind_addr)
            .field("snapshot_path", &self.snapshot_path)
            .field("wal_path", &self.wal_path)
            .field("event_log_path", &self.event_log_path)
            .field("event_log_rotation_bytes", &self.event_log_rotation_bytes)
            .field(
                "auto_snapshot_interval_secs",
                &self.auto_snapshot_interval_secs,
            )
            .field("snapshot_every_events", &self.snapshot_every_events)
            .field("snapshot_every_bytes", &self.snapshot_every_bytes)
            .field("snapshot_keep", &self.snapshot_keep)
            .field("zstd_compression_level", &self.zstd_compression_level)
            .field("genesis_replay", &self.genesis_replay)
            .field("node_id", &self.node_id)
            .field("health_check_mode", &self.health_check_mode)
            .field(
                "auth_token",
                &format_args!("{}", redacted(&self.auth_token)),
            )
            .field("keys_path", &self.keys_path)
            .field("shred_log_path", &self.shred_log_path)
            .field("mode", &self.mode)
            .field("object_store_url", &self.object_store_url)
            .field("object_store_keep", &self.object_store_keep)
            .field("cors_origin", &self.cors_origin)
            .field("hnsw_m", &self.hnsw_m)
            .field("hnsw_ef_construction", &self.hnsw_ef_construction)
            .field("hnsw_ef_search", &self.hnsw_ef_search)
            .field("ivf_n_list", &self.ivf_n_list)
            .field("ivf_n_probe", &self.ivf_n_probe)
            .field("bq_pool_factor", &self.bq_pool_factor)
            .field("bq_min_candidates", &self.bq_min_candidates)
            .field("shard_count", &self.shard_count)
            .field("decay_half_life_secs", &self.decay_half_life_secs)
            .field("embed_provider", &self.embed_provider)
            .field("embed_model", &self.embed_model)
            .field("embed_url", &self.embed_url)
            .field(
                "embed_api_key",
                &format_args!("{}", redacted(&self.embed_api_key)),
            )
            .finish()
    }
}

impl Default for NodeConfig {
    fn default() -> Self {
        let max_records = std::env::var("VALORI_MAX_RECORDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1_000_000);

        let max_nodes = std::env::var("VALORI_MAX_NODES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100_000);

        let max_edges = std::env::var("VALORI_MAX_EDGES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(500_000);

        let bind_addr = std::env::var("VALORI_BIND")
            .unwrap_or_else(|_| "0.0.0.0:3000".to_string())
            .parse()
            .expect("Invalid Bind Address");

        let quantization_kind = match std::env::var("VALORI_QUANT").as_deref() {
            Ok("scalar") => QuantizationKind::Scalar,
            Ok("product") => QuantizationKind::Product,
            _ => QuantizationKind::None,
        };

        // Arithmetic format. Unlike other knobs this NEVER falls back
        // silently: precision is identity-defining (different format =
        // different hashes, different search results), so a typo or an
        // unimplemented format must stop the process, not default away.
        let format_name = std::env::var("VALORI_FORMAT").unwrap_or_else(|_| "q16.16".to_string());
        match valori_kernel::fxp::format::parse_format(&format_name) {
            Some(id) if id == valori_kernel::fxp::format::ACTIVE_FORMAT_ID => {}
            Some(_) => panic!(
                "VALORI_FORMAT='{format_name}' is a recognized format but this \
                 build only implements q16.16 (see FxpFormat in valori-kernel)"
            ),
            None => panic!(
                "VALORI_FORMAT='{format_name}' is not a known format \
                 (known: q16.16, q8.8, q32.32; implemented: q16.16)"
            ),
        }

        let snapshot_path = std::env::var("VALORI_SNAPSHOT_PATH")
            .ok()
            .map(PathBuf::from);

        let wal_path = std::env::var("VALORI_WAL_PATH").ok().map(PathBuf::from);

        let auto_snapshot_interval_secs = std::env::var("VALORI_SNAPSHOT_INTERVAL")
            .ok()
            .and_then(|v| v.parse::<u64>().ok());

        let snapshot_every_events = std::env::var("VALORI_SNAPSHOT_EVERY_EVENTS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok());
        let snapshot_every_bytes = std::env::var("VALORI_SNAPSHOT_EVERY_BYTES")
            .ok()
            .and_then(|v| v.parse::<u64>().ok());
        let snapshot_keep = std::env::var("VALORI_SNAPSHOT_KEEP")
            .ok()
            .and_then(|v| v.parse::<u32>().ok());
        let zstd_compression_level = std::env::var("VALORI_ZSTD_LEVEL")
            .ok()
            .and_then(|v| v.parse::<i32>().ok());
        let genesis_replay = std::env::var("VALORI_GENESIS_REPLAY")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let node_id = std::env::var("VALORI_NODE_ID")
            .ok()
            .and_then(|v| v.parse::<u32>().ok());

        let auth_token = std::env::var("VALORI_AUTH_TOKEN").ok();
        let keys_path = std::env::var("VALORI_KEYS_PATH").ok().map(PathBuf::from);
        let shred_log_path = std::env::var("VALORI_SHRED_LOG_PATH")
            .ok()
            .map(PathBuf::from);

        let object_store_url = std::env::var("VALORI_OBJECT_STORE_URL").ok();
        let object_store_keep = std::env::var("VALORI_OBJECT_STORE_KEEP")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(7);

        let cors_origin = std::env::var("VALORI_CORS_ORIGIN").ok();

        let hnsw_m = std::env::var("VALORI_HNSW_M")
            .ok()
            .and_then(|v| v.parse().ok());
        let hnsw_ef_construction = std::env::var("VALORI_HNSW_EF_CONSTRUCTION")
            .ok()
            .and_then(|v| v.parse().ok());
        let hnsw_ef_search = std::env::var("VALORI_HNSW_EF_SEARCH")
            .ok()
            .and_then(|v| v.parse().ok());

        let ivf_n_list: Option<usize> = std::env::var("VALORI_IVF_N_LIST")
            .ok()
            .and_then(|v| v.parse().ok());
        let ivf_n_probe: Option<usize> = std::env::var("VALORI_IVF_N_PROBE")
            .ok()
            .and_then(|v| v.parse().ok());

        let bq_pool_factor: Option<usize> = std::env::var("VALORI_BQ_POOL_FACTOR")
            .ok()
            .and_then(|v| v.parse().ok());
        let bq_min_candidates: Option<usize> = std::env::var("VALORI_BQ_MIN_CANDIDATES")
            .ok()
            .and_then(|v| v.parse().ok());

        let decay_half_life_secs = std::env::var("VALORI_DECAY_HALF_LIFE_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|&v| v > 0);

        let embed_provider = std::env::var("VALORI_EMBED_PROVIDER").ok();
        let embed_model = std::env::var("VALORI_EMBED_MODEL").ok();
        let embed_url = std::env::var("VALORI_EMBED_URL").ok();
        let embed_api_key = std::env::var("VALORI_EMBED_API_KEY").ok();

        // Mode
        let mode = if let Ok(url) = std::env::var("VALORI_FOLLOWER_OF") {
            NodeMode::Follower { leader_url: url }
        } else {
            NodeMode::Leader
        };

        let event_log_path = std::env::var("VALORI_EVENT_LOG_PATH")
            .ok()
            .map(PathBuf::from);

        let shard_count = std::env::var("VALORI_SHARD_COUNT")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1)
            .max(1);

        let event_log_rotation_bytes = std::env::var("VALORI_EVENT_LOG_ROTATION_BYTES")
            .ok()
            .and_then(|v| v.parse::<u64>().ok());

        let storage_root = std::env::var("VALORI_STORAGE_ROOT").ok().map(PathBuf::from);
        let project_id = std::env::var("VALORI_PROJECT_ID")
            .ok()
            .and_then(|v| v.parse::<valori_domain::ProjectId>().ok());
        let project_name = std::env::var("VALORI_PROJECT_NAME").ok();

        Self {
            max_records,
            max_nodes,
            max_edges,
            bind_addr,
            quantization_kind,
            snapshot_path,
            wal_path,
            event_log_path,
            event_log_rotation_bytes,
            storage_root,
            project_id,
            project_name,
            auto_snapshot_interval_secs,
            snapshot_every_events,
            snapshot_every_bytes,
            snapshot_keep,
            zstd_compression_level,
            genesis_replay,
            node_id,
            health_check_mode: false, // set by CLI arg, not env var
            auth_token,
            keys_path,
            shred_log_path,
            mode,
            object_store_url,
            object_store_keep,
            cors_origin,
            hnsw_m,
            hnsw_ef_construction,
            hnsw_ef_search,
            ivf_n_list,
            ivf_n_probe,
            bq_pool_factor,
            bq_min_candidates,
            shard_count,
            decay_half_life_secs,
            embed_provider,
            embed_model,
            embed_url,
            embed_api_key,
        }
    }
}
