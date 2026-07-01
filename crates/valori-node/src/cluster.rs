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

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use openraft::storage::RaftLogStorage;
use openraft::{Config, SnapshotPolicy};

use valori_consensus::types::Raft;
use valori_consensus::{
    serve_raft, serve_raft_tls, AuditSink, NullAuditSink, ValoriLogStore, ValoriNetworkFactory,
    ValoriStateMachine,
};

use crate::commit::RaftCommitter;
use valori_consensus::types::{NodeId, ShardId, ValoriNode};

/// Parsed cluster topology.
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    pub node_id: NodeId,
    pub raft_bind: String,
    pub members: BTreeMap<NodeId, ValoriNode>,
    /// True on the one node that runs `Raft::initialize` for a brand-new
    /// cluster. Joining an existing cluster never initializes.
    pub init: bool,
    /// Env: VALORI_RAFT_LOG_PATH. When set, the Raft log + vote live in a
    /// redb database at this path and survive restarts (Phase 2.10).
    /// None = in-memory (tests, ephemeral deployments).
    pub raft_log_path: Option<std::path::PathBuf>,
    /// Env: VALORI_TLS_CA / VALORI_TLS_CERT / VALORI_TLS_KEY (PEM paths)
    /// + VALORI_TLS_DOMAIN (default "valori-cluster.internal").
    /// All three paths set → mutual TLS on the Raft channel (Phase 2.10b).
    /// Partially set → boot error: a half-configured TLS setup silently
    /// running plaintext would defeat the point.
    pub tls: Option<valori_consensus::RaftTlsConfig>,
    /// Env: VALORI_SHARD_COUNT (default 1). Number of independent Raft
    /// groups this process runs, all sharing the one gRPC listener at
    /// `raft_bind`. Every configured member is a voter in every shard
    /// (symmetric placement — Phase S1 does not implement namespace→shard
    /// routing; that is a later phase). `shard_count == 1` is byte-for-byte
    /// the pre-S1 single-Raft-group behavior (see `bootstrap_cluster`).
    pub shard_count: u32,
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
    #[error("TLS is partially configured ({0}) — set all of VALORI_TLS_CA, VALORI_TLS_CERT, VALORI_TLS_KEY, or none")]
    PartialTls(String),
    #[error("TLS file unreadable: {0}")]
    TlsFile(String),
    #[error("VALORI_SHARD_COUNT '{0}' is not a positive integer")]
    BadShardCount(String),
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
        let raft_log_path = std::env::var("VALORI_RAFT_LOG_PATH")
            .ok()
            .map(std::path::PathBuf::from);
        let shard_count = match std::env::var("VALORI_SHARD_COUNT") {
            Ok(v) if !v.trim().is_empty() => v
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| ClusterConfigError::BadShardCount(v.clone()))?,
            _ => 1,
        };

        let mut cfg = Self::parse(node_id, &raft_bind, &members, init)?;
        cfg.raft_log_path = raft_log_path;
        cfg.tls = Self::tls_from_env()?;
        cfg.shard_count = shard_count;
        Ok(Some(cfg))
    }

    fn tls_from_env() -> Result<Option<valori_consensus::RaftTlsConfig>, ClusterConfigError> {
        let ca = std::env::var("VALORI_TLS_CA").ok();
        let cert = std::env::var("VALORI_TLS_CERT").ok();
        let key = std::env::var("VALORI_TLS_KEY").ok();
        match (ca, cert, key) {
            (None, None, None) => Ok(None),
            (Some(ca), Some(cert), Some(key)) => {
                let read = |p: &str| {
                    std::fs::read(p)
                        .map_err(|e| ClusterConfigError::TlsFile(format!("{p}: {e}")))
                };
                Ok(Some(valori_consensus::RaftTlsConfig {
                    ca_pem: read(&ca)?,
                    cert_pem: read(&cert)?,
                    key_pem: read(&key)?,
                    domain: std::env::var("VALORI_TLS_DOMAIN")
                        .unwrap_or_else(|_| "valori-cluster.internal".to_string()),
                }))
            }
            (ca, cert, key) => {
                let mut set = Vec::new();
                if ca.is_some() { set.push("CA"); }
                if cert.is_some() { set.push("CERT"); }
                if key.is_some() { set.push("KEY"); }
                Err(ClusterConfigError::PartialTls(format!("only {} set", set.join("+"))))
            }
        }
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
            raft_log_path: None,
            tls: None,
            shard_count: 1,
        })
    }
}

