// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::deterministic::kmeans::{
    deterministic_kmeans, f32_to_q16, l2_sq_q16 as _l2_sq_q16_scalar,
};
use crate::traits::VectorIndex;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn l2_sq_neon(a: &[i32], b: &[i32]) -> i64 {
    use std::arch::aarch64::*;
    let len = a.len();
    let mut pa = a.as_ptr();
    let mut pb = b.as_ptr();
    let mut acc0 = vdupq_n_s64(0);
    let mut acc1 = vdupq_n_s64(0);
    let mut acc2 = vdupq_n_s64(0);
    let mut acc3 = vdupq_n_s64(0);
    let chunks8 = len / 8;
    for _ in 0..chunks8 {
        let d0 = vsubq_s32(vld1q_s32(pa), vld1q_s32(pb));
        let d1 = vsubq_s32(vld1q_s32(pa.add(4)), vld1q_s32(pb.add(4)));
        acc0 = vmlal_s32(acc0, vget_low_s32(d0), vget_low_s32(d0));
        acc1 = vmlal_s32(acc1, vget_high_s32(d0), vget_high_s32(d0));
        acc2 = vmlal_s32(acc2, vget_low_s32(d1), vget_low_s32(d1));
        acc3 = vmlal_s32(acc3, vget_high_s32(d1), vget_high_s32(d1));
        pa = pa.add(8);
        pb = pb.add(8);
    }
    let combined = vaddq_s64(vaddq_s64(acc0, acc1), vaddq_s64(acc2, acc3));
    let mut sum = vgetq_lane_s64::<0>(combined) + vgetq_lane_s64::<1>(combined);
    let base = chunks8 * 8;
    for i in base..len {
        let d = *a.get_unchecked(i) as i64 - *b.get_unchecked(i) as i64;
        sum += d * d;
    }
    sum
}

#[inline(always)]
fn l2_sq(a: &[i32], b: &[i32]) -> i64 {
    #[cfg(target_arch = "aarch64")]
    return unsafe { l2_sq_neon(a, b) };
    #[cfg(not(target_arch = "aarch64"))]
    _l2_sq_q16_scalar(a, b)
}

thread_local! {
    static CENTROID_SCRATCH: RefCell<Vec<(usize, i64)>> = RefCell::new(Vec::new());
    static CANDIDATE_SCRATCH: RefCell<Vec<(u32, i64)>>  = RefCell::new(Vec::new());
}

#[derive(Serialize, Deserialize, Clone)]
pub struct IvfConfig {
    pub n_list: usize,
    pub n_probe: usize,
    #[serde(default = "default_auto_scale")]
    pub auto_scale: bool,
}

fn default_auto_scale() -> bool {
    true
}

impl Default for IvfConfig {
    fn default() -> Self {
        Self {
            n_list: 100,
            n_probe: 10,
            auto_scale: true,
        }
    }
}

pub struct IvfIndex {
    pub config: IvfConfig,
    pub dim: usize,
    pub centroids: Vec<Vec<i32>>,
    pub inverted_lists: Vec<Vec<(u32, Vec<i32>)>>,
    pub n_at_last_build: usize,
}

impl IvfIndex {
    pub fn new(config: IvfConfig, dim: usize) -> Self {
        Self {
            config,
            dim,
            centroids: Vec::new(),
            inverted_lists: Vec::new(),
            n_at_last_build: 0,
        }
    }

    pub fn needs_rebuild(&self, current_count: usize) -> bool {
        self.n_at_last_build > 0 && current_count > self.n_at_last_build * 2
    }

    fn effective_params(config: &IvfConfig, n: usize) -> (usize, usize) {
        if !config.auto_scale {
            return (config.n_list, config.n_probe);
        }
        let n_list = ((n as f64).sqrt() as usize).max(16);
        let n_probe = ((n_list as f64).sqrt() as usize).max(1);
        (n_list, n_probe)
    }

