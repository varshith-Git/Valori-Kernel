// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Shard topology — configuration of the shards and Raft members for a project.
use serde::{Deserialize, Serialize};

/// The full topology of a project: how many shards, and which nodes form each
/// shard's Raft group (cluster mode) or serve it alone (standalone mode).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardTopology {
    pub shard_count: u8,
    pub shards: Vec<ShardConfig>,
}

/// Configuration for one shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardConfig {
    /// Zero-based shard index.
    pub shard_id: u8,
    /// Raft member nodes for this shard. Single-element for standalone.
    pub members: Vec<ShardMember>,
}

/// One node in a shard's Raft group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardMember {
    /// Raft node ID (1-based, matches `VALORI_NODE_ID`).
    pub node_id: u32,
    /// gRPC address for Raft consensus (e.g. `"10.0.0.1:3100"`).
    pub raft_addr: String,
    /// HTTP address for data-plane requests (e.g. `"10.0.0.1:3000"`).
    pub http_addr: String,
}

impl ShardTopology {
    /// Build a single-shard standalone topology with one member.
    pub fn standalone(http_addr: impl Into<String>) -> Self {
        ShardTopology {
            shard_count: 1,
            shards: vec![ShardConfig {
                shard_id: 0,
                members: vec![ShardMember {
                    node_id: 1,
                    raft_addr: String::new(),
                    http_addr: http_addr.into(),
                }],
            }],
        }
    }

    /// Return the shard index that owns `namespace_id`.
    pub fn shard_for_namespace(&self, namespace_id: u16) -> u8 {
        (namespace_id % self.shard_count as u16) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shard_routing() {
        let topo = ShardTopology { shard_count: 4, shards: vec![] };
        assert_eq!(topo.shard_for_namespace(0), 0);
        assert_eq!(topo.shard_for_namespace(5), 1);
        assert_eq!(topo.shard_for_namespace(7), 3);
    }
}
