use crate::types::FixedPointVector;
use crate::dist::euclidean_distance_squared;
use crate::error::{Result, KernelError};
use std::collections::{HashMap, BinaryHeap};
use std::cmp::Ordering;
use rustc_hash::FxHashSet;
use std::fs::File;
use std::io::{BufWriter, Write, BufReader, Read};
use byteorder::{LittleEndian, WriteBytesExt, ReadBytesExt};

// Magic Header for Validation
const SNAPSHOT_MAGIC: &[u8; 9] = b"VALORI_V3";

// Constants for HNSW
const M: usize = 16;
const M_MAX: usize = 32;
const EF_CONSTRUCTION: usize = 64; 

/// A deterministic, Fixed-Point HNSW Graph with FLAT Arena Storage.
#[derive(Debug)]
pub struct ValoriHNSW {
    /// THE ARENA: Contiguous memory for all vectors.
    /// Layout: [v0_0, v0_1... v0_d, v1_0...]
    pub vectors: Vec<i32>,
    pub dim: usize,
    
    /// Parallel Array: Metadata for each vector (Optional)
    pub metadata: Vec<Option<Vec<u8>>>,

    /// Parallel Array: Filter Tags (u64) for Categories/Masks
    pub tags: Vec<u64>,
    
    /// Parallel Array: External ID for each vector (Index = Internal ID)
    pub external_ids: Vec<u64>,

    /// Mapping from External User ID (u64) -> Internal Arena ID (u32)
    pub id_map: HashMap<u64, u32>,

    pub layers: Vec<Vec<Vec<u32>>>,
    pub entry_point: Option<u32>,
    pub max_level: usize,
}

impl Default for ValoriHNSW {
    fn default() -> Self {
        Self::new(128) // Default to 128 if not specified (will be updated on insert)
    }
}

impl ValoriHNSW {
    pub fn new(initial_dim: usize) -> Self {
        Self {
            vectors: Vec::with_capacity(1_000_000 * initial_dim), 
            dim: initial_dim,
            metadata: Vec::with_capacity(1_000_000),
            tags: Vec::with_capacity(1_000_000),
            external_ids: Vec::with_capacity(1_000_000),
            id_map: HashMap::new(),
            layers: vec![Vec::new()],
            entry_point: None,
            max_level: 0,
        }
    }

    #[inline(always)]
    fn get_vec(&self, id: u32) -> &[i32] {
        let start = id as usize * self.dim;
        &self.vectors[start .. start + self.dim]
    }

    pub fn insert(&mut self, external_id: u64, vector: FixedPointVector, meta: Option<Vec<u8>>, tag: u64) -> Result<()> {
        if self.id_map.contains_key(&external_id) {
             return Ok(());
        }

        // Auto-detect dim on first insert if needed (or valid)
        if self.vectors.is_empty() && self.dim == 0 {
             self.dim = vector.len();
        } else if vector.len() != self.dim {
             // Handle resize or error? 
             // If we initialized with default 128 but vector is different... 
             // Ideally we enforce dim consistency.
             if self.vectors.is_empty() {
                 self.dim = vector.len(); // Adjust if empty
             } else {
                 return Err(KernelError::DimensionMismatch { expected: self.dim, found: vector.len() });
             }
        }

        let internal_id = (self.vectors.len() / self.dim) as u32;
        self.vectors.extend_from_slice(&vector); // Flat Copy
        self.metadata.push(meta);
        self.tags.push(tag);
        self.external_ids.push(external_id);
        self.id_map.insert(external_id, internal_id);

        let level = self.determine_level(external_id, &vector);

        while self.layers.len() <= level {
            self.layers.push(Vec::new());
        }
        
        for l in 0..=level {
            if self.layers[l].len() <= internal_id as usize {
                 self.layers[l].resize(internal_id as usize + 1, Vec::new());
            }
        }

        match self.entry_point {
            None => {
                self.entry_point = Some(internal_id);
                self.max_level = level;
            }
            Some(entry_pt) => {
                self.insert_into_graph(internal_id, level, entry_pt, &vector)?;
            }
        }
        
        if level > self.max_level {
            self.max_level = level;
            self.entry_point = Some(internal_id);
        }
        
        Ok(())
    }