    fn find_nearest_centroid(&self, q_vec: &[i32]) -> (usize, i64) {
        if self.centroids.is_empty() {
            return (0, i64::MAX);
        }
        let mut best_idx = 0usize;
        let mut best_dist = i64::MAX;
        for (i, c) in self.centroids.iter().enumerate() {
            let d = l2_sq(q_vec, c);
            if d < best_dist || (d == best_dist && i < best_idx) {
                best_dist = d;
                best_idx = i;
            }
        }
        (best_idx, best_dist)
    }
}

impl VectorIndex for IvfIndex {
    fn build(&mut self, records: &[(u32, Vec<f32>)]) {
        if records.is_empty() {
            return;
        }

        let (eff_n_list, eff_n_probe) = Self::effective_params(&self.config, records.len());
        self.config.n_list = eff_n_list;
        self.config.n_probe = eff_n_probe;

        self.centroids = deterministic_kmeans(records, eff_n_list, 20);
        self.inverted_lists = vec![Vec::new(); self.centroids.len()];
        for (id, vec) in records {
            let q_vec: Vec<i32> = vec.iter().map(|&v| f32_to_q16(v)).collect();
            let (c_idx, _) = self.find_nearest_centroid(&q_vec);
            self.inverted_lists[c_idx].push((*id, q_vec));
        }

        self.n_at_last_build = records.len();
    }

    fn insert(&mut self, id: u32, vec: &[f32]) {
        let q_vec: Vec<i32> = vec.iter().map(|&v| f32_to_q16(v)).collect();
        if self.centroids.is_empty() {
            if self.inverted_lists.is_empty() {
                self.inverted_lists.push(Vec::new());
            }
            self.inverted_lists[0].push((id, q_vec));
            return;
        }
        let (c_idx, _) = self.find_nearest_centroid(&q_vec);
        self.inverted_lists[c_idx].push((id, q_vec));
    }

    fn delete(&mut self, id: u32) {
        for list in self.inverted_lists.iter_mut() {
            list.retain(|(rid, _)| *rid != id);
        }
    }

