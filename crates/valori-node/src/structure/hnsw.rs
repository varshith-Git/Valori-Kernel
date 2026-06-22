use crate::structure::index::VectorIndex;
use std::collections::HashMap;
use std::cmp::Ordering;
use std::sync::RwLock;
use serde::{Serialize, Deserialize};

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
/// Hierarchical Navigable Small World (HNSW) Index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HnswConfig {
    pub m: usize,               // Max edges per node per layer
    pub m_max0: usize,          // Max edges per node at layer 0 (usually 2*M)
    pub ef_construction: usize, // Beam width during index build
    pub ef_search: usize,       // Beam width during query (min k, default 50)
    pub lambda: f64,            // Level generation parameter (derived from M)
}

impl Default for HnswConfig {
    fn default() -> Self {
        Self {
            m: 16,
            m_max0: 32,
            ef_construction: 100,
            ef_search: 50,
            lambda: 1.0 / (16.0f64.ln()), // 1 / ln(M)
        }
    }
}

/// Helper for tie-breaking compare.
/// Sorts by (Distance Ascending, ID Ascending).
#[derive(Debug, Clone, Copy)]
struct Candidate {
    id: u32,
    dist: f32,
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.dist == other.dist && self.id == other.id
    }
}
impl Eq for Candidate {}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        // Rust BinaryHeap is MaxHeap. We want smallest distance.
        // So we reverse comparisons? 
        // Actually usually we use specific wrappers.
        // Let's define: "Greater" means "Better to keep" or "Closer"?
        // Standard BinaryHeap pops largest. 
        // For search (keep smallest), we want largest distance at top to pop it when full.
        // So MaxHeap of (dist, id) is correct for a fixed-size buffer where we evict worst.
        
        // But for selecting "Nearest", we wrap in MinHeap or Reverse.
        
        // Let's stick to explicit logic in algos.
        // Here, let's implement standard ordering: Small dist < Large dist.
        self.dist.partial_cmp(&other.dist).unwrap_or(Ordering::Equal)
            .then_with(|| self.id.cmp(&other.id))
    }
}

pub struct HnswIndex {
    config: HnswConfig,
    vectors: RwLock<HashMap<u32, Vec<f32>>>,
    // Layers: Vec<HashMap<u32, Vec<u32>>>
    layers: RwLock<Vec<HashMap<u32, Vec<u32>>>>,
    entry_point: RwLock<Option<u32>>, 
    max_level: RwLock<usize>,
}

impl HnswIndex {
    pub fn new() -> Self {
        Self::new_with_config(HnswConfig::default())
    }

    pub fn new_with_config(config: HnswConfig) -> Self {
        Self {
            config,
            vectors: RwLock::new(HashMap::new()),
            layers: RwLock::new(vec![HashMap::new()]), // Level 0 always exists
            entry_point: RwLock::new(None),
            max_level: RwLock::new(0),
        }
    }

    pub fn config(&self) -> &HnswConfig {
        &self.config
    }
    
    fn dist(&self, v1: &[f32], v2: &[f32]) -> f32 {
        // L2 Squared
        v1.iter().zip(v2).map(|(a, b)| (a - b).powi(2)).sum()
    }
    
    /// Deterministic Level Generation using FNV1a
    fn deterministic_level(&self, id: u32) -> usize {
        let mut hash: u64 = 0xcbf29ce484222325;
        let prime: u64 = 0x100000001b3;
        
        for byte in id.to_le_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(prime);
        }
        
        let scale = 1.0 / (u64::MAX as f64);
        let u = (hash as f64) * scale;
        let u = if u < 1e-9 { 1e-9 } else { u };
        
