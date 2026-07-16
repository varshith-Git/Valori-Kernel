// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Restart policy — **operational**, not runtime.
//!
//! Whether a node *should exist* is an operator decision, so `RestartPolicy`
//! lives above the runtime (D2.1): a runtime knows how to run a node, not
//! whether it ought to be running. The health-driven restart loop (backoff,
//! retry count, crash reason) that consumes this is a later milestone.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    /// Never auto-restart (default today — the operator decides).
    #[default]
    Never,
    /// Restart only after a non-graceful exit.
    OnFailure,
    /// Always keep the node running.
    Always,
}

impl RestartPolicy {
    /// Whether a node that exited should be restarted, given whether the exit
    /// was graceful (operator-initiated stop) or a crash.
    pub fn should_restart(self, graceful: bool) -> bool {
        match self {
            RestartPolicy::Never => false,
            RestartPolicy::OnFailure => !graceful,
            RestartPolicy::Always => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_semantics() {
        assert!(!RestartPolicy::Never.should_restart(false));
        assert!(RestartPolicy::OnFailure.should_restart(false)); // crash → restart
        assert!(!RestartPolicy::OnFailure.should_restart(true)); // graceful → no
        assert!(RestartPolicy::Always.should_restart(true));
    }
}
