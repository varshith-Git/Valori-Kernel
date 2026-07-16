// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Project — the top-level unit of isolation in Valori.
//!
//! Each project maps to exactly one valori-node process (or one cluster of nodes),
//! its own WAL/snapshot directory, and an independent KernelState.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The index algorithm a project's node was started with.
/// Immutable after the first insert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum IndexKind {
    #[default]
    Brute,
    Hnsw,
    Ivf,
    Bq,
    Auto,
}

impl std::fmt::Display for IndexKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexKind::Brute => write!(f, "brute"),
            IndexKind::Hnsw => write!(f, "hnsw"),
            IndexKind::Ivf => write!(f, "ivf"),
            IndexKind::Bq => write!(f, "bq"),
            IndexKind::Auto => write!(f, "auto"),
        }
    }
}

impl std::str::FromStr for IndexKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "brute" | "bruteforce" => Ok(IndexKind::Brute),
            "hnsw" => Ok(IndexKind::Hnsw),
            "ivf" => Ok(IndexKind::Ivf),
            "bq" => Ok(IndexKind::Bq),
            "auto" | "mstg" => Ok(IndexKind::Auto),
            other => Err(format!("Unknown index kind: {}", other)),
        }
    }
}

/// Whether the project is a single-node standalone or a Raft cluster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProjectMode {
    #[default]
    Standalone,
    Cluster,
}

/// A persistent project record stored in the MetadataDb.
///
/// One project = one isolated valori-node process (standalone) or one cluster of
/// nodes (cluster). Projects are listed on the Home page and auto-resumed on open.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// Unique project name. Used as the primary key and as the directory name.
    pub name: String,
    /// Absolute path to the project's data directory (`~/.valori/projects/<name>/`).
    pub dir: PathBuf,
    /// HTTP port the node listens on. Allocated by the process manager at creation.
    pub port: u16,
    /// Vector dimension. Immutable after the first insert.
    pub dim: u16,
    /// Index algorithm. Immutable after the first insert.
    pub index: IndexKind,
    /// Number of shards. Immutable after creation.
    pub shard_count: u8,
    /// Number of Raft replica nodes (cluster mode only). 1 = standalone.
    pub node_count: u8,
    /// Whether this is a standalone or cluster project.
    pub mode: ProjectMode,
    /// Unix seconds when the project was created.
    pub created_at: u64,
    /// Unix seconds when the project was last opened. `None` = never opened.
    pub last_opened_at: Option<u64>,
    /// Approximate record count at last close. `None` = unknown.
    pub record_count: Option<u64>,
    /// For cluster projects: per-node configuration (node_id, raft_addr, http_port).
    #[serde(default)]
    pub nodes: Vec<ClusterNodeConfig>,
}

/// Configuration for one node in a cluster project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterNodeConfig {
    pub node_id: u32,
    pub http_port: u16,
    pub raft_port: u16,
}

impl Project {
    /// Returns `true` if the project data directory exists on disk.
    pub fn exists_on_disk(&self) -> bool {
        self.dir.exists()
    }

    /// Returns the path to the event log for the given shard.
    /// For shard 0 on a single-shard project, this is `events.log`.
    pub fn event_log_path(&self, shard_id: u8) -> PathBuf {
        if self.shard_count <= 1 {
            self.dir.join("events.log")
        } else {
            self.dir.join(format!("events-shard{}.log", shard_id))
        }
    }

    /// Returns the path to the snapshot file.
    pub fn snapshot_path(&self) -> PathBuf {
        self.dir.join("current.snap")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_kind_roundtrip() {
        for (s, expected) in [
            ("brute", IndexKind::Brute),
            ("hnsw", IndexKind::Hnsw),
            ("auto", IndexKind::Auto),
        ] {
            assert_eq!(s.parse::<IndexKind>().unwrap(), expected);
            assert_eq!(expected.to_string(), s);
        }
    }

    #[test]
    fn project_paths() {
        let p = Project {
            name: "test".into(),
            dir: PathBuf::from("/home/user/.valori/projects/test"),
            port: 3010,
            dim: 768,
            index: IndexKind::Brute,
            shard_count: 2,
            node_count: 1,
            mode: ProjectMode::Standalone,
            created_at: 0,
            last_opened_at: None,
            record_count: None,
            nodes: vec![],
        };
        assert_eq!(
            p.event_log_path(0),
            PathBuf::from("/home/user/.valori/projects/test/events-shard0.log")
        );
        assert_eq!(
            p.snapshot_path(),
            PathBuf::from("/home/user/.valori/projects/test/current.snap")
        );
    }
}