        let f_level = -u.ln() * self.config.lambda;
        f_level.floor() as usize
    }

    #[allow(dead_code)]
    fn safe_dist(&self, v1: Option<&Vec<f32>>, v2: Option<&Vec<f32>>) -> f32 {
        if let (Some(a), Some(b)) = (v1, v2) {
            self.dist(a, b)
        } else {
            f32::MAX // Treat missing as infinite distance
        }
    }

    fn search_layer(&self, entry: u32, query: &[f32], ef: usize, _level: usize, layer_edges: &HashMap<u32, Vec<u32>>, vectors: &HashMap<u32, Vec<f32>>) -> Vec<Candidate> {
        let entry_vec = if let Some(v) = vectors.get(&entry) { v } else { return vec![]; };
        let dist = self.dist(query, entry_vec);
        
        let mut visited = std::collections::HashSet::new();
        visited.insert(entry);
        
        use std::collections::BinaryHeap;
        use std::cmp::Reverse;
        
        // C: Candidates to explore (MinHeap)
        let mut c = BinaryHeap::new(); 
        c.push(Reverse(Candidate { id: entry, dist }));
        
        // W: Best results found (MaxHeap)
        let mut w = BinaryHeap::new();
        w.push(Candidate { id: entry, dist }); 
        
        while let Some(Reverse(curr)) = c.pop() {
            let user_dist = curr.dist;
            
            if let Some(worst) = w.peek() {
                if user_dist > worst.dist {
                    break;
                }
            }
            
            if let Some(neighbors) = layer_edges.get(&curr.id) {
                for &neighbor_id in neighbors {
                    if visited.contains(&neighbor_id) { continue; }
                    visited.insert(neighbor_id);
                    
                    let neighbor_vec = if let Some(v) = vectors.get(&neighbor_id) { v } else { continue };
                    let d = self.dist(query, neighbor_vec);
                    
                    let cand = Candidate { id: neighbor_id, dist: d };
                    
                    let mut added = false;
                    if w.len() < ef {
                        w.push(cand);
                        added = true;
                    } else if let Some(worst) = w.peek() {
                        if d < worst.dist || (d == worst.dist && neighbor_id < worst.id) {
                             w.pop();
                             w.push(cand);
                             added = true;
                        }
                    }
                    
                    if added {
                        c.push(Reverse(cand));
                    }
                }
            }
        }
        
        w.into_sorted_vec()
    }

    // ... (other methods using safe indexing)

    fn select_neighbors(&self, candidates: Vec<Candidate>, m: usize) -> Vec<u32> {
        candidates.iter().take(m).map(|c| c.id).collect()
    }
}

impl VectorIndex for HnswIndex {
    fn build(&mut self, records: &[(u32, Vec<f32>)]) {
        for (id, vec) in records {
            self.insert(*id, vec);
        }
    }

