use std::collections::BTreeMap;

use crate::error::{KernelError, Result};
use crate::dist::euclidean_distance_squared;

#[derive(Debug, Clone)]
pub struct HNSWConfig {
    pub m: usize,
    pub m_max: usize, // usually M for higher layers, M_max0 for layer 0
    pub max_level: usize,
    pub ef_construction: usize,
}

impl Default for HNSWConfig {
    fn default() -> Self {
        Self {
            m: 16,
            m_max: 32,
            max_level: 16,
            ef_construction: 64,
        }
    }
}

#[derive(Debug)]
pub struct Node {
    pub id: u64,
    pub level: u8,
    // [layer][neighbor_index] -> neighbor_id
    pub neighbors: Vec<Vec<u64>>,
}

impl Node {
    pub fn new(id: u64, level: u8) -> Self {
        // Initialize adjacency lists for levels 0 to level
        let neighbors = vec![Vec::new(); (level + 1) as usize];
        Self {
            id,
            level,
            neighbors,
        }
    }

    /// Entropy-Aware Level Assignment
    /// Hash(id || vector) -> Trailing Zeros
    pub fn assign_level(id: u64, vector: &[i32], max_level: usize) -> u8 {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&id.to_le_bytes());
        for val in vector {
            hasher.update(&val.to_le_bytes());
        }
        let hash = hasher.finalize();
        
        // Take first 8 bytes (u64) to count zeros
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&hash.as_bytes()[0..8]);
        let val = u64::from_le_bytes(bytes);
        
        // trailing_zeros returns u32, cast to usize
        let zeros = val.trailing_zeros() as usize;
        let level = std::cmp::min(zeros, max_level - 1); // 0-indexed levels
        
        level as u8
    }
}

#[derive(Debug)]
pub struct HNSWGraph {
    pub config: HNSWConfig,
    pub nodes: BTreeMap<u64, Node>,
    pub entry_point: Option<u64>,
}

impl HNSWGraph {
    pub fn new(config: HNSWConfig) -> Self {
        Self {
            config,
            nodes: BTreeMap::new(),
            entry_point: None,
        }
    }

