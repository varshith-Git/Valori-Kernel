// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use std::net::SocketAddr;
use std::path::PathBuf;
// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexKind {
    BruteForce,
    Hnsw,
    Ivf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuantizationKind {
    None,
    Scalar,
    Product,
}

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

#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub max_records: usize,
    pub dim: usize,
    pub index_kind: IndexKind,
    pub quantization_kind: QuantizationKind,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub bind_addr: SocketAddr,

    // Persistence
    pub snapshot_path: Option<PathBuf>,
    pub wal_path: Option<PathBuf>,
    pub event_log_path: Option<PathBuf>, // Added explicit config

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
}

impl Default for NodeConfig {
    fn default() -> Self {
        let max_records = std::env::var("VALORI_MAX_RECORDS")
            .ok().and_then(|v| v.parse().ok())
            .unwrap_or(1024);
        
        let dim = std::env::var("VALORI_DIM")
            .ok().and_then(|v| v.parse().ok())
            .unwrap_or(16);
            
        let max_nodes = std::env::var("VALORI_MAX_NODES")
            .ok().and_then(|v| v.parse().ok())
            .unwrap_or(1024);

        let max_edges = std::env::var("VALORI_MAX_EDGES")
            .ok().and_then(|v| v.parse().ok())
            .unwrap_or(2048);

        let bind_addr = std::env::var("VALORI_BIND")
            .unwrap_or_else(|_| "127.0.0.1:3000".to_string())
            .parse()
            .expect("Invalid Bind Address");

        let index_kind = match std::env::var("VALORI_INDEX").as_deref() {
            Ok("hnsw") => IndexKind::Hnsw,
            Ok("ivf") => IndexKind::Ivf,
            _ => IndexKind::BruteForce,
        };

        let quantization_kind = match std::env::var("VALORI_QUANT").as_deref() {
            Ok("scalar") => QuantizationKind::Scalar,
            Ok("product") => QuantizationKind::Product,
            _ => QuantizationKind::None,
        };

        // Arithmetic format. Unlike other knobs this NEVER falls back
        // silently: precision is identity-defining (different format =
        // different hashes, different search results), so a typo or an
        // unimplemented format must stop the process, not default away.
        let format_name = std::env::var("VALORI_FORMAT")
            .unwrap_or_else(|_| "q16.16".to_string());
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
            .ok().map(PathBuf::from);
            
        let wal_path = std::env::var("VALORI_WAL_PATH")
            .ok().map(PathBuf::from);
            
        let auto_snapshot_interval_secs = std::env::var("VALORI_SNAPSHOT_INTERVAL")
            .ok().and_then(|v| v.parse::<u64>().ok());

        // Warn if deprecated knob is set without the new cadence knobs.
        // (Tracing may not be initialised yet — use eprintln so the warning
        //  always reaches the operator regardless of log config.)
        if auto_snapshot_interval_secs.is_some() {
            eprintln!(
                "WARN  valori: VALORI_SNAPSHOT_INTERVAL is deprecated. \
                 Use VALORI_SNAPSHOT_EVERY_EVENTS and/or VALORI_SNAPSHOT_EVERY_BYTES instead. \
                 Will be removed in Phase 3."
            );
        }

        let snapshot_every_events = std::env::var("VALORI_SNAPSHOT_EVERY_EVENTS")
            .ok().and_then(|v| v.parse::<u64>().ok());
        let snapshot_every_bytes = std::env::var("VALORI_SNAPSHOT_EVERY_BYTES")
            .ok().and_then(|v| v.parse::<u64>().ok());
        let snapshot_keep = std::env::var("VALORI_SNAPSHOT_KEEP")
            .ok().and_then(|v| v.parse::<u32>().ok());
        let zstd_compression_level = std::env::var("VALORI_ZSTD_LEVEL")
            .ok().and_then(|v| v.parse::<i32>().ok());
        let genesis_replay = std::env::var("VALORI_GENESIS_REPLAY")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let node_id = std::env::var("VALORI_NODE_ID")
            .ok().and_then(|v| v.parse::<u32>().ok());

        let auth_token = std::env::var("VALORI_AUTH_TOKEN").ok();
        let keys_path = std::env::var("VALORI_KEYS_PATH").ok().map(PathBuf::from);
        let shred_log_path = std::env::var("VALORI_SHRED_LOG_PATH").ok().map(PathBuf::from);

        let object_store_url = std::env::var("VALORI_OBJECT_STORE_URL").ok();
        let object_store_keep = std::env::var("VALORI_OBJECT_STORE_KEEP")
            .ok().and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(7);

        let cors_origin = std::env::var("VALORI_CORS_ORIGIN").ok();

        let hnsw_m = std::env::var("VALORI_HNSW_M").ok().and_then(|v| v.parse().ok());
        let hnsw_ef_construction = std::env::var("VALORI_HNSW_EF_CONSTRUCTION").ok().and_then(|v| v.parse().ok());
        let hnsw_ef_search = std::env::var("VALORI_HNSW_EF_SEARCH").ok().and_then(|v| v.parse().ok());

        // Mode
        let mode = if let Ok(url) = std::env::var("VALORI_FOLLOWER_OF") {
            NodeMode::Follower { leader_url: url }
        } else {
            NodeMode::Leader
        };
        
        let event_log_path = std::env::var("VALORI_EVENT_LOG_PATH")
            .ok().map(PathBuf::from);

        let event_log_rotation_bytes = std::env::var("VALORI_EVENT_LOG_ROTATION_BYTES")
            .ok().and_then(|v| v.parse::<u64>().ok());

        Self {
            max_records,
            dim,
            max_nodes,
            max_edges,
            bind_addr,
            index_kind,
            quantization_kind,
            snapshot_path,
            wal_path,
            event_log_path,
            event_log_rotation_bytes,
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
        }
    }
}
