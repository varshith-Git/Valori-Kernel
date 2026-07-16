// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Event stream — `EventStore` trait + `MemoryEventStore`.
//!
//! The daemon depends on the [`EventStore`] trait, not a `Vec`. Today the only
//! implementor is [`MemoryEventStore`] (a bounded ring buffer); tomorrow a
//! `SqliteEventStore` / `RedbEventStore` / append-only log drops in with no
//! daemon change. This is the seed of "everything publishes events": managers
//! record here instead of calling each other.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;

const MAX_EVENTS: usize = 1000;

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    /// Unix seconds.
    pub time: u64,
    /// Dotted type, e.g. `project.started`, `workspace.created`.
    #[serde(rename = "type")]
    pub kind: String,
    /// The resource the event is about (project/workspace name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
}

/// Sink + reader for daemon lifecycle events. Implementors decide durability.
pub trait EventStore: Send + Sync {
    /// Record an event (`kind` is a dotted type; `resource` the subject name).
    fn record(&self, kind: &str, resource: Option<&str>);
    /// The most recent `limit` events, oldest-first.
    fn recent(&self, limit: usize) -> Vec<Event>;
}

/// Bounded in-memory ring buffer.
pub struct MemoryEventStore {
    events: Mutex<VecDeque<Event>>,
}

impl MemoryEventStore {
    pub fn new() -> Self {
        Self {
            events: Mutex::new(VecDeque::with_capacity(MAX_EVENTS)),
        }
    }
}

impl Default for MemoryEventStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EventStore for MemoryEventStore {
    fn record(&self, kind: &str, resource: Option<&str>) {
        let event = Event {
            time: now_unix(),
            kind: kind.to_string(),
            resource: resource.map(|s| s.to_string()),
        };
        let mut buf = self.events.lock().unwrap();
        if buf.len() == MAX_EVENTS {
            buf.pop_front();
        }
        buf.push_back(event);
    }

    fn recent(&self, limit: usize) -> Vec<Event> {
        let buf = self.events.lock().unwrap();
        let start = buf.len().saturating_sub(limit);
        buf.iter().skip(start).cloned().collect()
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_tails() {
        let log = MemoryEventStore::new();
        log.record("project.created", Some("healthcare"));
        log.record("project.started", Some("healthcare"));
        let recent = log.recent(10);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].kind, "project.created");
        assert_eq!(recent[1].resource.as_deref(), Some("healthcare"));
    }
}
