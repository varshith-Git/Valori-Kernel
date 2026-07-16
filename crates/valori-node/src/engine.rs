// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Re-exports `valori_engine::Engine` and provides the `EngineFromNodeConfig`
//! extension trait so existing `Engine::new(&NodeConfig)` call sites in
//! `valori-node` (server.rs, tests, main.rs, valori-ffi) keep compiling
//! without changes — they just need `use valori_node::EngineFromNodeConfig;`.

pub use valori_engine::{
    Engine, EngineHealth, ExecutionResources, PoolStats, RecoveryMode,
    EngineConfig, IndexKind, QuantizationKind,
    EngineError, CommitError, MetadataStore, Persistence,
};

use std::sync::Arc;
use crate::config::NodeConfig;

/// Extension trait that bridges `NodeConfig` → `EngineConfig` so all the
/// existing `Engine::new(&cfg)` call sites continue to work after the Engine
/// struct moved to the `valori-engine` crate.
///
/// Trait lives here (not in `valori-engine`) to avoid an orphan: both the
/// type (`Engine`) and this impl belong to different crates, so we define the
/// trait in `valori-node`, which owns `NodeConfig`.
pub trait EngineFromNodeConfig {
    fn new(cfg: &NodeConfig) -> Self;
}

impl EngineFromNodeConfig for Engine {
    fn new(cfg: &NodeConfig) -> Self {
        let vault: Arc<dyn valori_kernel::crypto::KeyVault> = {
            use crate::crypto_vault::AesGcmVault;
            use valori_kernel::crypto::KeyVault;
            let v: Arc<dyn KeyVault> = if let Some(ref p) = cfg.shred_log_path {
                match AesGcmVault::with_shred_log(p) {
                    Ok(v) => Arc::new(v),
                    Err(e) => {
                        tracing::warn!("Failed to open shred log at {:?}: {e}", p);
                        Arc::new(AesGcmVault::in_memory())
                    }
                }
            } else {
                Arc::new(AesGcmVault::in_memory())
            };
            v
        };

        let engine_cfg = EngineConfig {
            dim:                        cfg.dim,
            max_records:                cfg.max_records,
            max_nodes:                  cfg.max_nodes,
            max_edges:                  cfg.max_edges,
            index_kind:                 cfg.index_kind,
            quantization_kind:          cfg.quantization_kind,
            hnsw_m:                     cfg.hnsw_m,
            hnsw_ef_construction:       cfg.hnsw_ef_construction,
            hnsw_ef_search:             cfg.hnsw_ef_search,
            ivf_n_list:                 cfg.ivf_n_list,
            ivf_n_probe:                cfg.ivf_n_probe,
            snapshot_path:              cfg.snapshot_path.clone(),
            wal_path:                   cfg.wal_path.clone(),
            event_log_path:             cfg.event_log_path.clone(),
            event_log_rotation_bytes:   cfg.event_log_rotation_bytes,
            decay_half_life_secs:       cfg.decay_half_life_secs,
            shard_count:                cfg.shard_count,
            object_store_keep:          cfg.object_store_keep,
            object_store:               crate::object_store::ObjectStoreBackend::from_env(),
            vault,
            embed_config:               embed_config_from_node(cfg),
        };

        Engine::with_config(engine_cfg)
    }
}

pub(crate) fn embed_config_from_node(cfg: &NodeConfig) -> Option<valori_ingest::EmbedConfig> {
    let provider = cfg.embed_provider.clone()?;
    let model = cfg.embed_model.clone().unwrap_or_else(|| match provider.as_str() {
        "openai" => "text-embedding-3-small".into(),
        _        => "nomic-embed-text".into(),
    });
    let url = cfg.embed_url.clone().unwrap_or_else(|| match provider.as_str() {
        "openai" => "https://api.openai.com".into(),
        _        => "http://localhost:11434".into(),
    });
    Some(valori_ingest::EmbedConfig {
        provider,
        model,
        url: url.trim_end_matches('/').to_string(),
        api_key: cfg.embed_api_key.clone(),
    })
}
