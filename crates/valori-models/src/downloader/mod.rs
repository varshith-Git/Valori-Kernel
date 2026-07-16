// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Streaming, SHA-256-verified model downloader — M4.
//!
//! Two levels of API:
//! - Low-level: [`download_and_verify`] — single future, caller manages state.
//! - High-level: [`DownloadJob`] — tracks `DownloadState` and sends progress
//!   events over a tokio channel.
//!
//! Pause / resume via byte-range HTTP is designed here but deferred to the
//! phase that wires persistent download queues (M4-full). Today `pause()`
//! cancels the in-progress download; `resume()` re-starts from byte 0.

use std::path::Path;
use std::sync::Arc;

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, Mutex};

use crate::error::{ModelError, ModelResult};

// ── Download state machine ────────────────────────────────────────────────────

/// Lifecycle state of one download job. M4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadState {
    Queued,
    Downloading { progress_bytes: u64, total_bytes: u64 },
    /// Cancelled by the user; file partially written (will be resumed or deleted).
    Paused { progress_bytes: u64 },
    Verifying,
    Complete { sha256: String, bytes: u64 },
    Failed { reason: String },
}

impl DownloadState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, DownloadState::Complete { .. } | DownloadState::Failed { .. })
    }
}

/// Progress event sent over the channel during a [`DownloadJob`].
#[derive(Debug, Clone)]
pub enum DownloadEvent {
    Started { total_bytes: Option<u64> },
    Progress { downloaded: u64, total: Option<u64> },
    Verifying,
    Complete { sha256: String, bytes: u64 },
    Failed { reason: String },
}

// ── High-level job handle ─────────────────────────────────────────────────────

/// A cancellable download job with state tracking.
///
/// `run()` drives the download and emits [`DownloadEvent`]s on the channel.
/// The caller watches the channel for progress UI updates.
pub struct DownloadJob {
    url: String,
    expected_sha: String,
    dest: std::path::PathBuf,
    state: Arc<Mutex<DownloadState>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
}

impl DownloadJob {
    pub fn new(
        url: impl Into<String>,
        expected_sha: impl Into<String>,
        dest: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self {
            url: url.into(),
            expected_sha: expected_sha.into(),
            dest: dest.into(),
            state: Arc::new(Mutex::new(DownloadState::Queued)),
            cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Shared handle for polling current state.
    pub fn state_handle(&self) -> Arc<Mutex<DownloadState>> {
        self.state.clone()
    }

    /// Signal the download to stop after the current chunk.
    pub fn cancel(&self) {
        self.cancel.store(true, std::sync::atomic::Ordering::Release);
    }

    /// Drive the download to completion; sends events on `tx`.
    /// Returns `Ok(sha256_hex)` on success.
    pub async fn run(
        &self,
        tx: Option<mpsc::Sender<DownloadEvent>>,
    ) -> ModelResult<String> {
        macro_rules! send {
            ($event:expr) => {
                if let Some(ref s) = tx {
                    let _ = s.try_send($event);
                }
            };
        }

        let resp = reqwest::Client::new()
            .get(&self.url)
            .send()
            .await
            .map_err(|e| ModelError::Download(format!("GET {}: {e}", self.url)))?;

        if !resp.status().is_success() {
            let reason = format!("HTTP {} for {}", resp.status(), self.url);
            *self.state.lock().await = DownloadState::Failed { reason: reason.clone() };
            send!(DownloadEvent::Failed { reason });
            return Err(ModelError::Download(format!("HTTP {} for {}", resp.status(), self.url)));
        }

        let total_bytes = resp.content_length();
        send!(DownloadEvent::Started { total_bytes });
        *self.state.lock().await = DownloadState::Downloading {
            progress_bytes: 0,
            total_bytes: total_bytes.unwrap_or(0),
        };

        let mut file = tokio::fs::File::create(&self.dest)
            .await
            .map_err(|e| ModelError::Download(format!("create {}: {e}", self.dest.display())))?;

        let mut hasher = Sha256::new();
        let mut downloaded: u64 = 0;
        let mut stream = resp.bytes_stream();

        while let Some(chunk) = stream.next().await {
            if self.cancel.load(std::sync::atomic::Ordering::Acquire) {
                *self.state.lock().await = DownloadState::Paused { progress_bytes: downloaded };
                send!(DownloadEvent::Failed { reason: "cancelled".into() });
                let _ = tokio::fs::remove_file(&self.dest).await;
                return Err(ModelError::Download("download cancelled".into()));
            }

            let chunk = chunk.map_err(|e| ModelError::Download(format!("stream: {e}")))?;
            hasher.update(&chunk);
            downloaded += chunk.len() as u64;
            file.write_all(&chunk).await
                .map_err(|e| ModelError::Download(format!("write: {e}")))?;

            *self.state.lock().await = DownloadState::Downloading {
                progress_bytes: downloaded,
                total_bytes: total_bytes.unwrap_or(0),
            };
            send!(DownloadEvent::Progress { downloaded, total: total_bytes });
        }

        file.flush().await?;

        // Verify
        send!(DownloadEvent::Verifying);
        *self.state.lock().await = DownloadState::Verifying;
        let got = hex_string(&hasher.finalize());

        if !self.expected_sha.is_empty() && got != self.expected_sha {
            let _ = tokio::fs::remove_file(&self.dest).await;
            let reason = format!(
                "SHA-256 mismatch: expected {}, got {got}",
                self.expected_sha
            );
            *self.state.lock().await = DownloadState::Failed { reason: reason.clone() };
            send!(DownloadEvent::Failed { reason: reason.clone() });
            return Err(ModelError::Verify(reason));
        }

        *self.state.lock().await = DownloadState::Complete { sha256: got.clone(), bytes: downloaded };
        send!(DownloadEvent::Complete { sha256: got.clone(), bytes: downloaded });
        tracing::info!(url = %self.url, bytes = downloaded, sha256 = %got, "download complete");
        Ok(got)
    }
}

// ── Low-level helper (kept for backward compat with ModelManager) ─────────────

/// Stream `url` to `dest`, verifying SHA-256 if `expected_sha` is non-empty.
/// Returns `(bytes_written, sha256_hex)`.
pub async fn download_and_verify(
    url: &str,
    expected_sha: &str,
    dest: &Path,
) -> ModelResult<(u64, String)> {
    let job = DownloadJob::new(url, expected_sha, dest);
    let sha = job.run(None).await?;
    let bytes = match *job.state.lock().await {
        DownloadState::Complete { bytes, .. } => bytes,
        _ => 0,
    };
    Ok((bytes, sha))
}

/// SHA-256 hex of a byte slice (pure — for tests and the verifier).
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex_string(&h.finalize())
}

pub(crate) fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn download_state_terminal_check() {
        assert!(DownloadState::Complete { sha256: "x".into(), bytes: 1 }.is_terminal());
        assert!(DownloadState::Failed { reason: "err".into() }.is_terminal());
        assert!(!DownloadState::Downloading { progress_bytes: 0, total_bytes: 100 }.is_terminal());
        assert!(!DownloadState::Queued.is_terminal());
    }
}
