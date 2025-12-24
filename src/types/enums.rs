// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Knowledge Graph Enums.

use serde::{Serialize, Deserialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum NodeKind {
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

impl Default for NodeKind {
    fn default() -> Self {
        NodeKind::Record
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum EdgeKind {
    Relation = 0,
    Follows = 1,
    InEpisode = 2,
    ByAgent = 3,
    Mentions = 4,
    RefersTo = 5,
    ParentOf = 6,
    // Add more as needed
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
            _ => None,
        }
    }
}

impl Default for EdgeKind {
    fn default() -> Self {
        EdgeKind::Relation
    }
}
