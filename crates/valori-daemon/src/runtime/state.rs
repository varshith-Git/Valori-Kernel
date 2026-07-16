// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `RuntimeState` — a node's lifecycle as an explicit state machine.
//!
//! Replaces stringly-typed status with legal transitions. Illegal moves (e.g.
//! `Running → Starting`) return an error instead of silently corrupting state.

use serde::Serialize;

use crate::error::{DaemonError, DaemonResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeState {
    Stopped,
    Starting,
    Running,
    Stopping,
    /// Exited unexpectedly (crash), not (yet) restarted.
    Failed,
    /// Auto-restarting after an unexpected exit (distinct from a fresh
    /// `Starting` — Valori replays its event log on recovery).
    Recovering,
}

impl RuntimeState {
    /// Legal transitions:
    /// ```text
    /// Stopped    → Starting
    /// Starting   → Running | Failed | Stopped
    /// Running    → Stopping | Failed
    /// Stopping   → Stopped | Failed
    /// Failed     → Recovering | Starting | Stopped
    /// Recovering → Running | Failed
    /// ```
    pub fn can_transition_to(self, next: RuntimeState) -> bool {
        use RuntimeState::*;
        matches!(
            (self, next),
            (Stopped, Starting)
                | (Starting, Running)
                | (Starting, Failed)
                | (Starting, Stopped)
                | (Running, Stopping)
                | (Running, Failed)
                | (Stopping, Stopped)
                | (Stopping, Failed)
                | (Failed, Recovering)
                | (Failed, Starting)
                | (Failed, Stopped)
                | (Recovering, Running)
                | (Recovering, Failed)
        )
    }

    /// Attempt a transition, erroring on an illegal one.
    pub fn transition(&mut self, next: RuntimeState) -> DaemonResult<()> {
        if self.can_transition_to(next) {
            *self = next;
            Ok(())
        } else {
            Err(DaemonError::InvalidState { from: *self, to: next })
        }
    }

    pub fn is_running(self) -> bool {
        matches!(self, RuntimeState::Running)
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeState::*;

    #[test]
    fn legal_and_illegal_transitions() {
        let mut s = Stopped;
        assert!(s.transition(Starting).is_ok());
        assert!(s.transition(Running).is_ok());
        // Running → Starting is illegal.
        assert!(s.transition(Starting).is_err());
        assert!(s.transition(Stopping).is_ok());
        assert!(s.transition(Stopped).is_ok());
    }
}
