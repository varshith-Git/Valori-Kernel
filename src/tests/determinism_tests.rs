use crate::state::kernel::KernelState;
use crate::state::command::Command;
use crate::types::id::{RecordId, NodeId, EdgeId, Version};
use crate::types::vector::FxpVector;
use crate::snapshot::hash::hash_state;
use crate::types::enums::{NodeKind, EdgeKind};
use crate::fxp::ops::from_f32;
use std::vec::Vec;

/// A simple deterministic RNG for tests.
struct Pcg32 {
    state: u64,
    inc: u64,
}

impl Pcg32 {
    fn new(seed: u64) -> Self {
        Self { state: seed, inc: 1 }
    }

    fn next_u32(&mut self) -> u32 {
        let oldstate = self.state;
        self.state = oldstate.wrapping_mul(6364136223846793005).wrapping_add(self.inc);
        let xorshifted = (((oldstate >> 18) ^ oldstate) >> 27) as u32;
        let rot = (oldstate >> 59) as u32;
        xorshifted.rotate_right(rot)
    }
}

fn generate_random_command<const D: usize>(rng: &mut Pcg32, i: u32) -> Command<D> {
    let type_val = rng.next_u32() % 6;
    match type_val {
        0 => {
            // InsertRecord
            let mut vec = FxpVector::new_zeros();
            for k in 0..D {
                vec.data[k] = from_f32((rng.next_u32() % 100) as f32);
            }
            Command::InsertRecord { id: RecordId(i), vector: vec } 
        },
        1 => Command::DeleteRecord { id: RecordId(rng.next_u32() % 10) },
        2 => Command::CreateNode { 
            node_id: NodeId(i), 
            kind: NodeKind::Record, 
            record: Some(RecordId(rng.next_u32() % 10)) 
        },
        3 => Command::CreateEdge {
            edge_id: EdgeId(i),
            kind: EdgeKind::Relation,
            from: NodeId(rng.next_u32() % 10),
            to: NodeId(rng.next_u32() % 10),
        },
        4 => Command::DeleteNode { node_id: NodeId(rng.next_u32() % 10) },
        _ => Command::DeleteEdge { edge_id: EdgeId(rng.next_u32() % 10) },
    }
}

/// Runs a deterministic sequence of commands and returns the final state hash.
fn run_simulation(seed: u64, steps: usize) -> u64 {
    const R: usize = 100;
    const D: usize = 2;
    const N: usize = 100;
    const E: usize = 100;

    let mut kernel = KernelState::<R, D, N, E>::new();
    let mut rng = Pcg32::new(seed);

    for i in 0..steps {
        let cmd = generate_random_command(&mut rng, i as u32);
        let _ = kernel.apply(&cmd); // Ignore errors, pass by reference
    }
    
    // Check invariants after simulation
    kernel.check_invariants().expect("Invariants violated during simulation");

    hash_state(&kernel)
}

#[test]
fn test_determinism_harness() {
    let seed = 42;
    let steps = 50;
    
    let hash1 = run_simulation(seed, steps);
    let hash2 = run_simulation(seed, steps);
    
    assert_eq!(hash1, hash2, "State hashes must be virtually identical for same seed");
    assert_ne!(hash1, 0, "Hash shouldn't be zero (probability)");
    
    // Different seed should differ
    let hash3 = run_simulation(seed + 1, steps);
    assert_ne!(hash1, hash3);
}
