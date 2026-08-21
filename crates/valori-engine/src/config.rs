// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Engine-layer configuration types.
//!
//! [`IndexKind`] and [`QuantizationKind`] are engine-owned enums — they
//! describe how the engine should behave, not how the HTTP layer routes
//! requests.  `valori-node`'s `NodeConfig` re-exports them from here.
//!
//! [`EngineConfig`] is the engine's injection point for everything that
//! `valori-node` constructs from environment variables:
//! - The AES-256-GCM vault ([`valori_kernel::crypto::KeyVault`] trait object)
//!   is constructed by `valori-node` and injected — the engine never imports
//!   the concrete `AesGcmVault` type (Dependency Inversion Principle).
//! - The object-store backend is similarly injected.
//! - The embed config is injected from `NodeConfig`'s embed env vars.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

/// Which vector index algorithm the engine should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexKind {
    BruteForce,
    Hnsw,
    Ivf,
    Bq,
    /// Automatically selects the tier based on live record count:
    /// < 10 000 → BruteForce, 10 000–2 000 000 → BQ, > 2 000 000 → HNSW.
    Auto,
}

impl IndexKind {
    /// Wire tag carried opaquely inside `KernelEvent::ConfigureNamespace` —
    /// the kernel does not interpret it, only `Engine` does (via
    /// `Engine::index_kind_from_wire`). Append-only, never renumber.
    pub const fn as_u8(self) -> u8 {
        match self {
            IndexKind::BruteForce => 0,
            IndexKind::Hnsw => 1,
            IndexKind::Ivf => 2,
            IndexKind::Bq => 3,
            IndexKind::Auto => 4,
        }
    }

    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(IndexKind::BruteForce),
            1 => Some(IndexKind::Hnsw),
            2 => Some(IndexKind::Ivf),
            3 => Some(IndexKind::Bq),
            4 => Some(IndexKind::Auto),
            _ => None,
        }
    }

    /// Convert from the canonical `valori_domain::IndexKind` used at the API
    /// boundary. A single, explicit adapter at this one crossing — not a
    /// second copy of the enum — per `valori_domain::IndexKind`'s own doc
    /// comment declaring it the target canonical type.
    pub const fn from_domain(k: valori_domain::IndexKind) -> Self {
        match k {
            valori_domain::IndexKind::Brute => IndexKind::BruteForce,
            valori_domain::IndexKind::Hnsw => IndexKind::Hnsw,
            valori_domain::IndexKind::Ivf => IndexKind::Ivf,
            valori_domain::IndexKind::Bq => IndexKind::Bq,
            valori_domain::IndexKind::Auto => IndexKind::Auto,
        }
    }
}

/// Which quantization scheme to apply to stored vectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuantizationKind {
    None,
    Scalar,
    Product,
}

/// All configuration the [`super::Engine`] needs at construction time.
///
/// `valori-node` builds this from its `NodeConfig` (env vars) and injects
/// the vault and object store as trait objects so the engine crate has no
/// dependency on `valori-node`.
pub struct EngineConfig {
    // ── Capacity ─────────────────────────────────────────────────────────────
    pub max_records: usize,
    pub max_nodes: usize,
    pub max_edges: usize,

    // ── Quantization ─────────────────────────────────────────────────────────
    pub quantization_kind: QuantizationKind,

    // ── HNSW tuning ───────────────────────────────────────────────────────────
    pub hnsw_m: Option<usize>,
    pub hnsw_ef_construction: Option<usize>,
    pub hnsw_ef_search: Option<usize>,

    // ── IVF tuning ────────────────────────────────────────────────────────────
    pub ivf_n_list: Option<usize>,
    pub ivf_n_probe: Option<usize>,

    // ── BQ tuning (S11.3) ────────────────────────────────────────────────────
    pub bq_pool_factor: Option<usize>,
    pub bq_min_candidates: Option<usize>,

    // ── Persistence paths ─────────────────────────────────────────────────────
    pub snapshot_path: Option<PathBuf>,
    pub wal_path: Option<PathBuf>,
    pub event_log_path: Option<PathBuf>,
    pub event_log_rotation_bytes: Option<u64>,

    // ── Feature knobs ─────────────────────────────────────────────────────────
    pub decay_half_life_secs: Option<u64>,
    pub shard_count: usize,

    // ── Object store ──────────────────────────────────────────────────────────
    pub object_store_keep: u32,
    /// Injected by `valori-node` — engine never constructs the backend itself.
    pub object_store: Option<Arc<valori_storage::object_store::ObjectStoreBackend>>,

    // ── Injected dependencies (DIP) ───────────────────────────────────────────
    /// AES-256-GCM vault for crypto-shredding. Constructed by `valori-node`
    /// from `VALORI_SHRED_LOG_PATH`; injected here so the engine crate has no
    /// AesGcmVault import.
    pub vault: Arc<dyn valori_kernel::crypto::KeyVault>,

    /// Embedding provider config (Ollama / OpenAI / custom).
    /// `None` when `VALORI_EMBED_PROVIDER` is not set.
    pub embed_config: Option<valori_ingest::EmbedConfig>,
}
