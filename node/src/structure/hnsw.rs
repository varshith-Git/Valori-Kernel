use crate::structure::index::VectorIndex;
use std::collections::HashMap;
use std::cmp::Ordering;
use std::sync::RwLock;

/// Configuration for HNSW
#[derive(Debug, Clone)]
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

    fn search_layer(&self, entry: u32, query: &[f32], ef: usize, _level: usize, layer_edges: &HashMap<u32, Vec<u32>>, vectors: &HashMap<u32, Vec<f32>>) -> Vec<Candidate> {
        let entry_vec = &vectors[&entry];
        let dist = self.dist(query, entry_vec);
        
        let mut visited = std::collections::HashSet::new();
        visited.insert(entry);
        
        use std::collections::BinaryHeap;
        use std::cmp::Reverse;
        
        // C: Candidates to explore (MinHeap - Closest first)
        let mut c = BinaryHeap::new(); 
        c.push(Reverse(Candidate { id: entry, dist }));
        
        // W: Best results found (MaxHeap - Farthest first)
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
        
        // Removed unnecessary mut
        w.into_sorted_vec()
    }
    
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
        // Removed unnecessary mut (curr_entry is immutable binding)
        let curr_entry = *self.entry_point.read().unwrap();
        
        if curr_entry.is_none() {
            *self.entry_point.write().unwrap() = Some(id);
            for l in 0..=level {
                 self.layers.write().unwrap()[l].insert(id, Vec::new());
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
                    let curr_dist = self.dist(vector, &vectors_guard[&curr_entry_id]);
                    
                    if let Some(neighbors) = layers_guard[l].get(&curr_entry_id) {
                         for &neighbor in neighbors {
                             let d = self.dist(vector, &vectors_guard[&neighbor]);
                             if d < curr_dist {
                                 curr_entry_id = neighbor;
                                 changed = true;
                             }
                         }
                    }
                }
            }
        }
        
        let mut layers = self.layers.write().unwrap();
        
        for l in (0..=level).rev() {
             let candidates = self.search_layer(curr_entry_id, vector, self.config.ef_construction, l, &layers[l], &vectors_guard);
             
             let m = if l == 0 { self.config.m_max0 } else { self.config.m };
             let neighbors = self.select_neighbors(candidates.clone(), m);
             
             layers[l].insert(id, neighbors.clone());
             
             for &neighbor_id in &neighbors {
                 let neighbor_edges = layers[l].get_mut(&neighbor_id).unwrap();
                 neighbor_edges.push(id);

                 if neighbor_edges.len() > m {
                      let n_vec = &vectors_guard[&neighbor_id];
                      let mut n_candidates: Vec<Candidate> = neighbor_edges.iter().map(|&nid| {
                           let v = &vectors_guard[&nid];
                           Candidate { id: nid, dist: self.dist(n_vec, v) }
                      }).collect();
                      n_candidates.sort(); 
                      
                      let best: Vec<u32> = n_candidates.into_iter().take(m).map(|c| c.id).collect();
                      *neighbor_edges = best;
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
                 let curr_dist = self.dist(query, &vectors[&curr_entry]);
                 if let Some(neighbors) = layers[l].get(&curr_entry) {
                     for &n in neighbors {
                         let d = self.dist(query, &vectors[&n]);
                         if d < curr_dist { 
                             curr_entry = n;
                             changed = true;
                         }
                     }
                 }
             }
        }
        
        let ef = k.max(50); 
        let results = self.search_layer(curr_entry, query, ef, 0, &layers[0], &vectors);
        
        results.into_iter().take(k).map(|c| (c.id, c.dist)).collect()
    }
}
