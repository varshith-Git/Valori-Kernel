// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Time-decay re-ranking for recency-aware recall.
//!
//! Decay is a **read-time re-rank**. It never mutates kernel state, never emits
//! a committed event, and never affects the BLAKE3 state hash. The
//! `as_of_state_hash` a search returns is identical with or without decay.
//!
//! # Model
//!
//! Each record has a creation time (unix seconds). Its `age = now − created_at`.
//! The decay weight is a geometric half-life:
//!
//! ```text
//! factor(age) = 0.5 ^ (age / half_life)      // 1.0 at age 0, 0.5 at one half-life
//! ```
//!
//! Kernel scores are **L2 distances** (lower is better), so an older record is
//! penalised by *inflating* its distance: `adjusted = distance / factor`. A
//! fresh, slightly-worse match can therefore overtake an old, slightly-better one.
//!
//! Records whose creation time is unknown are treated as age 0 — neutral. We
//! never penalise what we cannot date.

/// Multiplicative age weight in `(0, 1]`.
///
/// `age == 0` → `1.0` (no penalty); `age == half_life` → `0.5`; geometric beyond.
/// A `half_life_secs` of `0` disables decay (returns `1.0` unconditionally).
#[inline]
pub fn decay_factor(age_secs: u64, half_life_secs: u64) -> f64 {
    if half_life_secs == 0 {
        return 1.0;
    }
    0.5f64.powf(age_secs as f64 / half_life_secs as f64)
}

/// Smallest factor we divide by, so an ancient record never produces `inf` / `NaN`.
const FACTOR_FLOOR: f64 = 1e-9;

/// A scored candidate entering the decay re-ranker.
#[derive(Clone, Copy, Debug)]
pub struct DecayHit {
    pub id: u32,
    /// L2 distance from the query — lower is better.
    pub distance: f32,
    /// Unix-second creation time. `None` = unknown → treated as age 0 (neutral).
    pub created_at: Option<u64>,
}

/// The result of applying decay to one hit.
#[derive(Clone, Copy, Debug)]
pub struct DecayedHit {
    pub id: u32,
    /// Original undecayed distance — preserved for auditability.
    pub distance: f32,
    /// Applied decay factor in `(0, 1]`.
    pub factor: f32,
    /// Age in seconds at the time of the search, if `created_at` was known.
    pub age_secs: Option<u64>,
    /// Internal ordering key: `distance / factor`. Lower ranks first.
    adjusted: f64,
}

/// Re-rank `hits` by decayed distance and return the top `k`.
///
/// `now` is the reference time (unix seconds). A record with `created_at == None`
/// or a `created_at` in the future relative to `now` is treated as age 0.
///
/// Ordering is ascending by adjusted distance; ties are broken by `id` ascending
/// for deterministic, stable output across identical inputs.
pub fn rerank(hits: Vec<DecayHit>, now: u64, half_life_secs: u64, k: usize) -> Vec<DecayedHit> {
    let mut out: Vec<DecayedHit> = hits
        .into_iter()
        .map(|h| {
            let age = match h.created_at {
                Some(ts) if now >= ts => now - ts,
                _ => 0,
            };
            let factor = decay_factor(age, half_life_secs);
            let adjusted = h.distance as f64 / factor.max(FACTOR_FLOOR);
            DecayedHit {
                id: h.id,
                distance: h.distance,
                factor: factor as f32,
                age_secs: h.created_at.map(|_| age),
                adjusted,
            }
        })
        .collect();

    out.sort_by(|a, b| {
        a.adjusted
            .partial_cmp(&b.adjusted)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.id.cmp(&b.id))
    });
    out.truncate(k);
    out
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factor_endpoints() {
        assert!((decay_factor(0, 100) - 1.0).abs() < 1e-12);
        assert!((decay_factor(100, 100) - 0.5).abs() < 1e-12);
        assert!((decay_factor(200, 100) - 0.25).abs() < 1e-12);
        assert_eq!(decay_factor(999, 0), 1.0, "half_life 0 disables decay");
    }

    #[test]
    fn fresh_beats_old_when_half_life_short() {
        let now = 1_000;
        let hits = vec![
            DecayHit {
                id: 1,
                distance: 1.0,
                created_at: Some(now - 100),
            },
            DecayHit {
                id: 2,
                distance: 1.4,
                created_at: Some(now),
            },
        ];
        let ranked = rerank(hits, now, 100, 2);
        // id 1: adjusted = 1.0 / 0.5 = 2.0  |  id 2: adjusted = 1.4 / 1.0 = 1.4
        assert_eq!(ranked[0].id, 2);
        assert_eq!(ranked[1].id, 1);
        assert_eq!(ranked[1].distance, 1.0, "original distance preserved");
        assert!((ranked[1].factor - 0.5).abs() < 1e-6);
    }

    #[test]
    fn unknown_age_is_neutral() {
        let now = 1_000;
        let hits = vec![
            DecayHit {
                id: 1,
                distance: 1.0,
                created_at: None,
            },
            DecayHit {
                id: 2,
                distance: 2.0,
                created_at: None,
            },
        ];
        let ranked = rerank(hits, now, 10, 2);
        assert_eq!(ranked[0].id, 1, "unknown ages keep pure distance order");
        assert_eq!(ranked[0].factor, 1.0);
        assert!(ranked[0].age_secs.is_none());
    }

    #[test]
    fn future_timestamp_not_penalised() {
        let now = 1_000;
        let hits = vec![DecayHit {
            id: 7,
            distance: 0.5,
            created_at: Some(now + 500),
        }];
        let ranked = rerank(hits, now, 10, 1);
        assert_eq!(ranked[0].factor, 1.0);
        assert_eq!(ranked[0].age_secs, Some(0));
    }

    #[test]
    fn huge_half_life_preserves_distance_order() {
        let now = 1_000_000;
        let hits = vec![
            DecayHit {
                id: 1,
                distance: 3.0,
                created_at: Some(0),
            },
            DecayHit {
                id: 2,
                distance: 1.0,
                created_at: Some(0),
            },
            DecayHit {
                id: 3,
                distance: 2.0,
                created_at: Some(now),
            },
        ];
        let ranked = rerank(hits, now, 100 * 365 * 24 * 3600, 3);
        assert_eq!(
            ranked.iter().map(|h| h.id).collect::<Vec<_>>(),
            vec![2, 3, 1]
        );
    }

    #[test]
    fn truncates_to_k_and_is_stable() {
        let now = 100;
        let hits = vec![
            DecayHit {
                id: 5,
                distance: 1.0,
                created_at: Some(now),
            },
            DecayHit {
                id: 2,
                distance: 1.0,
                created_at: Some(now),
            },
            DecayHit {
                id: 9,
                distance: 1.0,
                created_at: Some(now),
            },
        ];
        let ranked = rerank(hits, now, 10, 2);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].id, 2, "stable tie-break by id ascending");
        assert_eq!(ranked[1].id, 5);
    }

    #[test]
    fn exact_match_stays_best_regardless_of_age() {
        let now = 1_000;
        let hits = vec![
            DecayHit {
                id: 1,
                distance: 0.0,
                created_at: Some(0),
            },
            DecayHit {
                id: 2,
                distance: 0.1,
                created_at: Some(now),
            },
        ];
        let ranked = rerank(hits, now, 10, 2);
        assert_eq!(ranked[0].id, 1, "distance 0 / factor == 0, always best");
    }
}