/// Everything one shard's Raft group runs on (Phase S1 — multi-Raft
/// skeleton).
pub struct ShardHandle {
    pub raft: Raft,
    pub state_machine: ValoriStateMachine,
    /// The committed log index this shard knew at boot — see
    /// [`ClusterHandle::startup_committed_index`].
    pub startup_committed_index: u64,
}

/// Everything a cluster node runs on.
///
/// `raft`/`state_machine`/`startup_committed_index` are shard 0's — kept as
/// flat fields (rather than routed through `shards`) so every existing
/// caller (`main.rs`, `cluster_server.rs`, `cluster_api.rs`) keeps
/// compiling unchanged; those files only ever know about shard 0's HTTP
/// surface in this slice. `shards` holds every shard, including shard 0
/// (the same `Raft`/`ValoriStateMachine` clones as the flat fields above),
/// for the bootstrap loop's own use and for a later phase's routing layer
/// to build on.
pub struct ClusterHandle {
    pub raft: Raft,
    pub state_machine: ValoriStateMachine,
    /// Address the gRPC server actually bound (raft_bind may be `…:0`).
    pub raft_addr: std::net::SocketAddr,
    pub server_task: tokio::task::JoinHandle<()>,
    /// Background watcher tasks (state-hash poller, etc.) that must be
    /// aborted before the database file can be re-opened on restart.
    pub watcher_tasks: Vec<tokio::task::JoinHandle<()>>,
    /// The committed log index this node knew at boot. The data plane's
    /// readiness gate refuses reads until apply reaches this index, so a
    /// restarting node does not serve partial state while replaying its log.
    pub startup_committed_index: u64,
    /// Every shard this node runs, keyed by `ShardId`. Always contains at
    /// least `ShardId(0)`.
    pub shards: BTreeMap<ShardId, ShardHandle>,
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

/// Derives a per-shard path from `base`: unchanged when `shard_count == 1`
/// (byte-for-byte the pre-S1 single-file layout), otherwise
/// `{stem}-shard{N}{ext}`.
fn shard_path(base: &std::path::Path, shard: ShardId, shard_count: u32) -> std::path::PathBuf {
    if shard_count == 1 {
        return base.to_path_buf();
    }
    let stem = base
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = base
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    base.with_file_name(format!("{stem}-shard{}{ext}", shard.0))
}

/// Assemble and start the Phase 2 stack for one node — as of Phase S1, one
/// or more independent Raft groups ("shards"), all sharing the one gRPC
/// listener at `cfg.raft_bind`.
///
/// `audit` is where shard 0's quorum-committed events are recorded — in
/// production, [`crate::commit::EventLogAuditSink`] over the chained
/// `events.log`. Shards beyond 0 have no HTTP surface in this slice (no
/// namespace routing exists yet), so nothing ever writes a `KernelEvent`
/// into them; they get an internal [`NullAuditSink`] rather than forcing
/// every caller to supply N audit sinks for shards nothing writes to.
pub async fn bootstrap_cluster(
    cfg: &ClusterConfig,
    audit: Box<dyn AuditSink>,
    dim: usize,
) -> Result<ClusterHandle, std::io::Error> {
    // Snapshot cadence is an explicit, operator-tunable policy — not openraft's
    // implicit default. A snapshot is built every `snapshot_every` applied
    // entries; logs are then purged keeping `snapshot_keep` entries. The
    // cadence bounds how far a restarting node must replay before it is caught
    // up (see the startup readiness gate in cluster_server). Workloads that
    // amplify events per write (e.g. graph-heavy ingestion) should lower
    // VALORI_SNAPSHOT_EVERY_EVENTS so the catch-up window stays small.
    let snapshot_every = std::env::var("VALORI_SNAPSHOT_EVERY_EVENTS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(5000);
    let snapshot_keep = std::env::var("VALORI_RAFT_SNAPSHOT_KEEP")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1000);

    // One validated Config reused across every shard — heartbeat/election/
    // snapshot policy are process-wide operational knobs, not per-shard.
    let raft_config = Arc::new(
        Config {
            heartbeat_interval: 200,
            election_timeout_min: 800,
            election_timeout_max: 1600,
            snapshot_policy: SnapshotPolicy::LogsSinceLast(snapshot_every),
            max_in_snapshot_log_to_keep: snapshot_keep,
            ..Default::default()
        }
        .validate()
        .map_err(|e| std::io::Error::other(format!("raft config invalid: {e}")))?,
    );

    let mut audit = Some(audit);
    let mut shards: BTreeMap<ShardId, ShardHandle> = BTreeMap::new();
    let mut raft_instances: HashMap<ShardId, Raft> = HashMap::new();

    for i in 0..cfg.shard_count {
        let shard_id = ShardId(i);

        // Persistent Raft log when a path is configured (survives restarts);
        // in-memory otherwise. Both pass the same openraft compliance suite.
        //
        // When a redb path is given, the state machine shares the same database
        // handle so last_applied, membership, and the latest snapshot are
        // persisted in the sm_meta table. On restart the state machine reads them
        // back and openraft resumes from where it left off, preventing already-
        // applied entries from being replayed through the AuditSink a second time.
        let network = match &cfg.tls {
            Some(tls) => ValoriNetworkFactory::with_tls(shard_id, tls.clone()),
            None => ValoriNetworkFactory::new(shard_id),
        };

        // Only shard 0 gets the caller's audit sink — see the doc comment
        // above. Shards >= 1 have no HTTP surface in S1, so nothing ever
        // writes a KernelEvent into them.
        let audit_i: Box<dyn AuditSink> = if i == 0 {
            audit.take().expect("shard 0 runs exactly once")
        } else {
            Box::new(NullAuditSink)
        };

        let (state_machine, raft, startup_committed_index) = match &cfg.raft_log_path {
            Some(base_path) => {
                let path = shard_path(base_path, shard_id, cfg.shard_count);
                let mut store = valori_consensus::RedbLogStore::open(&path).map_err(|e| {
                    std::io::Error::other(format!("shard {i} raft log open failed: {e}"))
                })?;
                let db = store.db();
                let sm = ValoriStateMachine::with_db(audit_i, db, dim).map_err(|e| {
                    std::io::Error::other(format!("shard {i} state machine restore failed: {e}"))
                })?;
                // The committed index this node durably knew before (re)start. The
                // data plane refuses reads until apply catches back up to it, so a
                // freshly-restarted node never serves partial state during replay.
                let startup_committed_index = store
                    .read_committed()
                    .await
                    .ok()
                    .flatten()
                    .map_or(0, |l| l.index);
                let raft = Raft::new(cfg.node_id, raft_config.clone(), network, store, sm.clone())
                    .await
                    .map_err(|e| std::io::Error::other(format!("shard {i} raft start failed: {e}")))?;
                (sm, raft, startup_committed_index)
            }
            None => {
                // C-1 (consensus audit): the in-memory log store does NOT persist votes.
                // A crash-restart allows a node to vote twice in the same Raft term,
                // potentially electing two leaders (split-brain). This path is safe only
                // in tests and throwaway demo environments. Set VALORI_RAFT_LOG_PATH for
                // any deployment that must survive restarts.
                tracing::error!(
                    shard = i,
                    "VALORI_RAFT_LOG_PATH is not set — shard is running with an IN-MEMORY \
                     Raft log store. Votes are NOT persisted. A crash can cause split-brain \
                     (two leaders in the same term). Set VALORI_RAFT_LOG_PATH to a redb file path."
                );
                let sm = ValoriStateMachine::new(audit_i, dim);
                let raft = Raft::new(
                    cfg.node_id,
                    raft_config.clone(),
                    network,
                    ValoriLogStore::new(),
                    sm.clone(),
                )
                .await
                .map_err(|e| std::io::Error::other(format!("shard {i} raft start failed: {e}")))?;
                (sm, raft, 0)
            }
        };

        raft_instances.insert(shard_id, raft.clone());
        shards.insert(
            shard_id,
            ShardHandle { raft, state_machine, startup_committed_index },
        );
    }

    // ONE shared gRPC listener multiplexing every shard.
    let (raft_addr, server_task) = match &cfg.tls {
        Some(tls) => serve_raft_tls(raft_instances, &cfg.raft_bind, tls.clone()).await?,
        None => serve_raft(raft_instances, &cfg.raft_bind).await?,
    };

    if cfg.init {
        // Every shard bootstraps from the same member list (symmetric
        // placement). Idempotent on an already-initialized shard: openraft
        // refuses a second initialize with NotAllowed, treated as "fine".
        for (shard_id, handle) in &shards {
            match handle.raft.initialize(cfg.members.clone()).await {
                Ok(()) => tracing::info!(shard = shard_id.0, "shard initialized by this node"),
                Err(e) => tracing::warn!(shard = shard_id.0, "initialize skipped: {e}"),
            }
        }
    }

    // Only stamp a "shard" Prometheus label when more than one shard
    // exists — at shard_count=1 (today's default) the metrics surface
    // stays byte-for-byte what it was before Phase S1.
    let label_shards = cfg.shard_count > 1;
    let mut watcher_tasks = Vec::new();
    for (shard_id, handle) in &shards {
        spawn_raft_metrics_watcher(*shard_id, label_shards, handle.raft.clone());
        watcher_tasks.push(spawn_state_hash_watcher(
            *shard_id,
            label_shards,
            handle.raft.clone(),
            handle.state_machine.clone(),
        ));
    }

    let shard0 = shards
        .get(&ShardId(0))
        .expect("shard_count >= 1 guarantees ShardId(0) exists")
        .raft
        .clone();
    let shard0_sm = shards.get(&ShardId(0)).unwrap().state_machine.clone();
    let shard0_index = shards.get(&ShardId(0)).unwrap().startup_committed_index;

    Ok(ClusterHandle {
        raft: shard0,
        state_machine: shard0_sm,
        raft_addr,
        server_task,
        watcher_tasks,
        startup_committed_index: shard0_index,
        shards,
    })
}

/// Mirror openraft's metrics watch-channel into Prometheus gauges
/// (Phase 2.10c). The task lives as long as the Raft core: the watch
/// stream ends when the core shuts down, and the task exits with it.
///
/// Exposed on the existing /metrics endpoint:
/// - valori_raft_term, valori_raft_current_leader (0 = none),
///   valori_raft_is_leader (0/1)
/// - valori_raft_last_log_index, valori_raft_last_applied_index
///   (the gap between them is replication/apply lag)
/// - valori_raft_snapshot_index, valori_raft_purged_index
fn spawn_raft_metrics_watcher(shard: ShardId, label_shards: bool, raft: Raft) {
    tokio::spawn(async move {
        let mut rx = raft.metrics();
        let shard_label = label_shards.then(|| shard.0.to_string());
        loop {
            {
                let m = rx.borrow().clone();
                match &shard_label {
                    Some(s) => {
                        metrics::gauge!("valori_raft_term", m.current_term as f64, "shard" => s.clone());
                        metrics::gauge!(
                            "valori_raft_current_leader",
                            m.current_leader.unwrap_or(0) as f64,
                            "shard" => s.clone()
                        );
                        metrics::gauge!(
                            "valori_raft_is_leader",
                            if m.current_leader == Some(m.id) { 1.0 } else { 0.0 },
                            "shard" => s.clone()
                        );
                        metrics::gauge!(
                            "valori_raft_last_log_index",
                            m.last_log_index.unwrap_or(0) as f64,
                            "shard" => s.clone()
                        );
                        metrics::gauge!(
                            "valori_raft_last_applied_index",
                            m.last_applied.map_or(0, |l| l.index) as f64,
                            "shard" => s.clone()
                        );
                        metrics::gauge!(
                            "valori_raft_snapshot_index",
                            m.snapshot.map_or(0, |s| s.index) as f64,
                            "shard" => s.clone()
                        );
                        metrics::gauge!(
                            "valori_raft_purged_index",
                            m.purged.map_or(0, |p| p.index) as f64,
                            "shard" => s.clone()
                        );
                    }
                    None => {
                        metrics::gauge!("valori_raft_term", m.current_term as f64);
                        metrics::gauge!(
                            "valori_raft_current_leader",
                            m.current_leader.unwrap_or(0) as f64
                        );
                        metrics::gauge!(
                            "valori_raft_is_leader",
                            if m.current_leader == Some(m.id) { 1.0 } else { 0.0 }
                        );
                        metrics::gauge!(
                            "valori_raft_last_log_index",
                            m.last_log_index.unwrap_or(0) as f64
                        );
                        metrics::gauge!(
                            "valori_raft_last_applied_index",
                            m.last_applied.map_or(0, |l| l.index) as f64
                        );
                        metrics::gauge!(
                            "valori_raft_snapshot_index",
                            m.snapshot.map_or(0, |s| s.index) as f64
                        );
                        metrics::gauge!(
                            "valori_raft_purged_index",
                            m.purged.map_or(0, |p| p.index) as f64
                        );
                    }
                }
            }
            if rx.changed().await.is_err() {
                // Raft core shut down — the watch stream is closed.
                break;
            }
        }
    });
}

/// Periodically compare this node's BLAKE3 state hash against every peer's
/// `/v1/proof/state` endpoint and publish `valori_raft_state_hash_match`
/// (1 = all agree, 0 = any divergence detected).
///
/// The interval defaults to 30 s and is configurable via the env var
/// `VALORI_STATE_HASH_CHECK_SECS` (set to `0` to disable the watcher).
///
/// Phase S1: only shard 0 has an HTTP `/v1/proof/state` surface to probe —
/// no namespace routing wires shards >= 1 into the data plane yet. Shards
/// >= 1 get a one-time warning and a no-op watcher rather than inventing a
/// new (out-of-scope) cross-shard probing mechanism this slice doesn't need.
fn spawn_state_hash_watcher(
    shard: ShardId,
    label_shards: bool,
    raft: Raft,
    sm: ValoriStateMachine,
) -> tokio::task::JoinHandle<()> {
    if shard != ShardId(0) {
        tracing::warn!(
            shard = shard.0,
            "state-hash cross-check watcher disabled for this shard — no HTTP surface \
             exists for shards >= 1 in Phase S1"
        );
        return tokio::spawn(async {});
    }

    let interval_secs: u64 = std::env::var("VALORI_STATE_HASH_CHECK_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    if interval_secs == 0 {
        return tokio::spawn(async {});
    }

    tokio::spawn(async move {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("http client");
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;
            check_state_hash_agreement(shard, label_shards, &raft, &sm, &http).await;
        }
    })
}

