// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! [`CancellationToken`] — E4.3.
//!
//! Any caller (desktop, daemon, CLI) cancels an in-flight pipeline run by
//! calling `token.cancel()`. The pipeline checks between stages and returns
//! `IngestError::Cancelled` instead of continuing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::document::IngestError;

/// A lightweight, `Clone`-able cancellation signal.
///
/// All clones share the same underlying flag — cancelling any clone cancels
/// the pipeline that holds the original.
#[derive(Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Signal that the operation should stop at the next checkpoint.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// Return `Err(IngestError::Cancelled)` if cancelled; `Ok(())` otherwise.
    /// Call this at each stage boundary inside the pipeline.
    pub fn check(&self) -> Result<(), IngestError> {
        if self.is_cancelled() {
            Err(IngestError::Cancelled)
        } else {
            Ok(())
        }
    }
}

impl std::fmt::Debug for CancellationToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CancellationToken(cancelled={})", self.is_cancelled())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_not_cancelled() {
        assert!(!CancellationToken::new().is_cancelled());
    }

    #[test]
    fn cancel_sets_flag() {
        let t = CancellationToken::new();
        t.cancel();
        assert!(t.is_cancelled());
    }

    #[test]
    fn check_returns_ok_when_not_cancelled() {
        assert!(CancellationToken::new().check().is_ok());
    }

    #[test]
    fn check_returns_err_when_cancelled() {
        let t = CancellationToken::new();
        t.cancel();
        assert!(matches!(t.check().unwrap_err(), IngestError::Cancelled));
    }

    #[test]
    fn clone_shares_flag() {
        let a = CancellationToken::new();
        let b = a.clone();
        a.cancel();
        assert!(b.is_cancelled());
    }
}
