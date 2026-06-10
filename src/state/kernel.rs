// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
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
use crate::types::id::{RecordId, NodeId, EdgeId};
use crate::types::vector::FxpVector;
use crate::storage::record::Record;

#[derive(Clone)]
pub struct KernelState {
    pub dim: Option<usize>,
    pub(crate) version: Version,
    pub(crate) records: RecordPool,
    pub(crate) nodes: NodePool,
    pub(crate) edges: EdgePool,
    pub(crate) index: BruteForceIndex,
}

impl KernelState {
    pub fn new() -> Self {
        Self {
            dim: None,
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

    pub fn record_count(&self) -> usize {
        self.records.iter().count()
    }

    /// Total number of allocated record slots (live + soft-deleted; excludes hard-deleted gaps).
    /// Used to size snapshot buffers and rebuild-index loops correctly.
    pub fn total_record_slots(&self) -> usize {
        self.records.total_slots()
    }

    pub fn get_record(&self, id: RecordId) -> Option<&Record> {
        self.records.get(id)
    }

    pub fn get_node(&self, id: NodeId) -> Option<&GraphNode> {
        self.nodes.get(id)
    }

    pub fn outgoing_edges<'a>(&'a self, node_id: NodeId) -> Option<OutEdgeIterator<'a>> {
        self.nodes.get(node_id).map(|node| OutEdgeIterator::new(&self.edges, node.first_out_edge))
    }

    /// Iterate over all live graph nodes (excludes deleted/hole slots).
    pub fn iter_nodes(&self) -> impl Iterator<Item = &crate::graph::node::GraphNode> {
        self.nodes.nodes.iter().filter_map(|slot| slot.as_ref())
    }



