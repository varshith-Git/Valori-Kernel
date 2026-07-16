// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::traits::VectorIndex;
use std::cmp::Ordering;
use std::sync::RwLock;
use serde::{Serialize, Deserialize};
use rustc_hash::FxHashSet;

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

struct Node {
    vector: Box<[f32]>,
    neighbors: Vec<Vec<u32>>,
}

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

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn dist_neon(v1: &[f32], v2: &[f32]) -> f32 {
    use std::arch::aarch64::*;

    let len = v1.len();
    let mut p1 = v1.as_ptr();
    let mut p2 = v2.as_ptr();

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

    acc0 = vaddq_f32(vaddq_f32(acc0, acc1), vaddq_f32(acc2, acc3));

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

    let base = chunks16 * 16 + rem4 * 4;
    for i in base..len {
        let d = *v1.get_unchecked(i) - *v2.get_unchecked(i);
        sum += d * d;
    }
    sum
}

pub struct HnswIndex {
    config: HnswConfig,
    nodes: RwLock<Vec<Option<Node>>>,
    entry_point: RwLock<Option<u32>>,
    max_level: RwLock<usize>,
}

#[inline]
fn ensure_node_slot(nodes: &mut Vec<Option<Node>>, idx: usize) {
    if idx >= nodes.len() {
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
    pub(crate) fn dist(v1: &[f32], v2: &[f32]) -> f32 {
        #[cfg(target_arch = "aarch64")]
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

        let mut visited = FxHashSet::with_capacity_and_hasher(ef * 2, Default::default());
        visited.insert(entry);

        use std::collections::BinaryHeap;
        use std::cmp::Reverse;

        let mut c = BinaryHeap::with_capacity(ef);
        c.push(Reverse(Candidate { id: entry, dist }));

        let mut w = BinaryHeap::with_capacity(ef);
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

        let sorted = w.into_sorted_vec();
        tracing::debug!(level, visited = visited.len(), found = sorted.len(), ef, "search_layer done");
        sorted
    }

    fn select_neighbors_heuristic(
        &self,
        query: &[f32],
        candidates: &[Candidate],
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

impl Default for HnswIndex {
    fn default() -> Self { Self::new() }
}

impl VectorIndex for HnswIndex {
    fn build(&mut self, records: &[(u32, Vec<f32>)]) {
        for (id, vec) in records { self.insert(*id, vec); }
    }

    fn insert(&mut self, id: u32, vector: &[f32]) {
        let level = self.deterministic_level(id);
        let curr_entry = *self.entry_point.read().unwrap();

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

        {
            let mut nodes = self.nodes.write().unwrap();
            ensure_node_slot(&mut nodes, id as usize);
            nodes[id as usize] = Some(Node {
                vector: vector.to_vec().into_boxed_slice(),
                neighbors: (0..=level).map(|l| Vec::with_capacity(if l == 0 { self.config.m_max0 } else { self.config.m })).collect(),
            });
        }

        {
            let mut max_l = self.max_level.write().unwrap();
            if level > *max_l {
                *max_l = level;
                *self.entry_point.write().unwrap() = Some(id);
            }
        }

        let max_l = *self.max_level.read().unwrap();
        let mut curr_entry_id = curr_entry.unwrap();

        let mut nodes = self.nodes.write().unwrap();

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

        for l in (0..=level).rev() {
            let m = if l == 0 { self.config.m_max0 } else { self.config.m };

            let candidates = self.search_layer(curr_entry_id, vector, self.config.ef_construction, l, &*nodes);
            let neighbors = self.select_neighbors_heuristic(vector, &candidates, m, &*nodes, true);

            if let Some(Some(node)) = nodes.get_mut(id as usize) {
                if l < node.neighbors.len() {
                    node.neighbors[l] = neighbors.clone();
                }
            }

            for &nb_id in &neighbors {
                let nb_uid = nb_id as usize;
                if let Some(Some(nb_node)) = nodes.get_mut(nb_uid) {
                    if let Some(edges) = nb_node.neighbors.get_mut(l) {
                        edges.push(id);
                        if edges.len() > m {
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

    #[test]
    fn first_high_level_insert_is_searchable() {
        let mut idx = HnswIndex::new();
        for i in 0..64u32 {
            let v: Vec<f32> = (0..8).map(|j| (i * 8 + j) as f32).collect();
            idx.insert(i, &v);
        }
        let query: Vec<f32> = (0..8).map(|j| j as f32).collect();
        let results = idx.search(&query, 5);
        assert!(!results.is_empty());
        assert_eq!(results[0].0, 0);
    }

    #[test]
    fn deleted_node_not_returned_in_search() {
        let mut idx = HnswIndex::new();
        for i in 0..10u32 {
            idx.insert(i, &[i as f32, 0.0, 0.0, 0.0]);
        }
        idx.delete(0);
        let results = idx.search(&[0.0, 0.0, 0.0, 0.0], 5);
        assert!(results.iter().all(|(id, _)| *id != 0));
    }

    #[test]
    fn graph_navigable_after_entry_point_deleted() {
        let mut idx = HnswIndex::new();
        for i in 0..8u32 { idx.insert(i, &[i as f32, 0.0]); }
        let ep = *idx.entry_point.read().unwrap();
        if let Some(ep_id) = ep {
            idx.delete(ep_id);
            let results = idx.search(&[0.0, 0.0], 3);
            assert!(!results.is_empty());
            assert!(results.iter().all(|(id, _)| *id != ep_id));
        }
    }

    #[test]
    fn snapshot_restore_roundtrip() {
        let mut idx = HnswIndex::new();
        for i in 0..20u32 {
            idx.insert(i, &[i as f32, 0.0, 0.0, 0.0]);
        }
        let snap = idx.snapshot().unwrap();
        let mut idx2 = HnswIndex::new();
        idx2.restore(&snap).unwrap();
        let r1 = idx.search(&[0.0, 0.0, 0.0, 0.0], 3);
        let r2 = idx2.search(&[0.0, 0.0, 0.0, 0.0], 3);
        assert_eq!(r1.iter().map(|(id,_)| *id).collect::<Vec<_>>(),
                   r2.iter().map(|(id,_)| *id).collect::<Vec<_>>());
    }
}
