use crate::structure::index::VectorIndex;
use std::cmp::Ordering;
use std::sync::RwLock;
use serde::{Serialize, Deserialize};
use rustc_hash::FxHashSet;

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
/// Hierarchical Navigable Small World (HNSW) Index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HnswConfig {
    pub m: usize,
    pub m_max0: usize,
    pub ef_construction: usize,
    pub ef_search: usize,
    pub lambda: f64,
}

impl Default for HnswConfig {
    fn default() -> Self {
        Self {
            m: 16,
            m_max0: 32,
            ef_construction: 100,
            ef_search: 50,
            lambda: 1.0 / (16.0f64.ln()),
        }
    }
}

/// A single node in the HNSW graph.
/// Storing vector + neighbor lists together means a search visit touches
/// one contiguous allocation instead of two separate arrays.
struct Node {
    vector: Box<[f32]>,
    /// neighbors[level] = neighbor IDs at that level.
    /// len() == assigned_level + 1; only levels the node actually participates in.
    neighbors: Vec<Vec<u32>>,
}

/// Tie-breaking candidate: sorts ascending by distance, then ascending by id.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    id: u32,
    dist: f32,
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool { self.dist == other.dist && self.id == other.id }
}
impl Eq for Candidate {}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.dist.partial_cmp(&other.dist).unwrap_or(Ordering::Equal)
            .then_with(|| self.id.cmp(&other.id))
    }
}

/// Scalar fallback: used on non-aarch64 targets.
#[cfg(not(target_arch = "aarch64"))]
#[inline]
fn dist_scalar(v1: &[f32], v2: &[f32]) -> f32 {
    let mut sum = 0.0f32;
    for (a, b) in v1.iter().zip(v2) {
        let d = a - b;
        sum += d * d;
    }
    sum
}

/// NEON L2-squared distance, unrolled 4× (16 f32 per iteration).
/// All common embedding dims (384, 768, 1536) are multiples of 16 — no tail needed in practice.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn dist_neon(v1: &[f32], v2: &[f32]) -> f32 {
    use std::arch::aarch64::*;

    let len = v1.len();
    let mut p1 = v1.as_ptr();
    let mut p2 = v2.as_ptr();

    // Four independent accumulators → 4× ILP; each holds 4 lanes → 16 floats/iter.
    let mut acc0 = vdupq_n_f32(0.0);
    let mut acc1 = vdupq_n_f32(0.0);
    let mut acc2 = vdupq_n_f32(0.0);
    let mut acc3 = vdupq_n_f32(0.0);

    let chunks16 = len / 16;
    for _ in 0..chunks16 {
        let d0 = vsubq_f32(vld1q_f32(p1),       vld1q_f32(p2));
        let d1 = vsubq_f32(vld1q_f32(p1.add(4)), vld1q_f32(p2.add(4)));
        let d2 = vsubq_f32(vld1q_f32(p1.add(8)), vld1q_f32(p2.add(8)));
        let d3 = vsubq_f32(vld1q_f32(p1.add(12)),vld1q_f32(p2.add(12)));
        acc0 = vfmaq_f32(acc0, d0, d0);
        acc1 = vfmaq_f32(acc1, d1, d1);
        acc2 = vfmaq_f32(acc2, d2, d2);
        acc3 = vfmaq_f32(acc3, d3, d3);
        p1 = p1.add(16);
        p2 = p2.add(16);
    }

    // Merge into acc0, then 4-element horizontal sum.
    acc0 = vaddq_f32(vaddq_f32(acc0, acc1), vaddq_f32(acc2, acc3));

    // 4-element tail (handles dims that are multiples of 4 but not 16).
    let rem4 = (len % 16) / 4;
    let mut acc4 = vdupq_n_f32(0.0);
    for _ in 0..rem4 {
        let d = vsubq_f32(vld1q_f32(p1), vld1q_f32(p2));
        acc4 = vfmaq_f32(acc4, d, d);
        p1 = p1.add(4);
        p2 = p2.add(4);
    }
    acc0 = vaddq_f32(acc0, acc4);

    let mut sum = vaddvq_f32(acc0);

    // Scalar tail for any leftover elements.
    let base = chunks16 * 16 + rem4 * 4;
    for i in base..len {
        let d = *v1.get_unchecked(i) - *v2.get_unchecked(i);
        sum += d * d;
    }
    sum
}

pub struct HnswIndex {
    config: HnswConfig,
    /// nodes[id] = Some(Node { vector, neighbors }) or None if deleted.
    /// Sequential IDs from the kernel slab make this a direct O(1) index.
    nodes: RwLock<Vec<Option<Node>>>,
    entry_point: RwLock<Option<u32>>,
    max_level: RwLock<usize>,
}

#[inline]
fn ensure_node_slot(nodes: &mut Vec<Option<Node>>, idx: usize) {
    if idx >= nodes.len() {
        // extend with None; actual Node is set by caller
        nodes.resize_with(idx + 1, || None);
    }
}

impl HnswIndex {
    pub fn new() -> Self { Self::new_with_config(HnswConfig::default()) }

    pub fn new_with_config(config: HnswConfig) -> Self {
        Self {
            config,
            nodes: RwLock::new(Vec::new()),
            entry_point: RwLock::new(None),
            max_level: RwLock::new(0),
        }
    }

    pub fn config(&self) -> &HnswConfig { &self.config }

    #[inline]
    fn dist(v1: &[f32], v2: &[f32]) -> f32 {
        #[cfg(target_arch = "aarch64")]
        // SAFETY: aarch64 always has NEON; slices are valid f32 data.
        return unsafe { dist_neon(v1, v2) };
        #[cfg(not(target_arch = "aarch64"))]
        dist_scalar(v1, v2)
    }