    /// Helper to get dist between two nodes by ID (requires access to global vector store)
    fn dist(&self, id_a: u64, id_b: u64, vectors: &BTreeMap<u64, Vec<i32>>) -> Result<i64> {
        let vec_a = vectors.get(&id_a).ok_or(KernelError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "Node A not found")))?;
        let vec_b = vectors.get(&id_b).ok_or(KernelError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "Node B not found")))?;
        euclidean_distance_squared(vec_a, vec_b)
    }

    fn dist_query(&self, query: &[i32], id_b: u64, vectors: &BTreeMap<u64, Vec<i32>>) -> Result<i64> {
        let vec_b = vectors.get(&id_b).ok_or(KernelError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "Node B not found")))?;
        euclidean_distance_squared(query, vec_b)
    }

    /// Insert a node into the graph.
    /// Assumes the vector is already in `vectors`.
    pub fn insert(&mut self, id: u64, vector: &[i32], vectors: &BTreeMap<u64, Vec<i32>>) -> Result<()> {
        let level = Node::assign_level(id, vector, self.config.max_level);
        let mut new_node = Node::new(id, level);

        // If graph is empty, set entry point and return
        if self.entry_point.is_none() {
            self.nodes.insert(id, new_node);
            self.entry_point = Some(id);
            return Ok(());
        }

        let mut curr_entry = self.entry_point.unwrap();
        let max_level = self.nodes.get(&curr_entry).unwrap().level;
        let target_level = level;

        // 1. Greedy descent from Top to target_level + 1
        // (If new node is higher than current max, we skip this and update entry point later)
        if max_level > target_level {
            for l in (target_level + 1..=max_level).rev() {
                 let mut changed = true;
                 while changed {
                     changed = false;
                     let curr_dist = self.dist_query(vector, curr_entry, vectors)?;
                     let node = self.nodes.get(&curr_entry).unwrap();
                     
                     // Simply scan neighbors at this layer to see if any is closer
                     if let Some(neighbors) = node.neighbors.get(l as usize) {
                         for &neighbor_id in neighbors {
                             let d = self.dist_query(vector, neighbor_id, vectors)?;
                             if d < curr_dist {
                                 curr_entry = neighbor_id;
                                 changed = true; // Optimization: Keep going from new best
                                 // Note: greedy descent usually checks ALL neighbors of current best, 
                                 // picks BEST one, then moves. 
                                 // Simple greedy: Update curr_entry if better found.
                             }
                         }
                     }
                 }
            }
        }

        // 2. Insert at layers 0 to target_level
        let ep_search = vec![curr_entry]; 
        // Logic: For each layer from min(max_level, target_level) down to 0
        let start_layer = std::cmp::min(max_level, target_level);
        
        for l in (0..=start_layer).rev() {
            // Search locally for M neighbors (using ef_construction)
            // Simplified: Just find closest candidates in this layer starting from ep_search
            // For rigorous HNSW, we use `search_layer`.
            // Here, we just do a simplified neighborhood search:
            // Iterate candidates, find K closest.
            
            // NOTE: Implementing FULL HNSW search_layer is complex. 
            // We'll stick to a simpler "Greedy Select" as per prompt "Run Greedy Insert".
            // "Connect to nearest neighbors at each layer."
            
            // Let's implement basic `search_layer` logic inline or helper?
            let candidates = self.search_layer(vector, &ep_search, self.config.ef_construction, l, vectors)?;
            
            // Select M neighbors
            let neighbors = self.select_neighbors(&candidates, self.config.m, vectors)?;
            
            // Add connections
            new_node.neighbors[l as usize] = neighbors.clone();
            
            // Add BACK connections (bidirectional)
            // We need to mutate OTHER nodes.
            // Problem: self.nodes ownership. `new_node` is owned locally.
            // We'll insert new_node later. For now, we collect back-links to add.
        }
        
        // Insert new node into map
        self.nodes.insert(id, new_node);
        
        // Now perform back-linking (since new_node is in map)
        for l in (0..=start_layer).rev() {
            let neighbors = self.nodes.get(&id).unwrap().neighbors[l as usize].clone();
            for neighbor_id in neighbors {
                self.add_connection(neighbor_id, id, l, vectors)?;
            }
        }

        // Update entry point if new node is higher
        if target_level > max_level {
            self.entry_point = Some(id);
        }

        Ok(())
    }
    
    /// Basic greedy search in a layer
    fn search_layer(&self, query: &[i32], entry_points: &[u64], ef: usize, layer: u8, vectors: &BTreeMap<u64, Vec<i32>>) -> Result<Vec<(u64, i64)>> {
        let mut visited = std::collections::HashSet::new();
        
        // Use simpler greedy pool instead of complex heaps for this phase
        let mut pool: Vec<(u64, i64)> = Vec::new();
        let mut queue: std::collections::VecDeque<u64> = std::collections::VecDeque::new();
        
        for &ep in entry_points {
            if visited.insert(ep) {
                let d = self.dist_query(query, ep, vectors)?;
                pool.push((ep, d));
                queue.push_back(ep);
            }
        }
        
        while let Some(curr_id) = queue.pop_front() {
             let node = self.nodes.get(&curr_id).unwrap();
             if let Some(neighbors) = node.neighbors.get(layer as usize) {
                 for &n_id in neighbors {
                     if visited.insert(n_id) {
                         let d = self.dist_query(query, n_id, vectors)?;
                         pool.push((n_id, d));
                         queue.push_back(n_id);
                     }
                 }
             }
             
             // Sort and prune pool to ef
             pool.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0))); // Sort by dist ASC
             if pool.len() > ef * 2 { // Heuristic pruning to avoid exploding queue
                 pool.truncate(ef);
                 // Rebuild queue?? No, this is BFS/Greedy hybrid.
                 // Correct logic is: explore from 'nearest' in pool that hasn't been explored.
             }
        }
         
        // Return Top-ef
        pool.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        if pool.len() > ef {
            pool.truncate(ef);
        }
        Ok(pool)
    }

    /// Select M neighbors using "Dist ASC, ID ASC" baseline
    fn select_neighbors(&self, candidates: &[(u64, i64)], m: usize, _vectors: &BTreeMap<u64, Vec<i32>>) -> Result<Vec<u64>> {
        // Candidates already basically sorted, but ensure it.
        let mut sorted = candidates.to_vec();
        sorted.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        
        let selection: Vec<u64> = sorted.iter().take(m).map(|(id, _)| *id).collect();
        Ok(selection)
    }
    
    fn add_connection(&mut self, src: u64, dst: u64, layer: u8, vectors: &BTreeMap<u64, Vec<i32>>) -> Result<()> {
        let max_conn = if layer == 0 { self.config.m_max * 2 } else { self.config.m_max };
        
        // We need to mutate src node's neighbor list
        // Careful with borrow checker: 'nodes' is borrowed mutably.
        // We need distances to sort.
        
        let mut neighbors = self.nodes.get(&src).unwrap().neighbors[layer as usize].clone();
        if neighbors.contains(&dst) { return Ok(()); }
        neighbors.push(dst);
        
        // Recalculate neighbors to enforce M_max
        // We need distances for all neighbors
        let mut candidates = Vec::new();
        for &n_id in &neighbors {
             let d = self.dist(src, n_id, vectors)?;
             candidates.push((n_id, d));
        }
        
        if candidates.len() > max_conn {
             let selected = self.select_neighbors(&candidates, max_conn, vectors)?;
             // Update node
             if let Some(node) = self.nodes.get_mut(&src) {
                 node.neighbors[layer as usize] = selected;
             }
        } else {
            // Just update with new list (already pushed)
            if let Some(node) = self.nodes.get_mut(&src) {
                 node.neighbors[layer as usize] = neighbors;
             }
        }
        
        Ok(())
    }

    pub fn search(&self, query: &[i32], k: usize, vectors: &BTreeMap<u64, Vec<i32>>) -> Result<Vec<(u64, i64)>> {
        if self.entry_point.is_none() {
            return Ok(Vec::new());
        }

        let mut curr_entry = self.entry_point.unwrap();
        let max_level = self.nodes.get(&curr_entry).unwrap().level;

        // 1. Zoom down to Layer 0
        for l in (1..=max_level).rev() {
            let mut changed = true;
            while changed {
                changed = false;
                let curr_dist = self.dist_query(query, curr_entry, vectors)?;
                let node = self.nodes.get(&curr_entry).unwrap();
                if let Some(neighbors) = node.neighbors.get(l as usize) {
                     for &n_id in neighbors {
                         let d = self.dist_query(query, n_id, vectors)?;
                         if d < curr_dist {
                             curr_entry = n_id;
                             changed = true;
                         }
                     }
                }
            }
        }

        // 2. Layer 0 Search (Broad)
        let ef_search = std::cmp::max(self.config.ef_construction, k);
        let results = self.search_layer(query, &[curr_entry], ef_search, 0, vectors)?;
        
        Ok(results.into_iter().take(k).collect())
    }
}
