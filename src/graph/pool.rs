//! Graph Node and Edge Pools.

use crate::graph::node::GraphNode;
// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::graph::edge::GraphEdge;
use crate::types::id::{NodeId, EdgeId};
use crate::error::{Result, KernelError};

#[derive(Clone)]
pub struct NodePool {
    pub(crate) nodes: alloc::vec::Vec<Option<GraphNode>>,
}

impl NodePool {
    pub(crate) fn raw_nodes(&self) -> &[Option<GraphNode>] {
        &self.nodes
    }

    pub fn new() -> Self {
        Self {
            nodes: alloc::vec::Vec::new(),
        }
    }

    pub fn insert(&mut self, mut node: GraphNode) -> Result<NodeId> {
        let id = NodeId(self.nodes.len() as u32);
        node.id = id;
        self.nodes.push(Some(node));
        Ok(id)
    }

    pub fn get(&self, id: NodeId) -> Option<&GraphNode> {
        self.nodes.get(id.0 as usize)?.as_ref()
    }

    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut GraphNode> {
        self.nodes.get_mut(id.0 as usize)?.as_mut()
    }
    
    pub fn delete(&mut self, id: NodeId) -> Result<()> {
         let idx = id.0 as usize;
        if idx >= self.nodes.len() || self.nodes[idx].is_none() {
            return Err(KernelError::NotFound);
        }
        self.nodes[idx] = None;
        Ok(())
    }

    pub fn is_allocated(&self, id: NodeId) -> bool {
        let idx = id.0 as usize;
        idx < self.nodes.len() && self.nodes[idx].is_some()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_full(&self) -> bool {
        false
    }
}

#[derive(Clone)]
pub struct EdgePool {
    pub(crate) edges: alloc::vec::Vec<Option<GraphEdge>>,
}

impl EdgePool {
    pub(crate) fn raw_edges(&self) -> &[Option<GraphEdge>] {
        &self.edges
    }

    pub fn new() -> Self {
        Self {
            edges: alloc::vec::Vec::new(),
        }
    }

    pub fn insert(&mut self, mut edge: GraphEdge) -> Result<EdgeId> {
        let id = EdgeId(self.edges.len() as u32);
        edge.id = id;
        self.edges.push(Some(edge));
        Ok(id)
    }

    pub fn get(&self, id: EdgeId) -> Option<&GraphEdge> {
        self.edges.get(id.0 as usize)?.as_ref()
    }

    pub fn get_mut(&mut self, id: EdgeId) -> Option<&mut GraphEdge> {
        self.edges.get_mut(id.0 as usize)?.as_mut()
    }
    
    pub fn delete(&mut self, id: EdgeId) -> Result<()> {
          let idx = id.0 as usize;
        if idx >= self.edges.len() || self.edges[idx].is_none() {
            return Err(KernelError::NotFound);
        }
        self.edges[idx] = None;
        Ok(())
    }

    pub fn is_allocated(&self, id: EdgeId) -> bool {
        let idx = id.0 as usize;
        idx < self.edges.len() && self.edges[idx].is_some()
    }

    pub fn len(&self) -> usize {
        self.edges.len()
    }

    pub fn is_full(&self) -> bool {
        false
    }
}
