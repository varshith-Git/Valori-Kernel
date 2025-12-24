use crate::structure::index::VectorIndex;
use std::collections::HashMap;
use std::cmp::Ordering;
use std::sync::RwLock;
use serde::{Serialize, Deserialize};

// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
/// Hierarchical Navigable Small World (HNSW) Index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HnswConfig {
    pub m: usize,           // Max edges per node per layer
    pub m_max0: usize,      // Max edges per node at layer 0 (usually 2*M)
    pub ef_construction: usize, // Beam size during build
    pub lambda: f64,        // Level generation parameter
}

impl Default for HnswConfig {
    fn default() -> Self {
        Self {
            m: 16,
            m_max0: 32,
            ef_construction: 100,
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
        Self {
            config: HnswConfig::default(),
            vectors: RwLock::new(HashMap::new()),
            layers: RwLock::new(vec![HashMap::new()]), // Level 0 always exists
            entry_point: RwLock::new(None),
            max_level: RwLock::new(0),
        }
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
        
        // ... (lines 226-318) - I'll need to copy the insert logic or reference it if I want to keep it short, 
        // but replace_file_content requires full replacement of the chunk.
        // I will copy the insert implementation I verified earlier.
        
        let level = self.deterministic_level(id);
        
        {
            let mut layers = self.layers.write().unwrap();
            let mut max_l = self.max_level.write().unwrap();
            if level > *max_l {
                layers.resize_with(level + 1, HashMap::new);
                *max_l = level;
                *self.entry_point.write().unwrap() = Some(id);
            }
        }
        
        let max_l = *self.max_level.read().unwrap();
        let curr_entry = *self.entry_point.read().unwrap();
        
        if curr_entry.is_none() {
            *self.entry_point.write().unwrap() = Some(id);
            for l in 0..=level {
                 self.layers.write().unwrap().get_mut(l).unwrap().insert(id, Vec::new());
            }
            return;
        }
        
        let mut curr_entry_id = curr_entry.unwrap();
        
        let vectors_guard = self.vectors.read().unwrap(); 
        {
            let layers_guard = self.layers.read().unwrap();
            
            for l in (level + 1..=max_l).rev() {
                let mut changed = true;
                while changed {
                    changed = false;
                    let curr_vec = if let Some(v) = vectors_guard.get(&curr_entry_id) { v } else { break; };
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
        
        let mut layers = self.layers.write().unwrap();
        
        for l in (0..=level).rev() {
             let candidates = self.search_layer(curr_entry_id, vector, self.config.ef_construction, l, layers.get(l).unwrap(), &vectors_guard);
             
             let m = if l == 0 { self.config.m_max0 } else { self.config.m };
             let neighbors = self.select_neighbors(candidates.clone(), m);
             
             layers.get_mut(l).unwrap().insert(id, neighbors.clone());
             
             for &neighbor_id in &neighbors {
                 if let Some(neighbor_edges) = layers.get_mut(l).unwrap().get_mut(&neighbor_id) {
                     neighbor_edges.push(id);

                     if neighbor_edges.len() > m {
                          let n_vec = if let Some(v) = vectors_guard.get(&neighbor_id) { v } else { continue };
                          
                          let mut n_candidates: Vec<Candidate> = Vec::new();
                          for &nid in neighbor_edges.iter() {
                              if let Some(v) = vectors_guard.get(&nid) {
                                  n_candidates.push(Candidate { id: nid, dist: self.dist(n_vec, v) });
                              }
                          }
                          n_candidates.sort(); 
                          
                          let best: Vec<u32> = n_candidates.into_iter().take(m).map(|c| c.id).collect();
                          *neighbor_edges = best;
                     }
                 }
             }
             
             if !candidates.is_empty() {
                 curr_entry_id = candidates[0].id;
             }
        }
        
        if level > max_l {
             *self.entry_point.write().unwrap() = Some(id);
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
        
        let ef = k.max(50); 
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
