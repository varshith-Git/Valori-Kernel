// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`RetryPolicy`] — E4.5.
//!
//! Only the embedder stage uses retry today. Other stages are either
//! deterministic (reader, chunker, writer) or immediately fatal (validator).

use serde::{Deserialize, Serialize};

/// Controls how many times a retryable stage is retried on failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RetryPolicy {
    /// No retries — first failure is terminal. Default.
    Never,
    /// Retry up to `attempts` times with a fixed `delay_ms` between each.
    Fixed { attempts: u32, delay_ms: u64 },
    /// Exponential back-off capped at `max_delay_ms`.
    Exponential {
        max_attempts: u32,
        base_delay_ms: u64,
        max_delay_ms: u64,
    },
}

impl Default for RetryPolicy {
    fn default() -> Self { Self::Never }
}

impl RetryPolicy {
    /// Execute `f` according to the policy. Returns the last error if all
    /// attempts fail. `E` only needs `Display` for logging; no boxing required.
    pub async fn execute<F, Fut, T, E>(&self, mut f: F) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
        E: std::fmt::Display,
    {
        match self {
            RetryPolicy::Never => f().await,

            RetryPolicy::Fixed { attempts, delay_ms } => {
                let mut last_err = None;
                for attempt in 0..=*attempts {
                    match f().await {
                        Ok(v) => return Ok(v),
                        Err(e) => {
                            if attempt < *attempts {
                                tracing::warn!("retry attempt {}/{attempts}: {e}", attempt + 1);
                                tokio::time::sleep(std::time::Duration::from_millis(*delay_ms)).await;
                            }
                            last_err = Some(e);
                        }
                    }
                }
                Err(last_err.unwrap())
            }

            RetryPolicy::Exponential { max_attempts, base_delay_ms, max_delay_ms } => {
                let mut last_err = None;
                let mut delay = *base_delay_ms;
                for attempt in 0..=*max_attempts {
                    match f().await {
                        Ok(v) => return Ok(v),
                        Err(e) => {
                            if attempt < *max_attempts {
                                tracing::warn!("retry attempt {}/{max_attempts}: {e}", attempt + 1);
                                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                                delay = (delay * 2).min(*max_delay_ms);
                            }
                            last_err = Some(e);
                        }
                    }
                }
                Err(last_err.unwrap())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn never_runs_once() {
        let count = Arc::new(AtomicU32::new(0));
        let c = count.clone();
        let result: Result<(), &str> = RetryPolicy::Never
            .execute(|| async {
                c.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
            .await;
        assert!(result.is_ok());
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn fixed_retries_on_failure() {
        let count = Arc::new(AtomicU32::new(0));
        let c = count.clone();
        let result: Result<(), &str> = RetryPolicy::Fixed { attempts: 2, delay_ms: 0 }
            .execute(|| async {
                c.fetch_add(1, Ordering::Relaxed);
                Err("boom")
            })
            .await;
        assert!(result.is_err());
        assert_eq!(count.load(Ordering::Relaxed), 3); // 1 initial + 2 retries
    }

    #[tokio::test]
    async fn fixed_succeeds_on_third_try() {
        let count = Arc::new(AtomicU32::new(0));
        let c = count.clone();
        let result: Result<u32, &str> = RetryPolicy::Fixed { attempts: 3, delay_ms: 0 }
            .execute(|| async {
                let n = c.fetch_add(1, Ordering::Relaxed);
                if n < 2 { Err("not yet") } else { Ok(n) }
            })
            .await;
        assert!(result.is_ok());
        assert_eq!(count.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn exponential_retries_capped_attempts() {
        let count = Arc::new(AtomicU32::new(0));
        let c = count.clone();
        let result: Result<(), &str> = RetryPolicy::Exponential {
            max_attempts: 2, base_delay_ms: 0, max_delay_ms: 0,
        }
        .execute(|| async {
            c.fetch_add(1, Ordering::Relaxed);
            Err("boom")
        })
        .await;
        assert!(result.is_err());
        assert_eq!(count.load(Ordering::Relaxed), 3); // 1 + 2
    }
}
