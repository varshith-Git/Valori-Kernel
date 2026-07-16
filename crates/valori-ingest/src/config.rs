// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`PipelineConfig`] — E4.6.
//!
//! One config object controls all tuning knobs. Callers override only what
//! they care about; all other fields use sensible defaults.

use serde::{Deserialize, Serialize};

use crate::retry::RetryPolicy;

/// Runtime configuration for [`crate::pipeline::IngestPipeline`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// How many chunks are embedded and written per batch. E4.7 streaming.
    ///
    /// `1` = one chunk at a time (lowest memory, highest request count).
    /// `usize::MAX` (default) = all chunks in one batch (original behavior).
    pub batch_size: usize,

    /// Retry policy applied to the embedder stage only. E4.5.
    pub retry: RetryPolicy,

    /// Wall-clock timeout for the entire pipeline run, seconds.
    /// `None` = no timeout (default).
    pub timeout_secs: Option<u64>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            batch_size: usize::MAX,
            retry: RetryPolicy::Never,
            timeout_secs: None,
        }
    }
}

impl PipelineConfig {
    pub fn with_batch_size(mut self, n: usize) -> Self { self.batch_size = n.max(1); self }
    pub fn with_retry(mut self, r: RetryPolicy) -> Self { self.retry = r; self }
    pub fn with_timeout_secs(mut self, secs: u64) -> Self { self.timeout_secs = Some(secs); self }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_passthrough() {
        let c = PipelineConfig::default();
        assert_eq!(c.batch_size, usize::MAX);
        assert!(matches!(c.retry, RetryPolicy::Never));
        assert!(c.timeout_secs.is_none());
    }

    #[test]
    fn builder_methods() {
        let c = PipelineConfig::default()
            .with_batch_size(32)
            .with_retry(RetryPolicy::Fixed { attempts: 3, delay_ms: 100 })
            .with_timeout_secs(60);
        assert_eq!(c.batch_size, 32);
        assert!(matches!(c.retry, RetryPolicy::Fixed { .. }));
        assert_eq!(c.timeout_secs, Some(60));
    }

    #[test]
    fn batch_size_zero_clamped_to_one() {
        let c = PipelineConfig::default().with_batch_size(0);
        assert_eq!(c.batch_size, 1);
    }
}
