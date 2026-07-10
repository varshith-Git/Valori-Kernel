//! Fuzz target: insert random vectors then search with a random query.
//!
//! The kernel must never panic regardless of vector contents,
//! including extreme values, k > record count, or empty state.
#![no_main]

use libfuzzer_sys::fuzz_target;
use libfuzzer_sys::arbitrary::{self, Arbitrary};
use valori_kernel::state::kernel::KernelState;
use valori_kernel::index::SearchResult;
use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::vector::FxpVector;
use valori_kernel::fxp::ops::from_f32;

const DIM: usize = 4;

#[derive(Arbitrary, Debug)]
struct Input {
    inserts: Vec<[f32; DIM]>,
    query: [f32; DIM],
    k: u8,
}

fn safe_fxp_vec(raw: &[f32]) -> FxpVector {
    FxpVector {
        data: raw.iter()
            .map(|v| from_f32(if v.is_finite() { *v } else { 0.0 }))
            .collect(),
    }
}

fuzz_target!(|input: Input| {
    let mut state = KernelState::new();

    for (i, raw) in input.inserts.iter().take(64).enumerate() {
        let evt = KernelEvent::InsertRecord {
            id: RecordId(i as u32),
            vector: safe_fxp_vec(raw),
            metadata: None,
            tag: 0,
        };
        let _ = state.apply_event(&evt);
    }

    let k = (input.k as usize).max(1);
    let query = safe_fxp_vec(&input.query);
    let mut results = vec![SearchResult::default(); k];
    let _ = state.search_l2(&query, &mut results, None);
});
