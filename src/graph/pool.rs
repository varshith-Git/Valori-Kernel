//! Graph Node and Edge Pools.

use crate::graph::node::GraphNode;
use crate::graph::edge::GraphEdge;
use crate::types::id::{NodeId, EdgeId};
use crate::error::{Result, KernelError};

pub struct NodePool<const CAP: usize> {
    pub(crate) nodes: [Option<GraphNode>; CAP],
}

impl<const CAP: usize> NodePool<CAP> {
    pub(crate) fn raw_nodes(&self) -> &[Option<GraphNode>] {
        &self.nodes
    }

    pub fn new() -> Self {
        Self {
            nodes: [None; CAP],
        }
    }

    pub fn insert(&mut self, mut node: GraphNode) -> Result<NodeId> {
        // Deterministic scan for first empty slot
        for (i, slot) in self.nodes.iter_mut().enumerate() {
            if slot.is_none() {
                let id = NodeId(i as u32);
                node.id = id; // Ensure ID matches index
                *slot = Some(node);
                return Ok(id);
            }
        }
        Err(KernelError::CapacityExceeded)
    }

    pub fn get(&self, id: NodeId) -> Option<&GraphNode> {
        self.nodes.get(id.0 as usize)?.as_ref()
    }

    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut GraphNode> {
        self.nodes.get_mut(id.0 as usize)?.as_mut()
    }
    
    pub fn delete(&mut self, id: NodeId) -> Result<()> {
         let idx = id.0 as usize;
        if idx >= CAP || self.nodes[idx].is_none() {
            return Err(KernelError::NotFound);
        }
        self.nodes[idx] = None;
        Ok(())
    }

    pub fn is_allocated(&self, id: NodeId) -> bool {
        let idx = id.0 as usize;
        idx < CAP && self.nodes[idx].is_some()
    }

    pub fn len(&self) -> usize {
        self.nodes.iter().filter(|s| s.is_some()).count()
    }

    pub fn is_full(&self) -> bool {
        self.len() >= CAP
    }
}

pub struct EdgePool<const CAP: usize> {
    pub(crate) edges: [Option<GraphEdge>; CAP],
}

impl<const CAP: usize> EdgePool<CAP> {
    pub(crate) fn raw_edges(&self) -> &[Option<GraphEdge>] {
        &self.edges
    }

    pub fn new() -> Self {
        Self {
            edges: [None; CAP],
        }
    }

    pub fn insert(&mut self, mut edge: GraphEdge) -> Result<EdgeId> {
        for (i, slot) in self.edges.iter_mut().enumerate() {
            if slot.is_none() {
                let id = EdgeId(i as u32);
                edge.id = id;
                *slot = Some(edge);
                return Ok(id);
            }
        }
        Err(KernelError::CapacityExceeded)
    }

    pub fn get(&self, id: EdgeId) -> Option<&GraphEdge> {
        self.edges.get(id.0 as usize)?.as_ref()
    }

    pub fn get_mut(&mut self, id: EdgeId) -> Option<&mut GraphEdge> {
        self.edges.get_mut(id.0 as usize)?.as_mut()
    }
    
    pub fn delete(&mut self, id: EdgeId) -> Result<()> {
          let idx = id.0 as usize;
        if idx >= CAP || self.edges[idx].is_none() {
            return Err(KernelError::NotFound);
        }
        self.edges[idx] = None;
        Ok(())
    }

    pub fn is_allocated(&self, id: EdgeId) -> bool {
        let idx = id.0 as usize;
        idx < CAP && self.edges[idx].is_some()
    }

    pub fn len(&self) -> usize {
        self.edges.iter().filter(|s| s.is_some()).count()
    }

    pub fn is_full(&self) -> bool {
        self.len() >= CAP
    }
}