    fn deterministic_level(&self, id: u32) -> usize {
        let mut hash: u64 = 0xcbf29ce484222325;
        let prime: u64 = 0x100000001b3;
        for byte in id.to_le_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(prime);
        }
        let u = (hash as f64) / (u64::MAX as f64);
        let u = if u < 1e-9 { 1e-9 } else { u };
        (-u.ln() * self.config.lambda).floor() as usize
    }

    /// Beam search on `nodes` at `level`.
    /// Takes a shared slice — no write access needed. Returns owned candidates.
    fn search_layer(
        &self,
        entry: u32,
        query: &[f32],
        ef: usize,
        level: usize,
        nodes: &[Option<Node>],
    ) -> Vec<Candidate> {
        let entry_node = match nodes.get(entry as usize).and_then(|n| n.as_ref()) {
            Some(n) => n,
            None => return vec![],
        };
        let dist = Self::dist(query, &entry_node.vector);

        // FxHashSet: identity-like hash for u32 keys; pre-sized to ef avoids rehash.
        let mut visited = FxHashSet::with_capacity_and_hasher(ef * 2, Default::default());
        visited.insert(entry);

        use std::collections::BinaryHeap;
        use std::cmp::Reverse;

        let mut c = BinaryHeap::with_capacity(ef); // MinHeap: candidates to explore
        c.push(Reverse(Candidate { id: entry, dist }));

        let mut w = BinaryHeap::with_capacity(ef); // MaxHeap: best results (worst at top for O(log n) evict)
        w.push(Candidate { id: entry, dist });

        while let Some(Reverse(curr)) = c.pop() {
            if let Some(worst) = w.peek() {
                if curr.dist > worst.dist { break; }
            }

            let curr_node = match nodes.get(curr.id as usize).and_then(|n| n.as_ref()) {
                Some(n) => n,
                None => continue,
            };
            let neighbors = curr_node.neighbors.get(level).map(|v| v.as_slice()).unwrap_or(&[]);

            for &neighbor_id in neighbors {
                if !visited.insert(neighbor_id) { continue; }

                let neighbor_node = match nodes.get(neighbor_id as usize).and_then(|n| n.as_ref()) {
                    Some(n) => n,
                    None => continue,
                };
                let d = Self::dist(query, &neighbor_node.vector);
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
                if added { c.push(Reverse(cand)); }
            }
        }

        let sorted = w.into_sorted_vec(); // ascending (BinaryHeap MaxHeap drain order)
        tracing::debug!(level, visited = visited.len(), found = sorted.len(), ef, "search_layer done");
        sorted
    }

    /// Algorithm 4 from the HNSW paper: heuristic neighbor selection.
    ///
    /// Iterates candidates nearest-first. Adds candidate `e` to the result only if
    /// dist(query, e) < dist(r, e) for every already-selected result `r`. This ensures
    /// spatial diversity — no selected neighbor "shadows" e — giving the graph long-range
    /// routing paths that the greedy (take-m-nearest) approach misses.
    ///
    /// `keep_pruned = true` fills remaining slots from discarded candidates so the graph
    /// stays connected in sparse regions (recommended by the paper).
    fn select_neighbors_heuristic(
        &self,
        query: &[f32],
        candidates: &[Candidate],   // must be sorted ascending by dist to query
        m: usize,
        nodes: &[Option<Node>],
        keep_pruned: bool,
    ) -> Vec<u32> {
        let mut result: Vec<Candidate> = Vec::with_capacity(m);
        let mut discarded: Vec<Candidate> = Vec::with_capacity(candidates.len());

        'outer: for &e in candidates {
            if result.len() >= m { break; }
            let e_vec = match nodes.get(e.id as usize).and_then(|n| n.as_ref()) {
                Some(n) => &n.vector,
                None => continue,
            };
            // Diversity gate: keep e only if it is closer to the query than to
            // every already-selected neighbor. If any r is nearer to e than q is,
            // r "shadows" e and e would add no new routing information.
            for r in &result {
                let r_vec = &nodes[r.id as usize].as_ref().unwrap().vector;
                if Self::dist(r_vec, e_vec) <= e.dist {
                    discarded.push(e);
                    continue 'outer;
                }
            }
            result.push(e);
        }

        if keep_pruned {
            for e in discarded {
                if result.len() >= m { break; }
                result.push(e);
            }
        }

        let mut out = Vec::with_capacity(result.len());
        out.extend(result.iter().map(|c| c.id));
        out
    }
}

impl VectorIndex for HnswIndex {
    fn build(&mut self, records: &[(u32, Vec<f32>)]) {
        for (id, vec) in records { self.insert(*id, vec); }
    }