    fn insert(&mut self, id: u32, vector: &[f32]) {
        self.vectors.write().unwrap().insert(id, vector.to_vec());
        let level = self.deterministic_level(id);

        // Read entry point BEFORE any modifications so first-insert detection is correct.
        let curr_entry = *self.entry_point.read().unwrap();

        if curr_entry.is_none() {
            // First insert: initialize layer slots then set entry point.
            let mut layers = self.layers.write().unwrap();
            let mut max_l = self.max_level.write().unwrap();
            layers.resize_with(level + 1, HashMap::new);
            for l in 0..=level {
                layers[l].insert(id, Vec::new());
            }
            *max_l = level;
            drop(layers);
            drop(max_l);
            *self.entry_point.write().unwrap() = Some(id);
            return;
        }

        // Expand layer structure if this node reaches a new maximum level.
        {
            let mut layers = self.layers.write().unwrap();
            let mut max_l = self.max_level.write().unwrap();
            if level > *max_l {
                layers.resize_with(level + 1, HashMap::new);
                *max_l = level;
                *self.entry_point.write().unwrap() = Some(id);
            }
            // Pre-create empty neighbor slots so search_layer can find the node.
            for l in 0..=level {
                layers[l].entry(id).or_insert_with(Vec::new);
            }
        }

        let mut curr_entry_id = curr_entry.unwrap();
        let max_l = *self.max_level.read().unwrap();
        let vectors_guard = self.vectors.read().unwrap();

        // Greedy descent through layers above this node's level to find a close entry.
        {
            let layers_guard = self.layers.read().unwrap();
            for l in (level + 1..=max_l).rev() {
                let mut changed = true;
                while changed {
                    changed = false;
                    let curr_vec = if let Some(v) = vectors_guard.get(&curr_entry_id) { v } else { break };
                    let curr_dist = self.dist(vector, curr_vec);
                    if let Some(layer_l) = layers_guard.get(l) {
                        if let Some(neighbors) = layer_l.get(&curr_entry_id) {
                            for &neighbor in neighbors {
                                if let Some(n_vec) = vectors_guard.get(&neighbor) {
                                    let d = self.dist(vector, n_vec);
                                    if d < curr_dist {
                                        curr_entry_id = neighbor;
                                        changed = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Connect node into layers 0..=level.
        let mut layers = self.layers.write().unwrap();
        for l in (0..=level).rev() {
            let candidates = self.search_layer(
                curr_entry_id, vector, self.config.ef_construction, l,
                layers.get(l).unwrap(), &vectors_guard,
            );
            let m = if l == 0 { self.config.m_max0 } else { self.config.m };
            let neighbors = self.select_neighbors(candidates.clone(), m);
            layers.get_mut(l).unwrap().insert(id, neighbors.clone());
            for &neighbor_id in &neighbors {
                if let Some(neighbor_edges) = layers.get_mut(l).unwrap().get_mut(&neighbor_id) {
                    neighbor_edges.push(id);
                    if neighbor_edges.len() > m {
                        let n_vec = if let Some(v) = vectors_guard.get(&neighbor_id) { v } else { continue };
                        let mut n_candidates: Vec<Candidate> = neighbor_edges.iter()
                            .filter_map(|&nid| {
                                vectors_guard.get(&nid).map(|v| Candidate { id: nid, dist: self.dist(n_vec, v) })
                            })
                            .collect();
                        n_candidates.sort();
                        *neighbor_edges = n_candidates.into_iter().take(m).map(|c| c.id).collect();
                    }
                }
            }
            if !candidates.is_empty() {
                curr_entry_id = candidates[0].id;
            }
        }
    }

    fn delete(&mut self, id: u32) {
        self.vectors.write().unwrap().remove(&id);

        // Remove the deleted node from all layer adjacency lists so it cannot
        // act as a routing waypoint in future searches. Without this, deleted
        // nodes become permanent dead-ends in the graph, silently degrading recall.
        {
            let mut layers = self.layers.write().unwrap();
            for layer in layers.iter_mut() {
                layer.remove(&id);
                for neighbors in layer.values_mut() {
                    neighbors.retain(|&n| n != id);
                }
            }
        }

        // If the deleted node was the entry point, find a replacement from the
        // highest non-empty layer so the graph remains navigable.
        let is_entry = *self.entry_point.read().unwrap() == Some(id);
        if is_entry {
            let layers = self.layers.read().unwrap();
            let max_l = *self.max_level.read().unwrap();
            let new_ep = (0..=max_l)
                .rev()
                .flat_map(|l| layers.get(l))
                .find_map(|layer| layer.keys().next().copied());
            drop(layers);
            *self.entry_point.write().unwrap() = new_ep;
        }
    }

    fn search(&self, query: &[f32], k: usize) -> Vec<(u32, f32)> {
        let max_l = *self.max_level.read().unwrap();
        let entry_pt = *self.entry_point.read().unwrap();
        
        if entry_pt.is_none() {
            return Vec::new();
        }
        
        let mut curr_entry = entry_pt.unwrap();
        
        let vectors = self.vectors.read().unwrap();
        let layers = self.layers.read().unwrap();
        
        for l in (1..=max_l).rev() {
             let mut changed = true;
             while changed {
                 changed = false;
                 if let Some(c_vec) = vectors.get(&curr_entry) {
                     let curr_dist = self.dist(query, c_vec);
                     if let Some(layer_l) = layers.get(l) {
                         if let Some(neighbors) = layer_l.get(&curr_entry) {
                             for &n in neighbors {
                                 if let Some(n_vec) = vectors.get(&n) {
                                     let d = self.dist(query, n_vec);
                                     if d < curr_dist { 
                                         curr_entry = n;
                                         changed = true;
                                     }
                                 }
                             }
                         }
                     }
                 } else {
                     break; 
                 }
             }
        }
        
        // ef must be at least k (HNSW correctness) and at least ef_search (quality floor).
        let ef = k.max(self.config.ef_search);
        let results = self.search_layer(curr_entry, query, ef, 0, layers.get(0).unwrap(), &vectors);
        
        results.into_iter().take(k).map(|c| (c.id, c.dist)).collect()
    }

    fn snapshot(&self) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        #[derive(Serialize)]
        struct HnswDump<'a> {
            config: &'a HnswConfig,
            entry_point: Option<u32>,
            max_level: usize,
            vectors: Vec<(u32, &'a Vec<f32>)>, 
            layers: Vec<Vec<(u32, &'a Vec<u32>)>>, 
        }

        let entry_point = *self.entry_point.read().unwrap(); // RwLock poison is ignored
        
        let vectors_guard = self.vectors.read().unwrap();
        let layers_guard = self.layers.read().unwrap();
        let max_level = *self.max_level.read().unwrap();

        let mut sorted_vectors: Vec<_> = vectors_guard.iter().map(|(k, v)| (*k, v)).collect();
        sorted_vectors.sort_by_key(|(k, _)| *k);

        let mut sorted_layers = Vec::with_capacity(layers_guard.len());
        for layer_map in layers_guard.iter() {
            let mut sorted_nodes: Vec<_> = layer_map.iter().map(|(k, v)| (*k, v)).collect();
            sorted_nodes.sort_by_key(|(k, _)| *k);
            sorted_layers.push(sorted_nodes);
        }

        let dump = HnswDump {
            config: &self.config,
            entry_point,
            max_level,
            vectors: sorted_vectors,
            layers: sorted_layers,
        };

        Ok(bincode::serde::encode_to_vec(&dump, bincode::config::standard())?)
    }

    fn restore(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[derive(Deserialize)]
        struct HnswLoad {
            config: HnswConfig,
            entry_point: Option<u32>,
            max_level: usize,
            vectors: Vec<(u32, Vec<f32>)>,
            layers: Vec<Vec<(u32, Vec<u32>)>>,
        }

        let dump: HnswLoad = bincode::serde::decode_from_slice(data, bincode::config::standard())?.0;
        
        self.config = dump.config;

        let mut vectors = self.vectors.write().unwrap();
        vectors.clear();
        for (id, vec) in dump.vectors {
            vectors.insert(id, vec);
        }
        
        let mut layers = self.layers.write().unwrap();
        layers.clear();
        while layers.len() < dump.layers.len() {
             layers.push(HashMap::new());
        }
        
        for (level, layer_nodes) in dump.layers.into_iter().enumerate() {
             for (id, neighbors) in layer_nodes {
                 layers.get_mut(level).unwrap().insert(id, neighbors);
             }
        }
        
        *self.entry_point.write().unwrap() = dump.entry_point;
        *self.max_level.write().unwrap() = dump.max_level;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::index::VectorIndex;

    fn make_vec(vals: &[f32]) -> Vec<f32> { vals.to_vec() }

    // Regression test for Bug A: first insert with deterministic_level > 0 used to
    // produce a disconnected entry point, making the graph un-navigable.
    // Force the condition by inserting enough nodes that one reaches level >= 1.
    #[test]
    fn first_high_level_insert_is_searchable() {
        let mut idx = HnswIndex::new();
        // Insert 64 nodes — statistically guarantees at least one gets level >= 1.
        for i in 0..64u32 {
            let v: Vec<f32> = (0..8).map(|j| (i * 8 + j) as f32).collect();
            idx.insert(i, &v);
        }
        // Every inserted node must be reachable from search.
        let query: Vec<f32> = (0..8).map(|j| j as f32).collect(); // close to node 0
        let results = idx.search(&query, 5);
        assert!(!results.is_empty(), "search must return results after inserting 64 nodes");
        assert_eq!(results[0].0, 0, "nearest to [0..8] should be node 0");
    }

    // Regression test for Bug B: delete must remove the node from layer adjacency
    // lists, not just from the vector map. Previously deleted nodes remained as
    // routing waypoints and were silently skipped during traversal.
    #[test]
    fn deleted_node_not_returned_in_search() {
        let mut idx = HnswIndex::new();
        for i in 0..10u32 {
            let v: Vec<f32> = vec![i as f32, 0.0, 0.0, 0.0];
            idx.insert(i, &v);
        }
        // node 0 is the closest to [0,0,0,0]; delete it.
        idx.delete(0);
        let results = idx.search(&[0.0, 0.0, 0.0, 0.0], 5);
        assert!(
            results.iter().all(|(id, _)| *id != 0),
            "deleted node 0 must not appear in search results"
        );
    }

    // After deleting the entry point the graph must still be searchable.
    #[test]
    fn graph_navigable_after_entry_point_deleted() {
        let mut idx = HnswIndex::new();
        for i in 0..8u32 {
            idx.insert(i, &[i as f32, 0.0]);
        }
        let ep = *idx.entry_point.read().unwrap();
        if let Some(ep_id) = ep {
            idx.delete(ep_id);
            let results = idx.search(&[0.0, 0.0], 3);
            assert!(!results.is_empty(), "graph must remain searchable after entry point deletion");
            assert!(results.iter().all(|(id, _)| *id != ep_id));
        }
    }
}
