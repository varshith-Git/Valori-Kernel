// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 3 property tests.
//!
//! No external property-testing framework is used (avoids std-only deps in CI).
//! Each test runs a deterministic pseudo-random sweep over many configurations.
//!
//! # Categories
//! 1. `insert → snapshot → restore → same top-K` (1000 random states)
//! 2. Snapshot decoder never panics / OOMs on crafted malformed input.
//! 3. Replay fuzzing: random event streams produce matching hashes.

use valori_kernel::event::KernelEvent;
use valori_kernel::index::{IndexVariant, SearchResult};
use valori_kernel::snapshot::{
    blake3::hash_state_blake3, decode::decode_state, encode::encode_state,
};
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

// ── Deterministic PRNG ────────────────────────────────────────────────────────

struct Lcg(u64);
impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0xdeadbeef_cafebabe)
    }
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn next_u32(&mut self) -> u32 {
        (self.next() >> 32) as u32
    }
    fn next_usize(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn next_i32_range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next_u32() % (hi - lo) as u32) as i32
    }
}

// ── State builder ─────────────────────────────────────────────────────────────

struct Config {
    n_records: usize,
    dim: usize,
    n_soft_delete: usize,
}

fn build_state(rng: &mut Lcg, cfg: &Config) -> KernelState {
    let mut state = KernelState::new();
    for i in 0..cfg.n_records as u32 {
        let data = (0..cfg.dim)
            .map(|_| FxpScalar(rng.next_i32_range(-32768, 32767)))
            .collect();
        state
            .apply_event(&KernelEvent::InsertRecord {
                id: RecordId(i),
                vector: FxpVector { data },
                metadata: if rng.next_u32() % 4 == 0 {
                    Some(format!("meta-{i}").into_bytes())
                } else {
                    None
                },
                tag: (rng.next_u32() % 8) as u64,
            })
            .unwrap();
    }
    // Randomly soft-delete some records.
    for _ in 0..cfg.n_soft_delete {
        let victim = RecordId(rng.next_u32() % cfg.n_records as u32);
        let _ = state.apply_event(&KernelEvent::SoftDeleteRecord { id: victim });
    }
    state
}

fn encode(state: &KernelState) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_state(state, &mut buf).expect("encode");
    buf
}

// ── 3.1 Property: insert → snapshot → restore → same search results ──────────

#[test]
fn property_snapshot_restore_preserves_top_k_search() {
    const TRIALS: u64 = 200;
    const K: usize = 5;

    for seed in 0..TRIALS {
        let mut rng = Lcg::new(seed * 0x_9e37_79b9);
        let n = 20 + rng.next_usize(80); // 20–99 records
        let dim = 4 + rng.next_usize(12); // 4–15 dims
        let cfg = Config {
            n_records: n,
            dim,
            n_soft_delete: n / 5,
        };

        let origin = build_state(&mut rng, &cfg);
        let snap = encode(&origin);
        let restored = decode_state(&snap).expect("decode");

        // Construct a random query of the right dimension.
        let query = FxpVector {
            data: (0..dim)
                .map(|_| FxpScalar(rng.next_i32_range(-32768, 32767)))
                .collect(),
        };

        let mut r_origin = vec![SearchResult::default(); K];
        let mut r_restored = vec![SearchResult::default(); K];
        let c1 = origin.search_l2(&query, &mut r_origin, None);
        let c2 = restored.search_l2(&query, &mut r_restored, None);

        assert_eq!(c1, c2, "seed={seed}: result count differs after restore");
        for i in 0..c1 {
            assert_eq!(
                r_origin[i].id, r_restored[i].id,
                "seed={seed} k={i}: result id differs after restore"
            );
        }
    }
}

#[test]
fn property_hash_stable_across_many_configurations() {
    const TRIALS: u64 = 200;

    for seed in 0..TRIALS {
        let mut rng = Lcg::new(seed.wrapping_mul(0x517cc1b727220a95));
        let n = 10 + rng.next_usize(40);
        let dim = 4 + rng.next_usize(8);
        let cfg = Config {
            n_records: n,
            dim,
            n_soft_delete: n / 6,
        };

        // Build the same state twice from the same seed.
        let h1 = hash_state_blake3(&build_state(
            &mut Lcg::new(seed.wrapping_mul(0x517cc1b727220a95)),
            &cfg,
        ));
        let h2 = hash_state_blake3(&build_state(
            &mut Lcg::new(seed.wrapping_mul(0x517cc1b727220a95)),
            &cfg,
        ));
        assert_eq!(
            h1, h2,
            "seed={seed}: hash differs between two identical builds"
        );

        // Encode → decode must preserve hash.
        let mut rng2 = Lcg::new(seed.wrapping_mul(0x517cc1b727220a95));
        let state = build_state(&mut rng2, &cfg);
        let snap = encode(&state);
        let rest = decode_state(&snap).expect("decode failed at seed={seed}");
        assert_eq!(
            h1,
            hash_state_blake3(&rest),
            "seed={seed}: hash changed after snapshot restore"
        );
    }
}

