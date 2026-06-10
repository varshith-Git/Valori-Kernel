// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Leader HTTP client with exponential-backoff retry.
//!
//! All three RPCs (`get_proof`, `stream_events`, `download_snapshot`) retry
//! transient network errors using truncated binary exponential backoff:
//!   attempt 0 → immediate
//!   attempt 1 → 500 ms
//!   attempt 2 → 1 s
//!   attempt 3 → 2 s  (capped at MAX_BACKOFF_MS)
//!
//! `stream_events` is a streaming endpoint; it does not retry mid-stream.

use crate::errors::EngineError;
use reqwest::Client;
use std::time::Duration;
use tokio::time::sleep;

/// Maximum number of retry attempts for non-streaming RPCs.
const MAX_RETRIES: u32 = 4;
/// Initial backoff for the first retry (ms). Doubles each attempt, capped below.
const INITIAL_BACKOFF_MS: u64 = 500;
/// Hard ceiling on backoff duration (ms).
const MAX_BACKOFF_MS: u64 = 8_000;

#[derive(Debug, Clone)]
pub struct LeaderClient {
    base_url: String,
    client: Client,
}

impl LeaderClient {
    pub fn new(url: String) -> Self {
        Self {
            base_url: url.trim_end_matches('/').to_string(),
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("failed to build reqwest client"),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Compute backoff for attempt N (0-indexed).  Returns 0 for the first
    /// attempt so we fail fast on non-transient errors.
    fn backoff_ms(attempt: u32) -> u64 {
        if attempt == 0 {
            return 0;
        }
        let ms = INITIAL_BACKOFF_MS * (1u64 << (attempt - 1).min(6));
        ms.min(MAX_BACKOFF_MS)
    }

    /// Fetch the leader's current deterministic proof, retrying on transient errors.
    pub async fn get_proof(&self) -> Result<valori_kernel::proof::DeterministicProof, EngineError> {
        let url = format!("{}/v1/proof/state", self.base_url);
        let mut last_err = EngineError::Network("unreachable".into());

        for attempt in 0..MAX_RETRIES {
            let delay = Self::backoff_ms(attempt);
            if delay > 0 {
                tracing::debug!("get_proof: retry {} after {}ms", attempt, delay);
                sleep(Duration::from_millis(delay)).await;
            }

            match self.client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    return resp.json().await
                        .map_err(|e| EngineError::Network(e.to_string()));
                }
                Ok(resp) => {
                    // Non-transient HTTP error (4xx) — don't retry.
                    let status = resp.status();
                    if status.is_client_error() {
                        return Err(EngineError::Network(
                            format!("Proof request failed: {}", status)
                        ));
                    }
                    last_err = EngineError::Network(format!("Proof request failed: {}", status));
                }
                Err(e) => {
                    last_err = EngineError::Network(e.to_string());
                }
            }
        }

        Err(last_err)
    }

    /// Open a streaming connection to the leader's event log.
    /// Not retried — callers should handle reconnection at a higher level
    /// (the `run_follower_loop` outer loop handles that).
    pub async fn stream_events(&self, start_offset: u64) -> Result<reqwest::Response, EngineError> {
        let url = format!(
            "{}/v1/replication/events?start_offset={}",
            self.base_url, start_offset
        );
        let resp = self.client.get(&url).send().await
            .map_err(|e| EngineError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(EngineError::Network(
                format!("Stream request failed: {}", resp.status())
            ));
        }

        Ok(resp)
    }

    /// Download the full leader snapshot, retrying on transient errors.
    pub async fn download_snapshot(&self) -> Result<Vec<u8>, EngineError> {
        let url = format!("{}/v1/snapshot/download", self.base_url);
        let mut last_err = EngineError::Network("unreachable".into());

        for attempt in 0..MAX_RETRIES {
            let delay = Self::backoff_ms(attempt);
            if delay > 0 {
                tracing::debug!("download_snapshot: retry {} after {}ms", attempt, delay);
                sleep(Duration::from_millis(delay)).await;
            }

            match self.client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    return resp.bytes().await
                        .map(|b| b.to_vec())
                        .map_err(|e| EngineError::Network(e.to_string()));
                }
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_client_error() {
                        return Err(EngineError::Network(
                            format!("Snapshot request failed: {}", status)
                        ));
                    }
                    last_err = EngineError::Network(format!("Snapshot request failed: {}", status));
                }
                Err(e) => {
                    last_err = EngineError::Network(e.to_string());
                }
            }
        }

        Err(last_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_schedule_is_correct() {
        assert_eq!(LeaderClient::backoff_ms(0), 0);        // immediate
        assert_eq!(LeaderClient::backoff_ms(1), 500);      // 500 ms
        assert_eq!(LeaderClient::backoff_ms(2), 1_000);    // 1 s
        assert_eq!(LeaderClient::backoff_ms(3), 2_000);    // 2 s
        assert_eq!(LeaderClient::backoff_ms(4), 4_000);    // 4 s
        assert_eq!(LeaderClient::backoff_ms(10), MAX_BACKOFF_MS); // capped
    }
}
