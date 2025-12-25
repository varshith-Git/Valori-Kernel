// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Kernel State definition.

use crate::types::id::Version;
use crate::storage::pool::RecordPool;
use crate::graph::pool::{NodePool, EdgePool};
use crate::index::brute_force::BruteForceIndex;
use crate::index::{SearchResult, VectorIndex};
use crate::state::command::Command;
use crate::error::{Result, KernelError};
use crate::graph::node::GraphNode;
use crate::graph::adjacency::{add_edge, OutEdgeIterator};
use crate::types::id::{RecordId, NodeId, EdgeId, EdgeId as GraphEdgeId};
use crate::types::vector::FxpVector;
use crate::storage::record::Record;

pub struct KernelState<const MAX_RECORDS: usize, const D: usize, const MAX_NODES: usize, const MAX_EDGES: usize> {
    pub(crate) version: Version,
    pub(crate) records: RecordPool<MAX_RECORDS, D>,
    pub(crate) nodes: NodePool<MAX_NODES>,
    pub(crate) edges: EdgePool<MAX_EDGES>,
    pub(crate) index: BruteForceIndex,
}

impl<const MAX_RECORDS: usize, const D: usize, const MAX_NODES: usize, const MAX_EDGES: usize> KernelState<MAX_RECORDS, D, MAX_NODES, MAX_EDGES> {
    pub fn new() -> Self {
        Self {
            version: Version(0),
            records: RecordPool::new(),
            nodes: NodePool::new(),
            edges: EdgePool::new(),
            index: BruteForceIndex::default(),
        }
    }

    // --- Read APIs ---

    pub fn version(&self) -> u64 {
        self.version.0
    }

    pub fn get_record(&self, id: RecordId) -> Option<&Record<D>> {
        self.records.get(id)
    }

    pub fn get_node(&self, id: NodeId) -> Option<&GraphNode> {
        self.nodes.get(id)
    }

    pub fn outgoing_edges<'a>(&'a self, node_id: NodeId) -> Option<OutEdgeIterator<'a, MAX_EDGES>> {
        self.nodes.get(node_id).map(|node| OutEdgeIterator::new(&self.edges, node.first_out_edge))
    }

    pub fn is_edge_active(&self, id: EdgeId) -> bool {
        self.edges.get(id).is_some()
    }

    pub fn search_l2(&self, query: &FxpVector<D>, results: &mut [SearchResult]) -> usize {
        self.index.search(&self.records, query, results)
    }

    // --- Write Logic ---

    pub fn apply(&mut self, cmd: &Command<D>) -> Result<()> {
        match cmd {
            Command::InsertRecord { id, vector } => {
                let allocated_id = self.records.insert(*vector)?;
                if allocated_id != *id {
                     return Err(KernelError::InvalidOperation);
                }
                <BruteForceIndex as VectorIndex<MAX_RECORDS, D>>::on_insert(&mut self.index, allocated_id, vector);
            }
            Command::DeleteRecord { id } => {
                self.records.delete(*id)?;
                <BruteForceIndex as VectorIndex<MAX_RECORDS, D>>::on_delete(&mut self.index, *id);
            }
            Command::CreateNode { node_id, kind, record } => {
                if let Some(rid) = record {
                    if self.records.get(*rid).is_none() {
                        return Err(KernelError::NotFound);
                    }
                }
                let node = GraphNode::new(*node_id, *kind, *record);
                let allocated = self.nodes.insert(node)?;
                if allocated != *node_id {
                    return Err(KernelError::InvalidOperation);
                }
            }
            Command::CreateEdge { edge_id, kind, from, to } => {
                let allocated = add_edge(&mut self.nodes, &mut self.edges, *kind, *from, *to)?;
                if allocated != *edge_id {
                    return Err(KernelError::InvalidOperation);
                }
            }
            Command::DeleteNode { node_id } => {
                self._delete_node(*node_id)?;
            }
            Command::DeleteEdge { edge_id } => {
                self._delete_edge(*edge_id)?;
            }
        }

        self.version = self.version.next();
        Ok(())
    }
    
    fn _delete_node(&mut self, node_id: NodeId) -> Result<()> {
        if self.nodes.get(node_id).is_none() {
            return Err(KernelError::NotFound);
        }

        // Cascading delete: Remove all edges involving this node.
        loop {
            let mut edge_to_remove: Option<EdgeId> = None;
            // Scan all edges to find one that involves this node.
            // Note: inefficient O(E) scan per edge, but robust for no_std without reverse index.
            for edge in self.edges.edges.iter().flatten() {
                if edge.from == node_id || edge.to == node_id {
                    edge_to_remove = Some(edge.id);
                    break;
                }
            }
            
            if let Some(eid) = edge_to_remove {
                // _delete_edge handles unlinking from adjacency lists
                self._delete_edge(eid)?;
            } else {
                break; 
            }
        }

        self.nodes.delete(node_id)?;
        Ok(())
    }

    fn _delete_edge(&mut self, edge_id: EdgeId) -> Result<()> {
        let edge = self.edges.get(edge_id).ok_or(KernelError::NotFound)?;
        let from_node_id = edge.from;
        
        let mut prev_id: Option<GraphEdgeId> = None;
        
        if let Some(node) = self.nodes.get(from_node_id) {
            let mut curr_id = node.first_out_edge;
            
            while let Some(c) = curr_id {
                if c == edge_id {
                    // Found it. Unlink.
                    let next_id = self.edges.get(c).unwrap().next_out;
                    
                    if let Some(p) = prev_id {
                        // Interior
                        self.edges.get_mut(p).unwrap().next_out = next_id;
                    } else {
                        // Head
                        self.nodes.get_mut(from_node_id).unwrap().first_out_edge = next_id;
                    }
                    break;
                }
                prev_id = Some(c);
                if let Some(e) = self.edges.get(c) {
                    curr_id = e.next_out;
                } else {
                    break;
                }
            }
        }
        
        self.edges.delete(edge_id)?;
        Ok(())
    }

    // --- Invariant Checker ---

    /// Checks the internal consistency of the kernel state.
    pub fn check_invariants(&self) -> Result<()> {
        // 1. Check Nodes
        for (i, slot) in self.nodes.raw_nodes().iter().enumerate() {
            if let Some(node) = slot {
                if node.id.0 as usize != i {
                    return Err(KernelError::InvalidOperation); 
                }
                
                if let Some(rid) = node.record {
                    if self.records.get(rid).is_none() {
                        return Err(KernelError::NotFound); 
                    }
                }

                if let Some(eid) = node.first_out_edge {
                    if self.edges.get(eid).is_none() {
                        return Err(KernelError::NotFound); 
                    }
                    let edge = self.edges.get(eid).unwrap();
                    if edge.from != node.id {
                        return Err(KernelError::InvalidOperation); 
                    }
                }
            }
        }

        // 2. Check Edges
        for (i, slot) in self.edges.raw_edges().iter().enumerate() {
            if let Some(edge) = slot {
                if edge.id.0 as usize != i {
                    return Err(KernelError::InvalidOperation);
                }

                if self.nodes.get(edge.from).is_none() || self.nodes.get(edge.to).is_none() {
                    return Err(KernelError::NotFound); 
                }

                if let Some(next_id) = edge.next_out {
                     if self.edges.get(next_id).is_none() {
                         return Err(KernelError::NotFound); 
                     }
                     let next_edge = self.edges.get(next_id).unwrap();
                     if next_edge.from != edge.from {
                         return Err(KernelError::InvalidOperation); 
                     }
                }
            }
        }

        Ok(())
    }
}