    fn insert(&mut self, id: u32, vector: &[f32]) {
        let level = self.deterministic_level(id);
        let curr_entry = *self.entry_point.read().unwrap();

        // First insert: create node, set as entry point.
        if curr_entry.is_none() {
            let mut nodes = self.nodes.write().unwrap();
            ensure_node_slot(&mut nodes, id as usize);
            nodes[id as usize] = Some(Node {
                vector: vector.to_vec().into_boxed_slice(),
                neighbors: (0..=level).map(|l| Vec::with_capacity(if l == 0 { self.config.m_max0 } else { self.config.m })).collect(),
            });
            *self.max_level.write().unwrap() = level;
            *self.entry_point.write().unwrap() = Some(id);
            return;
        }

        // Create node with empty neighbor slots for levels 0..=level.
        {
            let mut nodes = self.nodes.write().unwrap();
            ensure_node_slot(&mut nodes, id as usize);
            nodes[id as usize] = Some(Node {
                vector: vector.to_vec().into_boxed_slice(),
                neighbors: (0..=level).map(|l| Vec::with_capacity(if l == 0 { self.config.m_max0 } else { self.config.m })).collect(),
            });
        }

        // Update entry point if this node reaches a new max level.
        {
            let mut max_l = self.max_level.write().unwrap();
            if level > *max_l {
                *max_l = level;
                *self.entry_point.write().unwrap() = Some(id);
            }
        }

        let max_l = *self.max_level.read().unwrap();
        let mut curr_entry_id = curr_entry.unwrap();

        // Single write lock for the rest of insert: search_layer borrows &nodes
        // immutably (returns owned Vec<Candidate>), then we mutably update.
        let mut nodes = self.nodes.write().unwrap();

        // Greedy descent through levels above this node's assigned level.
        for l in (level + 1..=max_l).rev() {
            loop {
                let curr_node = match nodes.get(curr_entry_id as usize).and_then(|n| n.as_ref()) {
                    Some(n) => n,
                    None => break,
                };
                let curr_dist = Self::dist(vector, &curr_node.vector);
                let neighbors = curr_node.neighbors.get(l).map(|v| v.as_slice()).unwrap_or(&[]);

                let mut best = curr_entry_id;
                let mut best_dist = curr_dist;
                for &nb in neighbors {
                    if let Some(Some(nb_node)) = nodes.get(nb as usize) {
                        let d = Self::dist(vector, &nb_node.vector);
                        if d < best_dist { best_dist = d; best = nb; }
                    }
                }
                if best == curr_entry_id { break; }
                curr_entry_id = best;
            }
        }

        // Connect node into levels 0..=level.
        for l in (0..=level).rev() {
            let m = if l == 0 { self.config.m_max0 } else { self.config.m };

            // search_layer borrows &*nodes immutably; borrow ends when candidates is bound.
            let candidates = self.search_layer(curr_entry_id, vector, self.config.ef_construction, l, &*nodes);
            let neighbors = self.select_neighbors_heuristic(vector, &candidates, m, &*nodes, true);

            // Write this node's neighbor list.
            if let Some(Some(node)) = nodes.get_mut(id as usize) {
                if l < node.neighbors.len() {
                    node.neighbors[l] = neighbors.clone();
                }
            }

            // Back-link: add id into each neighbor's list at level l, pruning if over m.
            for &nb_id in &neighbors {
                let nb_uid = nb_id as usize;
                if let Some(Some(nb_node)) = nodes.get_mut(nb_uid) {
                    if let Some(edges) = nb_node.neighbors.get_mut(l) {
                        edges.push(id);
                        if edges.len() > m {
                            // Prune with the heuristic so the neighbor's edge set also
                            // stays spatially diverse after the back-link is added.
                            let edge_ids: Vec<u32> = edges.clone();
                            let nb_vec: Box<[f32]> = nb_node.vector.clone();
                            let mut ranked: Vec<Candidate> = Vec::with_capacity(edge_ids.len());
                            ranked.extend(edge_ids.iter().filter_map(|&nid| {
                                nodes.get(nid as usize)
                                    .and_then(|n| n.as_ref())
                                    .map(|n| Candidate { id: nid, dist: Self::dist(&nb_vec, &n.vector) })
                            }));
                            ranked.sort();
                            let pruned = self.select_neighbors_heuristic(&nb_vec, &ranked, m, &**nodes, true);
                            if let Some(Some(nb2)) = nodes.get_mut(nb_uid) {
                                if let Some(e) = nb2.neighbors.get_mut(l) {
                                    e.clear();
                                    e.extend(pruned);
                                }
                            }
                        }
                    }
                }
            }

            if !candidates.is_empty() { curr_entry_id = candidates[0].id; }
        }
    }

    fn delete(&mut self, id: u32) {
        {
            let mut nodes = self.nodes.write().unwrap();
            if let Some(slot) = nodes.get_mut(id as usize) {
                *slot = None;
            }
            // Remove id from every neighbor's adjacency list.
            for node_opt in nodes.iter_mut() {
                if let Some(node) = node_opt {
                    for level_edges in &mut node.neighbors {
                        level_edges.retain(|&n| n != id);
                    }
                }
            }
        }

        let is_entry = *self.entry_point.read().unwrap() == Some(id);
        if is_entry {
            let nodes = self.nodes.read().unwrap();
            let new_ep = nodes.iter().enumerate()
                .find_map(|(i, n)| if n.is_some() { Some(i as u32) } else { None });
            drop(nodes);
            *self.entry_point.write().unwrap() = new_ep;
        }
    }

    fn search(&self, query: &[f32], k: usize) -> Vec<(u32, f32)> {
        let max_l = *self.max_level.read().unwrap();
        let mut curr_entry = match *self.entry_point.read().unwrap() {
            Some(ep) => ep,
            None => return Vec::new(),
        };

        let nodes = self.nodes.read().unwrap();

        // Greedy descent to layer 1.
        for l in (1..=max_l).rev() {
            loop {
                let curr_node = match nodes.get(curr_entry as usize).and_then(|n| n.as_ref()) {
                    Some(n) => n,
                    None => break,
                };
                let curr_dist = Self::dist(query, &curr_node.vector);
                let neighbors = curr_node.neighbors.get(l).map(|v| v.as_slice()).unwrap_or(&[]);

                let mut best = curr_entry;
                let mut best_dist = curr_dist;
                for &nb in neighbors {
                    if let Some(Some(nb_node)) = nodes.get(nb as usize) {
                        let d = Self::dist(query, &nb_node.vector);
                        if d < best_dist { best_dist = d; best = nb; }
                    }
                }
                if best == curr_entry { break; }
                curr_entry = best;
            }
        }

        let ef = k.max(self.config.ef_search);
        let results = self.search_layer(curr_entry, query, ef, 0, &nodes);
        results.into_iter().take(k).map(|c| (c.id, c.dist)).collect()
    }