    fn determine_level(&self, id: u64, vector: &[i32]) -> usize {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&id.to_le_bytes());
        for val in vector {
            hasher.update(&val.to_le_bytes());
        }
        let hash = hasher.finalize();
        
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&hash.as_bytes()[0..8]);
        let val = u64::from_le_bytes(bytes);
        let zeros = val.trailing_zeros() as usize;
        std::cmp::min(zeros, 15)
    }
    
    fn insert_into_graph(&mut self, q_id: u32, q_level: usize, mut curr_node: u32, q_vec: &[i32]) -> Result<()> {
        
        for l in (q_level + 1 ..= self.max_level).rev() {
             let mut changed = true;
             // self.get_vec requires borrowing &self. But self.vectors needs to be accessed
             // We can use split_at_mut if we need mutable access, but we only need read.
             // But Rust borrow rules might complain if we hold ref to vectors.
             // Actually `self.get_vec` takes `&self`.
             // `euclidean_distance_squared` takes slices.
             let mut curr_dist = euclidean_distance_squared(q_vec, self.get_vec(curr_node));
             
             while changed {
                 changed = false;
                 if let Some(neighbors) = self.layers.get(l).and_then(|layer| layer.get(curr_node as usize)) {
                     for &neighbor_id in neighbors {
                         let d = euclidean_distance_squared(q_vec, self.get_vec(neighbor_id));
                         if d < curr_dist {
                             curr_dist = d;
                             curr_node = neighbor_id;
                             changed = true;
                         }
                     }
                 }
             }
        }
        
        let mut ep_search = vec![curr_node];
        
        for l in (0..=q_level).rev() {
            let candidates = self.search_layer(q_vec, &ep_search, EF_CONSTRUCTION, l)?;
            let selected = self.select_neighbors(&candidates, M, l == 0);
            
            self.layers[l][q_id as usize] = selected.clone();
            
            for &n_id in &selected {
                self.add_connection(n_id, q_id, l)?;
            }
            ep_search = candidates.iter().map(|c| c.id).collect();
        }
        
        Ok(())
    }
    
    fn search_layer(&self, query: &[i32], entry_points: &[u32], ef: usize, layer_idx: usize) -> Result<Vec<Candidate>> {
        let mut visited = FxHashSet::default();
        let mut candidates_to_explore = BinaryHeap::new(); 
        let mut found_nearest = BinaryHeap::new(); 
        
        for &ep in entry_points {
            if visited.insert(ep) {
                let d = euclidean_distance_squared(query, self.get_vec(ep));
                let cand = Candidate { id: ep, dist: d };
                candidates_to_explore.push(std::cmp::Reverse(cand.clone()));
                found_nearest.push(cand);
            }
        }
        
        while let Some(std::cmp::Reverse(curr)) = candidates_to_explore.pop() {
            if let Some(furthest) = found_nearest.peek() {
                if curr.dist > furthest.dist && found_nearest.len() >= ef {
                    break;
                }
            }
            
            if let Some(neighbors) = self.layers.get(layer_idx).and_then(|layer| layer.get(curr.id as usize)) {
                for &n_id in neighbors {
                    if visited.insert(n_id) {
                         let d = euclidean_distance_squared(query, self.get_vec(n_id));
                         let neighbor_cand = Candidate { id: n_id, dist: d };
                         
                         if found_nearest.len() < ef || d < found_nearest.peek().unwrap().dist {
                             candidates_to_explore.push(std::cmp::Reverse(neighbor_cand.clone()));
                             found_nearest.push(neighbor_cand);
                             if found_nearest.len() > ef {
                                 found_nearest.pop();
                             }
                         }
                    }
                }
            }
        }
        
        Ok(found_nearest.into_vec())
    }
    
    fn select_neighbors(&self, candidates: &[Candidate], m: usize, is_layer0: bool) -> Vec<u32> {
        let limit = if is_layer0 { M_MAX } else { m };
        let mut sorted = candidates.to_vec();
        sorted.sort_by(|a, b| a.dist.cmp(&b.dist));
        sorted.iter().take(limit).map(|c| c.id).collect()
    }
    
    fn add_connection(&mut self, src_id: u32, dst_id: u32, layer: usize) -> Result<()> {
        let mut connections = self.layers[layer][src_id as usize].clone();
        if connections.contains(&dst_id) { return Ok(()); }
        
        connections.push(dst_id);
        
        let max_conn = if layer == 0 { M_MAX } else { M };
        
        if connections.len() > max_conn {
             let mut candidates = Vec::new();
             // Important: src_vec needs to be extracted. get_vec uses &self.
             // We are mutably borrowing self via add_connection (&mut self).
             // But we need to read from vectors.
             // Optimization: We can just index `self.vectors` directly if borrow checker allows simple splitting?
             // No, `self.layers` is being mutated. `self.vectors` is separate field.
             // But `self.get_vec` borrows whole `self`.
             // We must access `vectors` directly to satisfy split borrow.
             // `get_vec` implementation: `&self.vectors[start..]`
             // So:
             let dim = self.dim;
             let src_start = src_id as usize * dim;
             // But I can't take slice `&self.vectors[...]` and KEEP it while looping?
             // Yes I can.
             
             let src_vec_range = src_start .. src_start + dim;
             
             for &n_id in &connections {
                 let n_start = n_id as usize * dim;
                 // Slice calculation inside loop
                 let d = euclidean_distance_squared(
                     &self.vectors[src_vec_range.clone()], 
                     &self.vectors[n_start .. n_start + dim]
                 );
                 candidates.push(Candidate { id: n_id, dist: d });
             }
             
             let selected = self.select_neighbors(&candidates, max_conn, layer == 0);
             self.layers[layer][src_id as usize] = selected;
        } else {
             self.layers[layer][src_id as usize] = connections;
        }
        
        Ok(())
    }

    pub fn search(&self, query: &[i32], k: usize, filter_tag: Option<u64>) -> Result<Vec<(u64, i64)>> {
        if self.entry_point.is_none() {
            return Ok(Vec::new());
        }
        
        let mut curr_node = self.entry_point.unwrap();
        
        // 1. Greedy Zoom to Layer 0
        for l in (1..=self.max_level).rev() {
             let mut changed = true;
             let mut curr_dist = euclidean_distance_squared(query, self.get_vec(curr_node));
             
             while changed {
                 changed = false;
                 if let Some(neighbors) = self.layers.get(l).and_then(|layer| layer.get(curr_node as usize)) {
                     for &neighbor_id in neighbors {
                         let d = euclidean_distance_squared(query, self.get_vec(neighbor_id));
                         if d < curr_dist {
                             curr_dist = d;
                             curr_node = neighbor_id;
                             changed = true;
                         }
                     }
                 }
             }
        }
        
        // 2. Layer 0 Search
        // We search deeper (EF) to ensure we find candidates even with filtering
        let ef_search = std::cmp::max(EF_CONSTRUCTION, k * 2); // Heuristic: Double EF if filtering?
        let candidates = self.search_layer(query, &[curr_node], ef_search, 0)?;
        
        // 3. Sort and Collect with Filter
        let mut sorted = candidates;
        sorted.sort_by(|a, b| a.dist.cmp(&b.dist));
        
        let mut results = Vec::new();
        
        for c in sorted {
            // FILTER CHECK
            if let Some(req_tag) = filter_tag {
                // O(1) Lookup in flat tags array
                if self.tags[c.id as usize] != req_tag {
                    continue; // Skip mismatch
                }
            }
            
            results.push((self.external_ids[c.id as usize], c.dist));
            if results.len() >= k {
                break;
            }
        }

        Ok(results)
    }

    /// Saves the HNSW index to a binary file (Dump).
    pub fn save(&self, path: &str) -> Result<()> {
        let f = File::create(path).map_err(KernelError::IoError)?;
        let mut writer = BufWriter::new(f);

        // 1. Magic
        writer.write_all(SNAPSHOT_MAGIC).map_err(KernelError::IoError)?;

        // 2. Counts & Dimensions
        let count = self.vectors.len() / self.dim; // Record count
        writer.write_u64::<LittleEndian>(count as u64).map_err(KernelError::IoError)?;
        writer.write_u32::<LittleEndian>(self.dim as u32).map_err(KernelError::IoError)?;

        // 3. Data (Flat Arena + ID + Metadata + Tag)
        for i in 0..count {
             // A. External ID
             writer.write_u64::<LittleEndian>(self.external_ids[i]).map_err(KernelError::IoError)?;
             
             // B. Vector
             let start = i * self.dim;
             for val in &self.vectors[start .. start + self.dim] {
                 writer.write_i32::<LittleEndian>(*val).map_err(KernelError::IoError)?;
             }

             // C. Tag (V3)
             // We write tag immediately after vector (or wherever, as long as consistent)
             // Let's write tag here.
             writer.write_u64::<LittleEndian>(self.tags[i]).map_err(KernelError::IoError)?;

             // D. Metadata
             if let Some(meta) = &self.metadata[i] {
                 writer.write_u8(1).map_err(KernelError::IoError)?;
                 writer.write_u32::<LittleEndian>(meta.len() as u32).map_err(KernelError::IoError)?;
                 writer.write_all(meta).map_err(KernelError::IoError)?;
             } else {
                 writer.write_u8(0).map_err(KernelError::IoError)?;
             }
        }

        // 4. Graph Structure
        writer.write_u32::<LittleEndian>(self.layers.len() as u32).map_err(KernelError::IoError)?;
        for layer in &self.layers {
            writer.write_u32::<LittleEndian>(layer.len() as u32).map_err(KernelError::IoError)?;
            for neighbors in layer {
                writer.write_u32::<LittleEndian>(neighbors.len() as u32).map_err(KernelError::IoError)?;
                for &n_id in neighbors {
                    writer.write_u32::<LittleEndian>(n_id).map_err(KernelError::IoError)?;
                }
            }
        }
        
        // 5. Entry Point
        match self.entry_point {
            Some(ep) => {
                writer.write_u8(1).map_err(KernelError::IoError)?;
                writer.write_u32::<LittleEndian>(ep).map_err(KernelError::IoError)?;
            }
            None => writer.write_u8(0).map_err(KernelError::IoError)?,
        }
        writer.write_u32::<LittleEndian>(self.max_level as u32).map_err(KernelError::IoError)?;

        writer.flush().map_err(KernelError::IoError)?;
        Ok(())
    }

    /// Loads the HNSW index from a binary file.
    pub fn load(path: &str) -> Result<Self> {
        let f = File::open(path).map_err(KernelError::IoError)?;
        let mut reader = BufReader::new(f);

        // 1. Magic
        let mut magic = [0u8; 9];
        reader.read_exact(&mut magic).map_err(KernelError::IoError)?;
        if &magic != SNAPSHOT_MAGIC {
            return Err(KernelError::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid Snapshot Magic Header: {:?}", magic)
            )));
        }

        // 2. Setup
        let count = reader.read_u64::<LittleEndian>().map_err(KernelError::IoError)? as usize;
        let dim = reader.read_u32::<LittleEndian>().map_err(KernelError::IoError)? as usize;
        
        let mut vectors = Vec::with_capacity(count * dim);
        let mut external_ids = Vec::with_capacity(count);
        let mut metadata = Vec::with_capacity(count);
        let mut tags = Vec::with_capacity(count);
        let mut id_map = HashMap::with_capacity(count);

        // 3. Read Data
        for i in 0..count {
            let ext_id = reader.read_u64::<LittleEndian>().map_err(KernelError::IoError)?;
            external_ids.push(ext_id);
            id_map.insert(ext_id, i as u32);
            
            // Read Vector
            for _ in 0..dim {
                vectors.push(reader.read_i32::<LittleEndian>().map_err(KernelError::IoError)?);
            }
            
            // Read Tag
            tags.push(reader.read_u64::<LittleEndian>().map_err(KernelError::IoError)?);
            
            // Read Metadata
            let has_meta = reader.read_u8().map_err(KernelError::IoError)?;
            if has_meta == 1 {
                let len = reader.read_u32::<LittleEndian>().map_err(KernelError::IoError)? as usize;
                let mut m_buf = vec![0u8; len];
                reader.read_exact(&mut m_buf).map_err(KernelError::IoError)?;
                metadata.push(Some(m_buf));
            } else {
                metadata.push(None);
            }
        }

        // 4. Graph Structure
        let num_layers = reader.read_u32::<LittleEndian>().map_err(KernelError::IoError)? as usize;
        let mut layers = Vec::with_capacity(num_layers);
        
        for _ in 0..num_layers {
            let node_count = reader.read_u32::<LittleEndian>().map_err(KernelError::IoError)? as usize;
            let mut layer = Vec::with_capacity(node_count);
            for _ in 0..node_count {
                 let n_count = reader.read_u32::<LittleEndian>().map_err(KernelError::IoError)? as usize;
                 let mut neighbors = Vec::with_capacity(n_count);
                 for _ in 0..n_count {
                     neighbors.push(reader.read_u32::<LittleEndian>().map_err(KernelError::IoError)?);
                 }
                 layer.push(neighbors);
            }
            layers.push(layer);
        }

        // 5. Entry Point
        let has_ep = reader.read_u8().map_err(KernelError::IoError)?;
        let entry_point = if has_ep == 1 {
            Some(reader.read_u32::<LittleEndian>().map_err(KernelError::IoError)?)
        } else {
            None
        };
        let max_level = reader.read_u32::<LittleEndian>().map_err(KernelError::IoError)? as usize;

        Ok(Self {
            vectors,
            dim,
            external_ids,
            metadata,
            tags,
            id_map,
            layers,
            entry_point,
            max_level,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    id: u32,
    dist: i64,
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.dist.cmp(&other.dist)
    }
}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