    pub fn next_record_id(&self) -> RecordId {
        RecordId(self.records.raw_records().len() as u32)
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn next_node_id(&self) -> NodeId {
        NodeId(self.nodes.len() as u32)
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn next_edge_id(&self) -> EdgeId {
        EdgeId(self.edges.len() as u32)
    }

    pub fn is_edge_active(&self, id: EdgeId) -> bool {
        self.edges.get(id).is_some()
    }

    pub fn search_l2(&self, query: &FxpVector, results: &mut [SearchResult], filter: Option<u64>) -> usize {
        self.index.search(&self.records, query, results, filter)
    }

    pub fn create_node(&mut self, kind: crate::types::enums::NodeKind, record: Option<RecordId>) -> Result<NodeId> {
        let id = NodeId(self.nodes.len() as u32); // 0-based
        // But Command::CreateNode requires ID.
        // We need next ID.
        // NodePool doesn't expose next_id easily? 
        // It has `len()`.
        
        let cmd = Command::CreateNode {
            node_id: id,
            kind,
            record,
        };
        self.apply(&cmd)?;
        Ok(id)
    }

    pub fn create_edge(&mut self, from: NodeId, to: NodeId, kind: crate::types::enums::EdgeKind) -> Result<EdgeId> {
        let id = EdgeId(self.edges.len() as u32);
        let cmd = Command::CreateEdge {
            edge_id: id,
            from,
            to,
            kind,
        };
        self.apply(&cmd)?;
        Ok(id)
    }

    // --- Event-Sourced Write Logic ---
    
    /// Apply a KernelEvent to the state
    ///
    /// This is the ONLY valid mutation entrypoint for event-sourced operations.
    /// All state changes must flow through events for:
    /// - Deterministic replay
    /// - Audit trails
    /// - Cross-architecture reproducibility
    ///
    /// # Invariants
    /// - Same event sequence => Same final state
    /// - No side effects or implicit state
    /// - Crash-symmetric: replay(committed_events) = recovered_state
    pub fn apply_event(&mut self, evt: &crate::event::KernelEvent) -> Result<()> {
        use crate::event::KernelEvent;

        match evt {
            KernelEvent::InsertRecord { id, vector, metadata, tag } => {
                let cmd = Command::InsertRecord { 
                    id: *id, 
                    vector: vector.clone(),
                    metadata: metadata.clone(),
                    tag: *tag,
                };
                self.apply(&cmd)?;
            }

            KernelEvent::DeleteRecord { id } => {
                let cmd = Command::DeleteRecord { id: *id };
                self.apply(&cmd)?;
            }

            KernelEvent::CreateNode { id, kind, record } => {
                let cmd = Command::CreateNode {
                    node_id: *id,
                    kind: *kind,
                    record: *record,
                };
                self.apply(&cmd)?;
            }

            KernelEvent::CreateEdge { id, from, to, kind } => {
                let cmd = Command::CreateEdge {
                    edge_id: *id,
                    from: *from,
                    to: *to,
                    kind: *kind,
                };
                self.apply(&cmd)?;
            }

            KernelEvent::DeleteEdge { id } => {
                let cmd = Command::DeleteEdge { edge_id: *id };
                self.apply(&cmd)?;
            }

            KernelEvent::SoftDeleteRecord { id } => {
                let cmd = Command::SoftDeleteRecord { id: *id };
                self.apply(&cmd)?;
            }

            KernelEvent::DeleteNode { id } => {
                let cmd = Command::DeleteNode { node_id: *id };
                self.apply(&cmd)?;
            }
        }

        Ok(())
    }

    // --- Write Logic ---

    pub fn apply(&mut self, cmd: &Command) -> Result<()> {
        match cmd {
            Command::InsertRecord { id, vector, metadata, tag } => {
                let d = vector.len();
                if let Some(dim) = self.dim {
                    if d != dim {
                        return Err(KernelError::InvalidOperation);
                    }
                } else {
                    self.dim = Some(d);
                }
                
                use crate::config::MAX_METADATA_SIZE;
                if let Some(m) = metadata {
                    if m.len() > MAX_METADATA_SIZE {
                        return Err(KernelError::MetadataTooLarge);
                    }
                }

                let allocated_id = self.records.insert(vector.clone(), metadata.clone(), *tag)?;
                if allocated_id != *id {
                     return Err(KernelError::InvalidOperation);
                }
                <BruteForceIndex as VectorIndex>::on_insert(&mut self.index, allocated_id, vector);
            }
            Command::DeleteRecord { id } => {
                self.records.delete(*id)?;
                <BruteForceIndex as VectorIndex>::on_delete(&mut self.index, *id);
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
            Command::SoftDeleteRecord { id } => {
                // Mark the pool slot as a tombstone.
                self.records.soft_delete(*id)?;
                // Remove from the search index so it never surfaces in queries.
                <BruteForceIndex as VectorIndex>::on_delete(&mut self.index, *id);
            }
        }

        self.version = self.version.next();
        Ok(())
    }
    
    fn _delete_node(&mut self, node_id: NodeId) -> Result<()> {
        if self.nodes.get(node_id).is_none() {
            return Err(KernelError::NotFound);
        }

        // Collect all outgoing edge IDs via first_out_edge → next_out chain  O(out-degree)
        let out_edges: alloc::vec::Vec<EdgeId> = {
            let mut acc = alloc::vec::Vec::new();
            let mut curr = self.nodes.get(node_id).and_then(|n| n.first_out_edge);
            while let Some(eid) = curr {
                acc.push(eid);
                curr = self.edges.get(eid).and_then(|e| e.next_out);
            }
            acc
        };

        // Collect all incoming edge IDs via first_in_edge → next_in chain  O(in-degree)
        let in_edges: alloc::vec::Vec<EdgeId> = {
            let mut acc = alloc::vec::Vec::new();
            let mut curr = self.nodes.get(node_id).and_then(|n| n.first_in_edge);
            while let Some(eid) = curr {
                acc.push(eid);
                curr = self.edges.get(eid).and_then(|e| e.next_in);
            }
            acc
        };

        // Delete each incident edge once (guard against double-delete on self-loops)
        for &eid in out_edges.iter().chain(in_edges.iter()) {
            if self.edges.get(eid).is_some() {
                self._delete_edge(eid)?;
            }
        }

        self.nodes.delete(node_id)?;
        Ok(())
    }

    fn _delete_edge(&mut self, edge_id: EdgeId) -> Result<()> {
        let edge = self.edges.get(edge_id).ok_or(KernelError::NotFound)?;
        let from_node_id = edge.from;
        let to_node_id   = edge.to;

        // --- Unlink from `from` node's outgoing list ---
        {
            let mut prev: Option<EdgeId> = None;
            let mut curr = self.nodes.get(from_node_id).and_then(|n| n.first_out_edge);
            while let Some(c) = curr {
                if c == edge_id {
                    let next = self.edges.get(c).and_then(|e| e.next_out);
                    if let Some(p) = prev {
                        self.edges.get_mut(p).unwrap().next_out = next;
                    } else {
                        self.nodes.get_mut(from_node_id).unwrap().first_out_edge = next;
                    }
                    break;
                }
                prev = Some(c);
                curr = self.edges.get(c).and_then(|e| e.next_out);
            }
        }

        // --- Unlink from `to` node's incoming list ---
        {
            let mut prev: Option<EdgeId> = None;
            let mut curr = self.nodes.get(to_node_id).and_then(|n| n.first_in_edge);
            while let Some(c) = curr {
                if c == edge_id {
                    let next = self.edges.get(c).and_then(|e| e.next_in);
                    if let Some(p) = prev {
                        self.edges.get_mut(p).unwrap().next_in = next;
                    } else {
                        self.nodes.get_mut(to_node_id).unwrap().first_in_edge = next;
                    }
                    break;
                }
                prev = Some(c);
                curr = self.edges.get(c).and_then(|e| e.next_in);
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