    fn search(&self, query: &[f32], k: usize) -> Vec<(u32, f32)> {
        let q_query: Vec<i32> = query.iter().map(|&v| f32_to_q16(v)).collect();

        if self.centroids.is_empty() {
            if self.inverted_lists.is_empty() {
                return Vec::new();
            }
            return CANDIDATE_SCRATCH.with(|cell| {
                let mut candidates = cell.borrow_mut();
                candidates.clear();
                candidates.extend(
                    self.inverted_lists[0]
                        .iter()
                        .map(|(id, q_vec)| (*id, l2_sq(&q_query, q_vec))),
                );
                if candidates.len() > k {
                    candidates.select_nth_unstable_by(k - 1, |a, b| {
                        a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0))
                    });
                    candidates.truncate(k);
                }
                candidates.sort_unstable_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
                candidates.iter().map(|(id, d)| (*id, *d as f32)).collect()
            });
        }

        CENTROID_SCRATCH.with(|ccell| {
            CANDIDATE_SCRATCH.with(|qcell| {
                let mut centroid_dists = ccell.borrow_mut();
                let mut candidates = qcell.borrow_mut();

                centroid_dists.clear();
                centroid_dists.extend(
                    self.centroids
                        .iter()
                        .enumerate()
                        .map(|(i, c)| (i, l2_sq(&q_query, c))),
                );

                let probes = self.config.n_probe.min(centroid_dists.len());
                if probes < centroid_dists.len() {
                    centroid_dists.select_nth_unstable_by(probes - 1, |a, b| {
                        a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0))
                    });
                }

                candidates.clear();
                for i in 0..probes {
                    let c_idx = centroid_dists[i].0;
                    for (id, q_vec) in &self.inverted_lists[c_idx] {
                        candidates.push((*id, l2_sq(&q_query, q_vec)));
                    }
                }

                if candidates.len() > k {
                    candidates.select_nth_unstable_by(k - 1, |a, b| {
                        a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0))
                    });
                    candidates.truncate(k);
                }
                candidates.sort_unstable_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

                candidates.iter().map(|(id, d)| (*id, *d as f32)).collect()
            })
        })
    }

    fn snapshot(&self) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        #[derive(Serialize)]
        struct IvfDump<'a> {
            config: &'a IvfConfig,
            centroids: &'a Vec<Vec<i32>>,
            inverted_lists: Vec<Vec<(u32, &'a Vec<i32>)>>,
        }

        let sorted_lists: Vec<Vec<(u32, &Vec<i32>)>> = self
            .inverted_lists
            .iter()
            .map(|list| {
                let mut refs: Vec<(u32, &Vec<i32>)> = list.iter().map(|(id, v)| (*id, v)).collect();
                refs.sort_by_key(|(id, _)| *id);
                refs
            })
            .collect();

        let dump = IvfDump {
            config: &self.config,
            centroids: &self.centroids,
            inverted_lists: sorted_lists,
        };

        Ok(bincode::serde::encode_to_vec(
            &dump,
            bincode::config::standard(),
        )?)
    }

    fn restore(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[derive(Deserialize)]
        struct IvfLoad {
            config: IvfConfig,
            centroids: Vec<Vec<i32>>,
            inverted_lists: Vec<Vec<(u32, Vec<i32>)>>,
        }
        let dump: IvfLoad = bincode::serde::decode_from_slice(data, bincode::config::standard())?.0;
        let total: usize = dump.inverted_lists.iter().map(|l| l.len()).sum();
        self.config = dump.config;
        self.centroids = dump.centroids;
        self.inverted_lists = dump.inverted_lists;
        self.dim = if self.centroids.is_empty() {
            0
        } else {
            self.centroids[0].len()
        };
        self.n_at_last_build = total;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_corpus(n: usize, dim: usize) -> Vec<(u32, Vec<f32>)> {
        (0..n as u32)
            .map(|i| (i, (0..dim).map(|d| (i as f32 + d as f32) * 0.01).collect()))
            .collect()
    }

    #[test]
    fn build_and_search_returns_k() {
        let corpus = make_corpus(200, 8);
        let mut idx = IvfIndex::new(IvfConfig::default(), 8);
        idx.build(&corpus);
        let res = idx.search(&[0.0f32; 8], 5);
        assert_eq!(res.len(), 5);
    }

    #[test]
    fn delete_removes_from_results() {
        let corpus = make_corpus(50, 4);
        let mut idx = IvfIndex::new(IvfConfig::default(), 4);
        idx.build(&corpus);
        idx.delete(0);
        let res = idx.search(&[0.0f32; 4], 10);
        assert!(res.iter().all(|(id, _)| *id != 0));
    }

    #[test]
    fn snapshot_restore_roundtrip() {
        let corpus = make_corpus(100, 4);
        let mut idx = IvfIndex::new(IvfConfig::default(), 4);
        idx.build(&corpus);
        let snap = idx.snapshot().unwrap();
        let mut idx2 = IvfIndex::new(IvfConfig::default(), 4);
        idx2.restore(&snap).unwrap();
        let r1 = idx.search(&[0.1, 0.2, 0.3, 0.4], 5);
        let r2 = idx2.search(&[0.1, 0.2, 0.3, 0.4], 5);
        assert_eq!(
            r1.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            r2.iter().map(|(id, _)| *id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn needs_rebuild_triggers_at_2x() {
        let mut idx = IvfIndex::new(IvfConfig::default(), 4);
        idx.n_at_last_build = 100;
        assert!(!idx.needs_rebuild(100));
        assert!(!idx.needs_rebuild(200));
        assert!(idx.needs_rebuild(201));
    }
}
