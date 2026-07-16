// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Supervisor — the **operational** restart layer (D2.2).
//!
//! The runtime knows *how* to run a node; the supervisor decides *whether* a
//! crashed node should come back, per its [`RestartPolicy`]. It owns the
//! per-node operational state the runtime deliberately does not: crash state,
//! restart count, last crash reason, and backoff. This is the separation from
//! review point 3 — policy lives above the runtime, not inside it.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::policy::RestartPolicy;
use crate::runtime::RuntimeState;

const BACKOFF_BASE_SECS: u64 = 2;
const BACKOFF_MAX_SECS: u64 = 60;

/// Operational supervision state for one node.
#[derive(Debug, Clone)]
pub struct NodeSupervision {
    pub policy: RestartPolicy,
    pub state: RuntimeState,
    pub restarts: u32,
    pub last_crash: Option<String>,
    /// Do not attempt the next restart before this instant (exponential backoff).
    pub backoff_until: Option<Instant>,
}

/// Public overlay merged into `NodeInfo` for the API.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SupervisionInfo {
    pub restarts: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_crash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart_policy: Option<RestartPolicy>,
    /// Set only when the supervisor's view overrides the runtime's (e.g. Failed
    /// after a crash the runtime already dropped, or Recovering mid-restart).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<RuntimeState>,
}

/// Tracks supervision state per project name.
#[derive(Default)]
pub struct Supervisor {
    nodes: HashMap<String, NodeSupervision>,
}

impl Supervisor {
    pub fn new() -> Self {
        Self { nodes: HashMap::new() }
    }

    /// Record that a node was started (operator action) with its policy.
    pub fn on_started(&mut self, name: &str, policy: RestartPolicy) {
        self.nodes.insert(
            name.to_string(),
            NodeSupervision {
                policy,
                state: RuntimeState::Running,
                restarts: self.nodes.get(name).map(|n| n.restarts).unwrap_or(0),
                last_crash: None,
                backoff_until: None,
            },
        );
    }

    /// Record an operator-initiated stop — clears supervision (no restart).
    pub fn on_stopped(&mut self, name: &str) {
        self.nodes.remove(name);
    }

    /// Record a detected crash. Returns whether a restart should be scheduled.
    pub fn on_crash(&mut self, name: &str, reason: String) -> bool {
        let Some(node) = self.nodes.get_mut(name) else { return false };
        node.state = RuntimeState::Failed;
        node.last_crash = Some(reason);
        if node.policy.should_restart(false) {
            let delay = backoff(node.restarts);
            node.backoff_until = Some(Instant::now() + delay);
            true
        } else {
            false
        }
    }

    /// Names whose backoff has elapsed and are ready to restart now.
    pub fn due_for_restart(&self) -> Vec<String> {
        let now = Instant::now();
        self.nodes
            .iter()
            .filter(|(_, n)| {
                n.state == RuntimeState::Failed
                    && n.policy.should_restart(false)
                    && n.backoff_until.map(|t| now >= t).unwrap_or(true)
            })
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// A restart attempt succeeded.
    pub fn on_restart_success(&mut self, name: &str) {
        if let Some(node) = self.nodes.get_mut(name) {
            node.state = RuntimeState::Running;
            node.restarts += 1;
            node.backoff_until = None;
        }
    }

    /// A restart attempt failed — schedule the next with longer backoff.
    pub fn on_restart_failure(&mut self, name: &str, reason: String) {
        if let Some(node) = self.nodes.get_mut(name) {
            node.restarts += 1;
            node.state = RuntimeState::Failed;
            node.last_crash = Some(reason);
            node.backoff_until = Some(Instant::now() + backoff(node.restarts));
        }
    }

    /// Public overlay for the API, if this node is supervised.
    pub fn info(&self, name: &str) -> Option<SupervisionInfo> {
        self.nodes.get(name).map(|n| SupervisionInfo {
            restarts: n.restarts,
            last_crash: n.last_crash.clone(),
            restart_policy: Some(n.policy),
            state: match n.state {
                // Only override the runtime's view for supervisor-owned states.
                RuntimeState::Failed | RuntimeState::Recovering => Some(n.state),
                _ => None,
            },
        })
    }

    pub fn set_recovering(&mut self, name: &str) {
        if let Some(node) = self.nodes.get_mut(name) {
            node.state = RuntimeState::Recovering;
        }
    }
}

/// Capped exponential backoff: 2, 4, 8, … up to 60s.
fn backoff(restarts: u32) -> Duration {
    let secs = BACKOFF_BASE_SECS
        .saturating_mul(1u64 << restarts.min(5))
        .min(BACKOFF_MAX_SECS);
    Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_policy_does_not_restart() {
        let mut s = Supervisor::new();
        s.on_started("p", RestartPolicy::Never);
        assert!(!s.on_crash("p", "boom".into()));
        assert!(s.due_for_restart().is_empty());
    }

    #[test]
    fn always_policy_schedules_restart() {
        let mut s = Supervisor::new();
        s.on_started("p", RestartPolicy::Always);
        assert!(s.on_crash("p", "boom".into()));
        // backoff for the first restart is > 0, so not immediately due
        assert!(s.due_for_restart().is_empty());
        // force backoff elapsed
        s.nodes.get_mut("p").unwrap().backoff_until = Some(Instant::now());
        assert_eq!(s.due_for_restart(), vec!["p".to_string()]);
        s.on_restart_success("p");
        assert_eq!(s.info("p").unwrap().restarts, 1);
    }
}