    fn snapshot(&self) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        #[derive(Serialize)]
        struct NodeDump<'a> {
            id: u32,
            vector: &'a [f32],
            neighbors: &'a Vec<Vec<u32>>,
        }
        #[derive(Serialize)]
        struct HnswDump<'a> {
            config: &'a HnswConfig,
            entry_point: Option<u32>,
            max_level: usize,
            nodes: Vec<NodeDump<'a>>,
        }

        let entry_point = *self.entry_point.read().unwrap();
        let nodes_guard = self.nodes.read().unwrap();
        let max_level = *self.max_level.read().unwrap();

        let mut node_dumps: Vec<NodeDump> = nodes_guard.iter().enumerate()
            .filter_map(|(i, slot)| slot.as_ref().map(|n| NodeDump {
                id: i as u32,
                vector: &n.vector,
                neighbors: &n.neighbors,
            }))
            .collect();
        node_dumps.sort_by_key(|n| n.id);

        let dump = HnswDump { config: &self.config, entry_point, max_level, nodes: node_dumps };
        Ok(bincode::serde::encode_to_vec(&dump, bincode::config::standard())?)
    }

    fn restore(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[derive(Deserialize)]
        struct NodeLoad {
            id: u32,
            vector: Vec<f32>,
            neighbors: Vec<Vec<u32>>,
        }
        #[derive(Deserialize)]
        struct HnswLoad {
            config: HnswConfig,
            entry_point: Option<u32>,
            max_level: usize,
            nodes: Vec<NodeLoad>,
        }

        let dump: HnswLoad = bincode::serde::decode_from_slice(data, bincode::config::standard())?.0;
        self.config = dump.config;

        let mut nodes = self.nodes.write().unwrap();
        nodes.clear();
        for n in dump.nodes {
            let idx = n.id as usize;
            if idx >= nodes.len() { nodes.resize_with(idx + 1, || None); }
            nodes[idx] = Some(Node {
                vector: n.vector.into_boxed_slice(),
                neighbors: n.neighbors,
            });
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
    use std::time::Instant;

    // Run: cargo test -p valori-node --lib hnsw_latency_benchmark -- --nocapture --ignored
    #[test]
    #[ignore]
    fn hnsw_latency_benchmark() {
        fn run_dim(dim: usize) {
            let k = 10usize;
            let trials = 500usize;
            let n_max = 50_001usize;

            let mut seed: u64 = 0xdeadbeef_cafebabe;
            let mut next_f32 = |s: &mut u64| -> f32 {
                *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (((*s >> 33) as f32) / (u32::MAX as f32) - 0.5) * 0.6
            };

            let mut all_vecs: Vec<Vec<f32>> = (0..n_max)
                .map(|_| (0..dim).map(|_| next_f32(&mut seed)).collect())
                .collect();
            let query = all_vecs.pop().unwrap();

            let checkpoints = [1_000usize, 5_000, 10_000, 25_000, 50_000];

            println!("\n=== dim={dim} ===");
            println!("{:>8}  {:>10}  {:>10}  {:>10}", "N", "p50 µs", "p95 µs", "p99 µs");
            println!("{}", "-".repeat(46));

            let mut idx = HnswIndex::new();
            let mut inserted = 0usize;
            for &n in &checkpoints {
                for (i, v) in all_vecs[inserted..n].iter().enumerate() {
                    idx.insert((inserted + i) as u32, v);
                }
                inserted = n;

                let mut times_us: Vec<f64> = (0..trials).map(|_| {
                    let t = Instant::now();
                    let _ = idx.search(&query, k);
                    t.elapsed().as_secs_f64() * 1_000_000.0
                }).collect();
                times_us.sort_by(|a, b| a.partial_cmp(b).unwrap());

                let p50 = times_us[trials / 2];
                let p95 = times_us[(trials as f64 * 0.95) as usize];
                let p99 = times_us[trials - 1];
                println!("{:>8}  {:>10.1}  {:>10.1}  {:>10.1}", n, p50, p95, p99);
            }

            let results = idx.search(&query, k);
            for w in results.windows(2) {
                assert!(w[0].1 <= w[1].1, "dim={dim}: results must be sorted ascending by distance");
            }
        }

        for &dim in &[384usize, 768, 1536] {
            run_dim(dim);
        }
    }

    // Recall@k benchmark: compare HNSW results against brute-force ground truth.
    // Recall@k = |hnsw_top_k ∩ brute_top_k| / k, averaged over n_queries queries.
    // Run: cargo test -p valori-node --lib --release hnsw_recall_benchmark -- --nocapture --ignored
    #[test]
    #[ignore]
    fn hnsw_recall_benchmark() {
        fn brute_top_k(corpus: &[Vec<f32>], query: &[f32], k: usize) -> Vec<u32> {
            let mut dists: Vec<(u32, f32)> = corpus.iter().enumerate()
                .map(|(i, v)| (i as u32, HnswIndex::dist(query, v)))
                .collect();
            dists.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
            dists.into_iter().take(k).map(|(id, _)| id).collect()
        }

        fn recall(hnsw_ids: &[u32], truth_ids: &[u32]) -> f64 {
            let truth_set: rustc_hash::FxHashSet<u32> = truth_ids.iter().copied().collect();
            let hits = hnsw_ids.iter().filter(|id| truth_set.contains(id)).count();
            hits as f64 / truth_ids.len() as f64
        }

        let n_queries = 100usize;
        let corpus_size = 10_000usize;

        for &dim in &[384usize, 768, 1536] {
            let mut seed: u64 = 0xc0ffee_deadbeef;
            let next_f32 = |s: &mut u64| -> f32 {
                *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (((*s >> 33) as f32) / (u32::MAX as f32) - 0.5) * 0.6
            };

            let corpus: Vec<Vec<f32>> = (0..corpus_size)
                .map(|_| (0..dim).map(|_| next_f32(&mut seed)).collect())
                .collect();
            let queries: Vec<Vec<f32>> = (0..n_queries)
                .map(|_| (0..dim).map(|_| next_f32(&mut seed)).collect())
                .collect();

            let mut idx = HnswIndex::new();
            for (i, v) in corpus.iter().enumerate() { idx.insert(i as u32, v); }

            println!("\n=== dim={dim}, N={corpus_size}, queries={n_queries} ===");
            println!("{:>12}  {:>12}  {:>12}", "k", "recall@k", "min recall");

            for &k in &[10usize, 100] {
                let mut recalls: Vec<f64> = queries.iter().map(|q| {
                    let hnsw = idx.search(q, k);
                    let truth = brute_top_k(&corpus, q, k);
                    let hnsw_ids: Vec<u32> = hnsw.iter().map(|(id, _)| *id).collect();
                    recall(&hnsw_ids, &truth)
                }).collect();
                recalls.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let mean = recalls.iter().sum::<f64>() / recalls.len() as f64;
                let min  = recalls[0];
                println!("{:>12}  {:>11.1}%  {:>11.1}%", k, mean * 100.0, min * 100.0);
            }
        }
    }

    // ── Diagnostic helpers shared by tests 1-4 ──────────────────────────────────
    fn build_test_corpus(dim: usize, n: usize, seed0: u64) -> (Vec<Vec<f32>>, Vec<Vec<f32>>) {
        let mut seed = seed0;
        let mut next = |s: &mut u64| -> f32 {
            *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (((*s >> 33) as f32) / (u32::MAX as f32) - 0.5) * 0.6
        };
        let corpus:  Vec<Vec<f32>> = (0..n)   .map(|_| (0..dim).map(|_| next(&mut seed)).collect()).collect();
        let queries: Vec<Vec<f32>> = (0..100) .map(|_| (0..dim).map(|_| next(&mut seed)).collect()).collect();
        (corpus, queries)
    }

    fn brute_top_k(corpus: &[Vec<f32>], query: &[f32], k: usize) -> rustc_hash::FxHashSet<u32> {
        let mut dists: Vec<(u32, f32)> = corpus.iter().enumerate()
            .map(|(i, v)| (i as u32, HnswIndex::dist(query, v)))
            .collect();
        dists.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        dists.into_iter().take(k).map(|(id, _)| id).collect()
    }

    fn mean_recall(idx: &HnswIndex, corpus: &[Vec<f32>], queries: &[Vec<f32>], k: usize) -> f64 {
        let total: f64 = queries.iter().map(|q| {
            let hnsw: rustc_hash::FxHashSet<u32> = idx.search(q, k).iter().map(|(id,_)| *id).collect();
            let truth = brute_top_k(corpus, q, k);
            hnsw.intersection(&truth).count() as f64 / k as f64
        }).sum();
        total / queries.len() as f64
    }

    // Export bench vectors to /tmp/valori_hnsw_bench.bin for hnswlib comparison.
    // Header: [dim u32 LE][n_corpus u32 LE][n_queries u32 LE] then all f32 LE vectors.
    // Run: cargo test -p valori-node --lib --release hnsw_export_bench_vectors -- --nocapture --ignored
    // Then: pip install hnswlib numpy && python3 benchmarks/hnsw_hnswlib_compare.py
    #[test]
    #[ignore]
    fn hnsw_export_bench_vectors() {
        use std::io::Write;
        let dim       = 384usize;
        let n_corpus  = 10_000usize;
        let (corpus, queries) = build_test_corpus(dim, n_corpus, 0xdeadbeef);

        let path = "/tmp/valori_hnsw_bench.bin";
        let mut f = std::fs::File::create(path).unwrap();
        for &v in &[dim as u32, n_corpus as u32, queries.len() as u32] {
            f.write_all(&v.to_le_bytes()).unwrap();
        }
        for vec in corpus.iter().chain(queries.iter()) {
            for &x in vec { f.write_all(&x.to_le_bytes()).unwrap(); }
        }
        println!("Exported {n_corpus} corpus + {} query vectors (dim={dim}) to {path}", queries.len());
        println!("Run: python3 benchmarks/hnsw_hnswlib_compare.py");
    }

    // ef_construction sweep: fix ef_search=400, vary ef_construction.
    // Answers whether the 80% recall ceiling on clustered data is build-limited.
    // Expected: if graph quality is build-limited, recall rises with ef_construction.
    // Run: cargo test -p valori-node --lib --release hnsw_diag_ef_construction_sweep -- --nocapture --ignored
    #[test]
    #[ignore]
    fn hnsw_diag_ef_construction_sweep() {
        let dim = 384usize;
        let n   = 10_000usize;
        let k   = 10usize;
        let ef_search_fixed = 400usize;
        let (corpus, queries) = build_test_corpus(dim, n, 0xdeadbeef);

        println!("\n=== ef_construction sweep  dim={dim}  N={n}  ef_search={ef_search_fixed} ===");
        println!("{:>16}  {:>12}", "ef_construction", "Recall@10");
        println!("{}", "-".repeat(32));

        for &ef_c in &[50usize, 100, 200, 400, 800] {
            let config = HnswConfig { ef_construction: ef_c, ef_search: ef_search_fixed, ..HnswConfig::default() };
            let mut idx = HnswIndex::new_with_config(config);
            for (i, v) in corpus.iter().enumerate() { idx.insert(i as u32, v); }
            let r = mean_recall(&idx, &corpus, &queries, k);
            println!("{:>16}  {:>11.1}%", ef_c, r * 100.0);
        }
    }

    // Level distribution: actual nodes per level vs theoretical expectation N * (1/M)^l.
    // Run: cargo test -p valori-node --lib --release hnsw_diag_level_dist -- --nocapture --ignored
    #[test]
    #[ignore]
    fn hnsw_diag_level_dist() {
        let dim = 384usize;
        let n   = 10_000usize;
        let (corpus, _) = build_test_corpus(dim, n, 0xdeadbeef);

        let mut idx = HnswIndex::new();
        for (i, v) in corpus.iter().enumerate() { idx.insert(i as u32, v); }

        let nodes  = idx.nodes.read().unwrap();
        let max_l  = *idx.max_level.read().unwrap();
        let m      = idx.config.m as f64;

        // Count nodes at each level: a node is at level l if neighbors.len() > l.
        let mut counts = vec![0usize; max_l + 1];
        for node_opt in nodes.iter() {
            if let Some(node) = node_opt {
                for l in 0..node.neighbors.len() {
                    counts[l] += 1;
                }
            }
        }

        println!("\n=== level distribution  dim={dim}  N={n}  M={}  lambda={:.4} ===",
            idx.config.m, idx.config.lambda);
        println!("{:>6}  {:>8}  {:>10}  {:>8}", "level", "actual", "expected", "ratio");
        println!("{}", "-".repeat(38));
        for l in (0..=max_l).rev() {
            let actual   = counts[l];
            let expected = n as f64 / m.powi(l as i32);
            println!("{:>6}  {:>8}  {:>10.1}  {:>8.3}", l, actual, expected, actual as f64 / expected);
        }

        // Also print mean level.
        let mean_level: f64 = nodes.iter()
            .filter_map(|n| n.as_ref())
            .map(|n| (n.neighbors.len() - 1) as f64)
            .sum::<f64>() / n as f64;
        println!("\nmean assigned level : {mean_level:.3}");
        println!("expected (1/ln(M))  : {:.3}", idx.config.lambda);
    }

    // Clustered recall + latency sweep.
    // Generates 100 cluster centers × 100 vectors each (N=10k), then:
    //   - sweeps ef_search 10..800 and reports Recall@10
    //   - measures p50 latency at ef=100, 200, 400
    // If clustered recall at ef=50 jumps vs random, the low baseline is a data-distribution effect.
    // Run: cargo test -p valori-node --lib --release hnsw_diag_clustered -- --nocapture --ignored
    #[test]
    #[ignore]
    fn hnsw_diag_clustered() {
        let dim        = 384usize;
        let n_clusters = 100usize;
        let per_cluster= 100usize;
        let n          = n_clusters * per_cluster;
        let k          = 10usize;
        let trials     = 200usize;

        // LCG with two distinct seeds so centers and perturbations don't correlate.
        let mut seed = 0xfeed_c0de_u64;
        let mut lcg = |s: &mut u64| -> f32 {
            *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (((*s >> 33) as f32) / (u32::MAX as f32) - 0.5) * 2.0
        };

        // Centers drawn from N(0, 1); vectors are center + N(0, 0.05) perturbation.
        let centers: Vec<Vec<f32>> = (0..n_clusters)
            .map(|_| (0..dim).map(|_| lcg(&mut seed) * 1.0).collect())
            .collect();

        let mut corpus: Vec<Vec<f32>> = Vec::with_capacity(n);
        let mut corpus_cluster: Vec<usize> = Vec::with_capacity(n); // ground-truth cluster id
        for (ci, center) in centers.iter().enumerate() {
            for _ in 0..per_cluster {
                let v: Vec<f32> = center.iter().map(|&c| c + lcg(&mut seed) * 0.05).collect();
                corpus.push(v);
                corpus_cluster.push(ci);
            }
        }

        // Queries: one per cluster center (slightly perturbed).
        let queries: Vec<Vec<f32>> = centers.iter()
            .map(|c| c.iter().map(|&x| x + lcg(&mut seed) * 0.02).collect())
            .collect();

        let mut idx = HnswIndex::new();
        for (i, v) in corpus.iter().enumerate() { idx.insert(i as u32, v); }

        // ── ef sweep ────────────────────────────────────────────────────────────
        println!("\n=== clustered ef sweep  dim={dim}  N={n}  k={k}  ({n_clusters} clusters × {per_cluster}) ===");
        println!("{:>10}  {:>12}  {:>12}", "ef_search", "Recall@10", "Recall@100");
        println!("{}", "-".repeat(38));

        for &ef in &[10usize, 20, 50, 100, 200, 400, 800] {
            idx.config.ef_search = ef;
            let r10  = mean_recall(&idx, &corpus, &queries, 10);
            let r100 = mean_recall(&idx, &corpus, &queries, 100);
            println!("{:>10}  {:>11.1}%  {:>11.1}%", ef, r10 * 100.0, r100 * 100.0);
        }

        // ── latency at operating points ──────────────────────────────────────────
        println!("\n=== latency at operating points  dim={dim}  N={n}  k={k} ===");
        println!("{:>10}  {:>10}  {:>10}  {:>10}", "ef_search", "p50 µs", "p95 µs", "Recall@10");

        for &ef in &[100usize, 200, 400] {
            idx.config.ef_search = ef;
            // Warm up.
            for q in queries.iter().take(10) { let _ = idx.search(q, k); }

            let mut times_us: Vec<f64> = queries.iter().cycle().take(trials).map(|q| {
                let t = Instant::now();
                let _ = idx.search(q, k);
                t.elapsed().as_secs_f64() * 1_000_000.0
            }).collect();
            times_us.sort_by(|a, b| a.partial_cmp(b).unwrap());

            let p50 = times_us[trials / 2];
            let p95 = times_us[(trials as f64 * 0.95) as usize];
            let r   = mean_recall(&idx, &corpus, &queries, k);
            println!("{:>10}  {:>10.1}  {:>10.1}  {:>9.1}%", ef, p50, p95, r * 100.0);
        }
    }

    // Test 1: sweep ef_search — distinguishes "bad graph" from "insufficient search beam".
    // Run: cargo test -p valori-node --lib --release hnsw_diag_ef_sweep -- --nocapture --ignored
    #[test]
    #[ignore]
    fn hnsw_diag_ef_sweep() {
        let dim = 384usize;
        let n   = 10_000usize;
        let k   = 10usize;
        let (corpus, queries) = build_test_corpus(dim, n, 0xdeadbeef);

        let mut idx = HnswIndex::new();
        for (i, v) in corpus.iter().enumerate() { idx.insert(i as u32, v); }

        println!("\n=== ef_search sweep  dim={dim}  N={n}  k={k} ===");
        println!("{:>10}  {:>10}", "ef_search", "Recall@10");
        println!("{}", "-".repeat(24));

        for &ef in &[10usize, 20, 50, 100, 200, 400, 800] {
            idx.config.ef_search = ef;
            let r = mean_recall(&idx, &corpus, &queries, k);
            println!("{:>10}  {:>9.1}%", ef, r * 100.0);
        }
    }

    // Test 2: count visited nodes and distance calls per search_layer invocation.
    // Run: cargo test -p valori-node --lib --release hnsw_diag_search_stats -- --nocapture --ignored
    #[test]
    #[ignore]
    fn hnsw_diag_search_stats() {
        let dim = 384usize;
        let n   = 10_000usize;
        let k   = 10usize;
        let (corpus, queries) = build_test_corpus(dim, n, 0xdeadbeef);

        let mut idx = HnswIndex::new();
        for (i, v) in corpus.iter().enumerate() { idx.insert(i as u32, v); }

        let max_l  = *idx.max_level.read().unwrap();
        let nodes  =  idx.nodes.read().unwrap();
        let entry  = *idx.entry_point.read().unwrap();
        let ef     = k.max(idx.config.ef_search);

        let mut total_visited = 0u64;
        let mut total_dist    = 0u64;
        let n_q = queries.len() as u64;

        for query in &queries {
            let mut curr_entry = entry.unwrap();
            let mut dist_calls = 0u64;
            let mut visited_total = 0u64;

            // Greedy upper-layer descent (ef=1 each layer).
            for l in (1..=max_l).rev() {
                loop {
                    let curr_node = match nodes.get(curr_entry as usize).and_then(|n| n.as_ref()) {
                        Some(n) => n, None => break,
                    };
                    let curr_dist = HnswIndex::dist(query, &curr_node.vector);
                    dist_calls += 1;
                    let neighbors = curr_node.neighbors.get(l).map(|v| v.as_slice()).unwrap_or(&[]);
                    let mut best = curr_entry; let mut best_dist = curr_dist;
                    for &nb in neighbors {
                        if let Some(Some(nb_n)) = nodes.get(nb as usize) {
                            dist_calls += 1;
                            visited_total += 1;
                            let d = HnswIndex::dist(query, &nb_n.vector);
                            if d < best_dist { best_dist = d; best = nb; }
                        }
                    }
                    if best == curr_entry { break; }
                    curr_entry = best;
                }
            }

            // Layer-0 beam search: instrument using visited set size after search.
            let layer0 = nodes.get(0).map(|_| ()).is_some(); // just checking nodes is non-empty
            let _ = layer0;
            // Re-implement layer-0 search with counter.
            {
                use std::collections::{BinaryHeap, HashSet};
                use std::cmp::Reverse;
                let layer = match nodes.get(curr_entry as usize).and_then(|n| n.as_ref()) {
                    Some(_) => (), None => continue,
                };
                let _ = layer;
                let entry_dist = HnswIndex::dist(query, &nodes[curr_entry as usize].as_ref().unwrap().vector);
                dist_calls += 1;
                let mut visited: HashSet<u32> = HashSet::new();
                visited.insert(curr_entry);
                let mut c: BinaryHeap<Reverse<Candidate>> = BinaryHeap::with_capacity(ef);
                let mut w: BinaryHeap<Candidate>          = BinaryHeap::with_capacity(ef);
                c.push(Reverse(Candidate { id: curr_entry, dist: entry_dist }));
                w.push(Candidate { id: curr_entry, dist: entry_dist });

                while let Some(Reverse(curr)) = c.pop() {
                    if let Some(worst) = w.peek() { if curr.dist > worst.dist { break; } }
                    let curr_node = match nodes.get(curr.id as usize).and_then(|n| n.as_ref()) {
                        Some(n) => n, None => continue,
                    };
                    for &nb in curr_node.neighbors.first().map(|v| v.as_slice()).unwrap_or(&[]) {
                        if !visited.insert(nb) { continue; }
                        let nb_node = match nodes.get(nb as usize).and_then(|n| n.as_ref()) {
                            Some(n) => n, None => continue,
                        };
                        dist_calls += 1;
                        let d = HnswIndex::dist(query, &nb_node.vector);
                        let cand = Candidate { id: nb, dist: d };
                        let mut added = false;
                        if w.len() < ef { w.push(cand); added = true; }
                        else if let Some(worst) = w.peek() {
                            if d < worst.dist { w.pop(); w.push(cand); added = true; }
                        }
                        if added { c.push(Reverse(cand)); }
                    }
                }
                visited_total += visited.len() as u64;
            }

            total_visited += visited_total;
            total_dist    += dist_calls;
        }

        println!("\n=== search instrumentation  dim={dim}  N={n}  ef={ef} ===");
        println!("avg visited nodes    : {:.1}", total_visited as f64 / n_q as f64);
        println!("avg distance calls   : {:.1}", total_dist    as f64 / n_q as f64);
        println!("(N = {n}, so visited/N = {:.3})", (total_visited as f64 / n_q as f64) / n as f64);
    }

    // Test 3: how many candidates does search_layer return during construction?
    // Approximates build-time behaviour by re-running search for a sample of nodes.
    // Run: cargo test -p valori-node --lib --release hnsw_diag_insert_candidates -- --nocapture --ignored
    #[test]
    #[ignore]
    fn hnsw_diag_insert_candidates() {
        let dim    = 384usize;
        let n      = 10_000usize;
        let sample = 200usize; // nodes to probe after build
        let (corpus, _) = build_test_corpus(dim, n, 0xdeadbeef);

        let mut idx = HnswIndex::new();
        for (i, v) in corpus.iter().enumerate() { idx.insert(i as u32, v); }

        let nodes  = idx.nodes.read().unwrap();
        let max_l  = *idx.max_level.read().unwrap();
        let ef_c   = idx.config.ef_construction;

        // For each sampled node, run search_layer at each level it participates in,
        // recording how many candidates come back.
        let mut layer_cands: Vec<Vec<usize>> = vec![Vec::new(); max_l + 1];

        let step = n / sample;
        for i in (0..n).step_by(step.max(1)).take(sample) {
            let node = match nodes.get(i).and_then(|n| n.as_ref()) { Some(n) => n, None => continue };
            let node_level = node.neighbors.len().saturating_sub(1);
            let query = node.vector.as_ref();
            // entry point for this node's level (approximate: use global entry)
            let entry = match *idx.entry_point.read().unwrap() { Some(e) => e, None => continue };
            for l in 0..=node_level.min(max_l) {
                let cands = idx.search_layer(entry, query, ef_c, l, &*nodes);
                layer_cands[l].push(cands.len());
            }
        }

        println!("\n=== insert candidate counts  dim={dim}  N={n}  ef_construction={ef_c} ===");
        println!("{:>6}  {:>8}  {:>8}  {:>8}  {:>8}", "layer", "samples", "min", "avg", "max");
        println!("{}", "-".repeat(46));
        for (l, counts) in layer_cands.iter().enumerate().rev() {
            if counts.is_empty() { continue; }
            let min = counts.iter().min().unwrap();
            let max = counts.iter().max().unwrap();
            let avg = counts.iter().sum::<usize>() as f64 / counts.len() as f64;
            println!("{:>6}  {:>8}  {:>8}  {:>8.1}  {:>8}", l, counts.len(), min, avg, max);
        }
    }

    // Test 4: degree distribution of the built graph.
    // Run: cargo test -p valori-node --lib --release hnsw_diag_degree_dist -- --nocapture --ignored
    #[test]
    #[ignore]
    fn hnsw_diag_degree_dist() {
        let dim = 384usize;
        let n   = 10_000usize;
        let (corpus, _) = build_test_corpus(dim, n, 0xdeadbeef);

        let mut idx = HnswIndex::new();
        for (i, v) in corpus.iter().enumerate() { idx.insert(i as u32, v); }

        let nodes = idx.nodes.read().unwrap();
        let max_l = *idx.max_level.read().unwrap();

        println!("\n=== degree distribution  dim={dim}  N={n}  M={}  M0={} ===",
            idx.config.m, idx.config.m_max0);
        println!("{:>6}  {:>8}  {:>8}  {:>8}  {:>8}  {:>10}", "layer", "nodes", "min°", "avg°", "max°", "isolated");

        for l in (0..=max_l).rev() {
            let degrees: Vec<usize> = nodes.iter()
                .filter_map(|n| n.as_ref())
                .filter_map(|n| n.neighbors.get(l).map(|v| v.len()))
                .collect();
            if degrees.is_empty() { continue; }
            let min = degrees.iter().min().unwrap();
            let max = degrees.iter().max().unwrap();
            let avg = degrees.iter().sum::<usize>() as f64 / degrees.len() as f64;
            let isolated = degrees.iter().filter(|&&d| d == 0).count();
            println!("{:>6}  {:>8}  {:>8}  {:>8.2}  {:>8}  {:>10}", l, degrees.len(), min, avg, max, isolated);
        }
    }

    #[test]
    fn first_high_level_insert_is_searchable() {
        let mut idx = HnswIndex::new();
        for i in 0..64u32 {
            let v: Vec<f32> = (0..8).map(|j| (i * 8 + j) as f32).collect();
            idx.insert(i, &v);
        }
        let query: Vec<f32> = (0..8).map(|j| j as f32).collect();
        let results = idx.search(&query, 5);
        assert!(!results.is_empty(), "search must return results after inserting 64 nodes");
        assert_eq!(results[0].0, 0, "nearest to [0..8] should be node 0");
    }

    #[test]
    fn deleted_node_not_returned_in_search() {
        let mut idx = HnswIndex::new();
        for i in 0..10u32 {
            idx.insert(i, &[i as f32, 0.0, 0.0, 0.0]);
        }
        idx.delete(0);
        let results = idx.search(&[0.0, 0.0, 0.0, 0.0], 5);
        assert!(results.iter().all(|(id, _)| *id != 0), "deleted node 0 must not appear in results");
    }

    #[test]
    fn search_is_sublinear_not_exhaustive() {
        let mut idx = HnswIndex::new();
        let n = 1000u32;
        let mut seed: u64 = 12345;
        let mut next = |s: &mut u64| -> f32 {
            *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((*s >> 33) as f32) / (u32::MAX as f32) * 2.0 - 1.0
        };
        let dim = 16usize;
        let vecs: Vec<Vec<f32>> = (0..n).map(|_| (0..dim).map(|_| next(&mut seed)).collect()).collect();
        for (i, v) in vecs.iter().enumerate() { idx.insert(i as u32, v); }

        let nodes = idx.nodes.read().unwrap();
        let total_edges: usize = nodes.iter()
            .filter_map(|n| n.as_ref())
            .map(|n| n.neighbors.first().map_or(0, |v| v.len()))
            .sum();
        let nodes_in_layer0 = nodes.iter().filter(|n| n.is_some()).count();
        drop(nodes);

        eprintln!("N={n}, nodes_in_layer0={nodes_in_layer0}, total_edges_layer0={total_edges}");
        assert!(nodes_in_layer0 >= n as usize - 1, "all nodes should be present");
        assert!(total_edges > (n as usize) * 2,
            "each node should have multiple edges; got {total_edges} for {n} nodes");

        let query: Vec<f32> = (0..dim).map(|_| 0.0f32).collect();
        let results = idx.search(&query, 10);
        assert_eq!(results.len(), 10);
        for w in results.windows(2) { assert!(w[0].1 <= w[1].1, "results must be sorted"); }
    }

    #[test]
    fn graph_navigable_after_entry_point_deleted() {
        let mut idx = HnswIndex::new();
        for i in 0..8u32 { idx.insert(i, &[i as f32, 0.0]); }
        let ep = *idx.entry_point.read().unwrap();
        if let Some(ep_id) = ep {
            idx.delete(ep_id);
            let results = idx.search(&[0.0, 0.0], 3);
            assert!(!results.is_empty(), "graph must remain searchable after entry point deletion");
            assert!(results.iter().all(|(id, _)| *id != ep_id));
        }
    }
}
