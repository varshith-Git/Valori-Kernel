// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use std::net::SocketAddr;
use std::path::PathBuf;
// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
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
    pub auto_snapshot_interval_secs: Option<u64>,
    
    // Security
    pub auth_token: Option<String>,
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
        
        let snapshot_path = std::env::var("VALORI_SNAPSHOT_PATH")
            .ok().map(PathBuf::from);
            
        let auto_snapshot_interval_secs = std::env::var("VALORI_SNAPSHOT_INTERVAL")
            .ok().and_then(|v| v.parse().ok());
            
        let auth_token = std::env::var("VALORI_AUTH_TOKEN").ok();

        Self {
            max_records,
            dim,
            max_nodes,
            max_edges,
            bind_addr,
            index_kind,
            quantization_kind,
            snapshot_path,
            auto_snapshot_interval_secs,
            auth_token,
        }
    }
}