async fn check_state_hash_agreement(
    shard: ShardId,
    label_shards: bool,
    raft: &Raft,
    sm: &ValoriStateMachine,
    http: &reqwest::Client,
) {
    let shard_label = label_shards.then(|| shard.0.to_string());
    let local_hash: String = {
        let h = sm.state_hash().await;
        h.iter().map(|b| format!("{b:02x}")).collect()
    };

    let peers: Vec<String> = {
        let m = raft.metrics().borrow().clone();
        let my_id = m.id;
        m.membership_config
            .nodes()
            .filter(|(id, n)| **id != my_id && !n.api_addr.is_empty())
            .map(|(_, n)| n.api_addr.clone())
            .collect()
    };

    if peers.is_empty() {
        // Single-node cluster — trivially agrees with itself.
        match &shard_label {
            Some(s) => metrics::gauge!("valori_raft_state_hash_match", 1.0, "shard" => s.clone()),
            None => metrics::gauge!("valori_raft_state_hash_match", 1.0),
        }
        return;
    }

    let mut all_match = true;
    for peer_addr in &peers {
        let url = format!("http://{peer_addr}/v1/proof/state");
        match http.get(&url).send().await {
            Ok(r) if r.status().is_success() => {
                match r.json::<serde_json::Value>().await {
                    Ok(body) => {
                        let peer_hash = body
                            .get("final_state_hash")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if peer_hash != local_hash {
                            tracing::error!(
                                peer = peer_addr,
                                local = %local_hash,
                                remote = peer_hash,
                                "STATE HASH MISMATCH — replica divergence detected"
                            );
                            all_match = false;
                            match &shard_label {
                                Some(s) => metrics::counter!("valori_raft_divergence_detections_total", 1, "shard" => s.clone()),
                                None => metrics::counter!("valori_raft_divergence_detections_total", 1),
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(peer = peer_addr, err = %e, "state hash parse error");
                        all_match = false;
                    }
                }
            }
            Ok(r) => {
                tracing::warn!(peer = peer_addr, status = %r.status(), "state hash probe non-2xx");
                all_match = false;
            }
            Err(e) => {
                tracing::warn!(peer = peer_addr, err = %e, "state hash probe failed");
                // Unreachable peer: we don't count this as a mismatch —
                // that would false-positive during rolling restarts.
            }
        }
    }
    match shard_label {
        Some(s) => metrics::gauge!("valori_raft_state_hash_match", if all_match { 1.0 } else { 0.0 }, "shard" => s),
        None => metrics::gauge!("valori_raft_state_hash_match", if all_match { 1.0 } else { 0.0 }),
    }
}
