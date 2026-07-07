// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Snapshot manifest — tracks which files make up the current durable state.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The manifest records everything needed to bootstrap `KernelState` on restart:
/// the current snapshot (optional fast-path) and the event log segments that
/// follow it (canonical truth).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateManifest {
    /// Path to the current snapshot file, if any.
    pub snapshot_path: Option<PathBuf>,
    /// Ordered list of event log segment paths. Segments are replayed in order
    /// after the snapshot is loaded. Empty when only the snapshot exists.
    pub event_log_segments: Vec<PathBuf>,
    /// Number of `KernelEvent`s applied so far (across all segments).
    pub last_applied_height: u64,
    /// BLAKE3 hash of `KernelState` at `last_applied_height`. Used to verify
    /// that replay produced the expected state.
    pub state_hash: [u8; 32],
}

impl StateManifest {
    pub fn empty() -> Self {
        StateManifest {
            snapshot_path: None,
            event_log_segments: Vec::new(),
            last_applied_height: 0,
            state_hash: [0u8; 32],
        }
    }

    /// Write the manifest as JSON to `path`. Creates the file if absent.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }

    /// Load the manifest from `path`. Returns `StateManifest::empty()` if the
    /// file does not exist, so callers can treat a missing manifest as a fresh start.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        if !path.exists() {
            return Ok(Self::empty());
        }
        let bytes = std::fs::read(path)?;
        serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}
