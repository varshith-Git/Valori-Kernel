// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Cluster bootstrap — Phase 2.5.
//!
//! Standalone vs cluster is a boot-time decision, driven by environment:
//!
//! | Variable | Meaning | Example |
//! |---|---|---|
//! | `VALORI_CLUSTER_MEMBERS` | presence switches cluster mode ON | `1=10.0.0.1:3100/10.0.0.1:3000,2=…` |
//! | `VALORI_NODE_ID` | this node's id (must appear in members) | `1` |
//! | `VALORI_RAFT_BIND` | gRPC consensus listener | `0.0.0.0:3100` (default) |
//! | `VALORI_CLUSTER_INIT` | `1` on exactly one node of a NEW cluster | |
//!
//! Member format: comma-separated `id=raft_addr/api_addr` (api optional).
//! No `VALORI_CLUSTER_MEMBERS` → standalone, the existing path, untouched.
//!
//! [`bootstrap_cluster`] assembles the whole Phase 2 stack: log store,
//! state machine over the chained audit log, gRPC server, Raft handle —
//! returning a [`RaftCommitter`] that plugs into the `Committer` seam.

use std::collections::BTreeMap;
use std::sync::Arc;

use openraft::Config;

use valori_consensus::types::Raft;
use valori_consensus::{
    serve_raft, AuditSink, ValoriLogStore, ValoriNetworkFactory, ValoriStateMachine,
};

use crate::commit::RaftCommitter;
use valori_consensus::types::{NodeId, ValoriNode};

/// Parsed cluster topology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterConfig {
    pub node_id: NodeId,
    pub raft_bind: String,
    pub members: BTreeMap<NodeId, ValoriNode>,
    /// True on the one node that runs `Raft::initialize` for a brand-new
    /// cluster. Joining an existing cluster never initializes.
    pub init: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ClusterConfigError {
    #[error("VALORI_NODE_ID is required in cluster mode")]
    MissingNodeId,
    #[error("member entry '{0}' is not 'id=raft_addr[/api_addr]'")]
    BadMemberEntry(String),
    #[error("member id '{0}' is not a number")]
    BadMemberId(String),
    #[error("this node's id {0} does not appear in VALORI_CLUSTER_MEMBERS")]
    SelfNotInMembers(NodeId),
}

impl ClusterConfig {
    /// `None` = standalone mode (the variable is absent). Errors are config
    /// mistakes that must stop the process — a typo'd topology silently
    /// booting standalone would be a split-brain factory.
    pub fn from_env() -> Result<Option<Self>, ClusterConfigError> {
        let members = match std::env::var("VALORI_CLUSTER_MEMBERS") {
            Ok(v) if !v.trim().is_empty() => v,
            _ => return Ok(None),
        };
        let node_id = std::env::var("VALORI_NODE_ID")
            .ok()
            .and_then(|v| v.parse::<NodeId>().ok())
            .ok_or(ClusterConfigError::MissingNodeId)?;
        let raft_bind =
            std::env::var("VALORI_RAFT_BIND").unwrap_or_else(|_| "0.0.0.0:3100".to_string());
        let init = std::env::var("VALORI_CLUSTER_INIT").map(|v| v == "1").unwrap_or(false);

        Self::parse(node_id, &raft_bind, &members, init).map(Some)
    }

    /// Pure parser — testable without environment mutation.
    pub fn parse(
        node_id: NodeId,
        raft_bind: &str,
        members: &str,
        init: bool,
    ) -> Result<Self, ClusterConfigError> {
        let mut parsed = BTreeMap::new();
        for entry in members.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            let (id_str, addrs) = entry
                .split_once('=')
                .ok_or_else(|| ClusterConfigError::BadMemberEntry(entry.to_string()))?;
            let id: NodeId = id_str
                .trim()
                .parse()
                .map_err(|_| ClusterConfigError::BadMemberId(id_str.to_string()))?;
            let (raft_addr, api_addr) = match addrs.split_once('/') {
                Some((r, a)) => (r.trim().to_string(), a.trim().to_string()),
                None => (addrs.trim().to_string(), String::new()),
            };
            // Sanity: a raft address is host:port — must contain a colon and
            // no stray '=' (catches malformed entries like "1==/").
            if raft_addr.is_empty() || raft_addr.contains('=') || !raft_addr.contains(':') {
                return Err(ClusterConfigError::BadMemberEntry(entry.to_string()));
            }
            parsed.insert(id, ValoriNode { api_addr, raft_addr });
        }
        if !parsed.contains_key(&node_id) {
            return Err(ClusterConfigError::SelfNotInMembers(node_id));
        }
        Ok(Self {
            node_id,
            raft_bind: raft_bind.to_string(),
            members: parsed,
            init,
        })
    }
}

/// Everything a cluster node runs on.
pub struct ClusterHandle {
    pub raft: Raft,
    pub state_machine: ValoriStateMachine,
    /// Address the gRPC server actually bound (raft_bind may be `…:0`).
    pub raft_addr: std::net::SocketAddr,
    pub server_task: tokio::task::JoinHandle<()>,
}

impl ClusterHandle {
    /// The committer that plugs into the Engine's `Committer` seam.
    pub fn committer(&self) -> RaftCommitter {
        RaftCommitter::new(
            self.raft.clone(),
            self.state_machine.clone(),
            tokio::runtime::Handle::current(),
        )
    }
}

/// Assemble and start the Phase 2 stack for one node.
///
/// `audit` is where quorum-committed events are recorded — in production,
/// [`crate::commit::EventLogAuditSink`] over the chained `events.log`.
pub async fn bootstrap_cluster(
    cfg: &ClusterConfig,
    audit: Box<dyn AuditSink>,
) -> Result<ClusterHandle, std::io::Error> {
    let raft_config = Arc::new(
        Config {
            heartbeat_interval: 200,
            election_timeout_min: 800,
            election_timeout_max: 1600,
            ..Default::default()
        }
        .validate()
        .map_err(|e| std::io::Error::other(format!("raft config invalid: {e}")))?,
    );

    let log_store = ValoriLogStore::new();
    let state_machine = ValoriStateMachine::new(audit);

    let raft = Raft::new(
        cfg.node_id,
        raft_config,
        ValoriNetworkFactory,
        log_store,
        state_machine.clone(),
    )
    .await
    .map_err(|e| std::io::Error::other(format!("raft start failed: {e}")))?;

    let (raft_addr, server_task) = serve_raft(raft.clone(), &cfg.raft_bind).await?;

    if cfg.init {
        // Idempotent on an already-initialized cluster: openraft refuses a
        // second initialize with NotAllowed, which we treat as "fine".
        match raft.initialize(cfg.members.clone()).await {
            Ok(()) => tracing::info!("cluster initialized by this node"),
            Err(e) => tracing::warn!("initialize skipped: {e}"),
        }
    }

    Ok(ClusterHandle {
        raft,
        state_machine,
        raft_addr,
        server_task,
    })
}
