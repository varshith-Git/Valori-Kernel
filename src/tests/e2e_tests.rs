#![allow(unused_imports)]
#![allow(dead_code)]
extern crate std;
use std::vec::Vec;
use crate::state::kernel::KernelState;
use crate::state::command::Command;
use crate::types::id::{RecordId, NodeId, EdgeId};
use crate::types::vector::FxpVector;
use crate::types::scalar::FxpScalar;
use crate::types::enums::{NodeKind, EdgeKind};
use crate::snapshot::{encode::encode_state, decode::decode_state, hash::hash_state};
use crate::index::brute_force::SearchResult;
use crate::fxp::ops::from_f32;
use crate::error::KernelError;

/// Small test kernel config
const MAX_RECORDS: usize = 8;
const D: usize = 4;
const MAX_NODES: usize = 8;
const MAX_EDGES: usize = 8;

type KS = KernelState<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>;

fn make_vec(values: [f32; D]) -> FxpVector<D> {
    let mut v = FxpVector::<D>::new_zeros();
    for (i, f) in values.iter().enumerate() {
        v.data[i] = from_f32(*f);
    }
    v
}

fn build_commands() -> Vec<Command<D>> {
    vec![
        // Records
        Command::InsertRecord {
            id: RecordId(0),
            vector: make_vec([1.0, 0.0, 0.0, 0.0]),
        },
        Command::InsertRecord {
            id: RecordId(1),
            vector: make_vec([0.0, 1.0, 0.0, 0.0]),
        },

        // Nodes attached to records
        Command::CreateNode {
            node_id: NodeId(0),
            kind: NodeKind::Record,
            record: Some(RecordId(0)),
        },
        Command::CreateNode {
            node_id: NodeId(1),
            kind: NodeKind::Record,
            record: Some(RecordId(1)),
        },

        // Edge from node 0 -> node 1
        Command::CreateEdge {
            edge_id: EdgeId(0),
            kind: EdgeKind::Mentions,
            from: NodeId(0),
            to: NodeId(1),
        },
    ]
}

fn apply_commands(state: &mut KS, cmds: &[Command<D>]) -> Result<(), KernelError> {
    for cmd in cmds {
        state.apply(cmd)?;
    }
    Ok(())
}

#[test]
fn end_to_end_snapshot_roundtrip() {
    // 1. Build initial state
    let mut s1 = KS::new();
    let cmds = build_commands();
    apply_commands(&mut s1, &cmds).unwrap();

    // 2. Invariants should hold
    s1.check_invariants().unwrap();

    // 3. Hash original state
    let h1 = hash_state(&s1);

    // 4. Encode to a buffer
    // conservative upper bound; for tiny configs this is plenty
    let mut buf = [0u8; 4096];
    let encoded_len = encode_state(&s1, &mut buf).unwrap();

    // 5. Decode into a fresh kernel
    let decoded = decode_state::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>(&buf[..encoded_len]).unwrap();

    // 6. Invariants hold after decode
    decoded.check_invariants().unwrap();

    // 7. Hash must match
    let h2 = hash_state(&decoded);
    assert_eq!(h1, h2, "hash mismatch after snapshot roundtrip");
}

#[test]
fn search_is_deterministic() {
    let mut s = KS::new();
    let cmds = build_commands();
    apply_commands(&mut s, &cmds).unwrap();

    let mut results1 = [SearchResult::default(); 2];
    let mut results2 = [SearchResult::default(); 2];

    let query = make_vec([1.0, 0.0, 0.0, 0.0]);

    let k1 = s.search_l2(&query, &mut results1);
    let k2 = s.search_l2(&query, &mut results2);

    assert_eq!(k1, k2);
    assert_eq!(&results1[..k1], &results2[..k2]);
}

#[test]
fn delete_node_cleans_edges_and_preserves_invariants() {
    let mut s = KS::new();
    let mut cmds = build_commands();

    // Apply base scenario
    apply_commands(&mut s, &cmds).unwrap();
    s.check_invariants().unwrap();

    // Now delete node 0 (which also has an outgoing edge)
    cmds.push(Command::DeleteNode { node_id: NodeId(0) });

    // Apply only the delete on a fresh kernel built with same prior commands
    let mut s2 = KS::new();
    apply_commands(&mut s2, &cmds).unwrap();

    // Invariants must still hold after cascading delete
    s2.check_invariants().unwrap();
}
