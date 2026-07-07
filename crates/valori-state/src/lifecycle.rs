// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! State lifecycle — tracks the operational phase of KernelState within a node.

/// The lifecycle phase of `KernelState` inside a running node.
///
/// Transitions:
/// ```text
/// (start) ──► Recovering ──► Ready ──► Snapshotting ──► Ready
///                                          └──► (shutdown)
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateLifecycle {
    /// The node is replaying the event log or loading a snapshot. Reads and
    /// writes are not yet accepted on the HTTP API.
    Recovering,
    /// `KernelState` is fully loaded and the node is accepting requests.
    Ready,
    /// A snapshot is being serialized to disk. Writes continue to be accepted;
    /// the snapshot captures the state at the moment snapshotting began.
    Snapshotting,
}

impl StateLifecycle {
    pub fn is_ready(&self) -> bool {
        matches!(self, StateLifecycle::Ready)
    }

    pub fn is_recovering(&self) -> bool {
        matches!(self, StateLifecycle::Recovering)
    }
}

impl std::fmt::Display for StateLifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateLifecycle::Recovering => write!(f, "recovering"),
            StateLifecycle::Ready => write!(f, "ready"),
            StateLifecycle::Snapshotting => write!(f, "snapshotting"),
        }
    }
}