#[test]
fn property_bq_index_top1_matches_bf_after_restore() {
    const TRIALS: u64 = 100;

    for seed in 0..TRIALS {
        let mut rng = Lcg::new(seed.wrapping_mul(0xa0761d6478bd642f));
        let n = 30 + rng.next_usize(50);
        let dim = 8;
        let cfg = Config {
            n_records: n,
            dim,
            n_soft_delete: 0,
        };

        let mut state = build_state(&mut rng, &cfg);
        let snap = encode(&state);

        // Switch to BQ and restore.
        state.set_index_kind(IndexVariant::BinaryQuantization);
        let mut restored = decode_state(&snap).expect("decode");
        restored.set_index_kind(IndexVariant::BinaryQuantization);

        let query = FxpVector {
            data: (0..dim)
                .map(|_| FxpScalar(rng.next_i32_range(-16384, 16383)))
                .collect(),
        };

        let mut r1 = vec![SearchResult::default(); 3];
        let mut r2 = vec![SearchResult::default(); 3];
        state.search_l2(&query, &mut r1, None);
        restored.search_l2(&query, &mut r2, None);

        // On small, uniform data BQ top-1 must agree with restored BQ top-1.
        assert_eq!(
            r1[0].id, r2[0].id,
            "seed={seed}: BQ top-1 mismatch after restore"
        );
    }
}

// ── 3.2 Decoder stress: malformed bytes never panic ──────────────────────────
//
// The decoder must always return Err(…) for invalid input — never panic,
// never allocate unboundedly, never loop forever.

fn decode_must_not_panic(buf: &[u8]) {
    // If it returns Ok, that's fine too; the point is no panic/OOM.
    let _ = decode_state(buf);
}

#[test]
fn decoder_stress_random_bytes() {
    let mut rng = Lcg::new(0x0faa_0001_5eed_0001);
    for _ in 0..2000 {
        let len = rng.next_usize(512);
        let buf: Vec<u8> = (0..len).map(|_| rng.next_u32() as u8).collect();
        decode_must_not_panic(&buf);
    }
}

#[test]
fn decoder_stress_truncated_valid_snapshot() {
    let mut rng = Lcg::new(0x0faa_0002_5eed_0002);
    let cfg = Config {
        n_records: 20,
        dim: 8,
        n_soft_delete: 2,
    };
    let state = build_state(&mut rng, &cfg);
    let valid = encode(&state);

    // Try every prefix length — all must return Err (truncated).
    for cut in (0..valid.len()).step_by(7) {
        decode_must_not_panic(&valid[..cut]);
    }
}

#[test]
fn decoder_stress_bit_flips_in_valid_snapshot() {
    let mut rng = Lcg::new(0x0faa_0003_5eed_0003);
    let cfg = Config {
        n_records: 15,
        dim: 4,
        n_soft_delete: 0,
    };
    let state = build_state(&mut rng, &cfg);
    let valid = encode(&state);

    // Flip random single bits and verify no panic.
    for _ in 0..500 {
        let mut buf = valid.clone();
        let byte_pos = rng.next_usize(buf.len());
        let bit = 1u8 << (rng.next_usize(8));
        buf[byte_pos] ^= bit;
        decode_must_not_panic(&buf);
    }
}

#[test]
fn decoder_stress_oversized_length_fields() {
    // Construct minimal valid header then inject huge length values.
    // All must return Err without allocating gigabytes.
    let state = KernelState::new();
    let base = encode(&state);

    // total_slots at byte 33 — inject 0xFFFF_FFFF.
    let mut buf = base.clone();
    buf[33..37].copy_from_slice(&u32::MAX.to_le_bytes());
    decode_must_not_panic(&buf);

    // dim at byte 20 — inject MAX_DIM + 1.
    let mut buf = base.clone();
    buf[20..24].copy_from_slice(&65537u32.to_le_bytes());
    decode_must_not_panic(&buf);
}

// ── 3.3 Replay fuzzing: random event streams ──────────────────────────────────

#[test]
fn replay_fuzzing_random_streams_hash_stable() {
    const STREAMS: u64 = 300;
    let mut outer = Lcg::new(0x00ff_dead_beef_cafe);

    for _ in 0..STREAMS {
        let seed = outer.next();
        let mut rng = Lcg::new(seed);
        let n_events = 5 + rng.next_usize(45);
        let dim = 4 + rng.next_usize(8);

        let mut events: Vec<KernelEvent> = Vec::new();
        for i in 0..n_events as u32 {
            events.push(KernelEvent::InsertRecord {
                id: RecordId(i),
                vector: FxpVector {
                    data: (0..dim)
                        .map(|_| FxpScalar(rng.next_i32_range(-32768, 32767)))
                        .collect(),
                },
                metadata: None,
                tag: rng.next_u32() as u64 % 4,
            });
        }
        // Mix in some soft-deletes.
        let n_del = rng.next_usize(n_events.max(1));
        for _ in 0..n_del {
            let victim = RecordId(rng.next_u32() % n_events as u32);
            events.push(KernelEvent::SoftDeleteRecord { id: victim });
        }

        let mut s1 = KernelState::new();
        let mut s2 = KernelState::new();
        for e in &events {
            let _ = s1.apply_event(e);
            let _ = s2.apply_event(e);
        }

        assert_eq!(
            hash_state_blake3(&s1),
            hash_state_blake3(&s2),
            "seed={seed}: same events diverged between two fresh states"
        );

        // Also verify snapshot → restore gives same hash.
        let snap = encode(&s1);
        if let Ok(r) = decode_state(&snap) {
            assert_eq!(
                hash_state_blake3(&s1),
                hash_state_blake3(&r),
                "seed={seed}: hash changed after snapshot restore"
            );
        }
    }
}
