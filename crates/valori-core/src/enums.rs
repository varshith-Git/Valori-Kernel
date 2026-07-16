// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Domain enums shared across all Valori crates.

use serde::{Deserialize, Serialize};

// ── Graph node kinds ──────────────────────────────────────────────────────────

/// Semantic kind of a knowledge-graph node.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[repr(u8)]
pub enum NodeKind {
    #[default]
    Record = 0,
    Concept = 1,
    Agent = 2,
    User = 3,
    Tool = 4,
    Document = 5,
    Chunk = 6,
}

impl NodeKind {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(NodeKind::Record),
            1 => Some(NodeKind::Concept),
            2 => Some(NodeKind::Agent),
            3 => Some(NodeKind::User),
            4 => Some(NodeKind::Tool),
            5 => Some(NodeKind::Document),
            6 => Some(NodeKind::Chunk),
            _ => None,
        }
    }
}

// ── Graph edge kinds ──────────────────────────────────────────────────────────

/// Semantic kind of a directed knowledge-graph edge.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[repr(u8)]
pub enum EdgeKind {
    #[default]
    Relation = 0,
    Follows = 1,
    InEpisode = 2,
    ByAgent = 3,
    Mentions = 4,
    RefersTo = 5,
    ParentOf = 6,
    /// A record supersedes an older one (consolidation — Phase C4.2).
    Supersedes = 7,
    /// A record contradicts an older one (NLI verdict — Phase C4.3).
    Contradicts = 8,
}

impl EdgeKind {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(EdgeKind::Relation),
            1 => Some(EdgeKind::Follows),
            2 => Some(EdgeKind::InEpisode),
            3 => Some(EdgeKind::ByAgent),
            4 => Some(EdgeKind::Mentions),
            5 => Some(EdgeKind::RefersTo),
            6 => Some(EdgeKind::ParentOf),
            7 => Some(EdgeKind::Supersedes),
            8 => Some(EdgeKind::Contradicts),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_kind_roundtrip() {
        for v in 0u8..=6 {
            assert!(NodeKind::from_u8(v).is_some(), "missing NodeKind for {v}");
        }
        assert!(NodeKind::from_u8(7).is_none());
    }

    #[test]
    fn edge_kind_roundtrip() {
        for v in 0u8..=8 {
            assert!(EdgeKind::from_u8(v).is_some(), "missing EdgeKind for {v}");
        }
        assert!(EdgeKind::from_u8(9).is_none());
    }
}
