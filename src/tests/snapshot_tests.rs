use crate::state::kernel::KernelState;
use crate::state::command::Command;
use crate::types::id::{RecordId, NodeId};
use crate::types::vector::FxpVector;
use crate::types::enums::{NodeKind};
use crate::snapshot::encode::encode_state;
use crate::snapshot::decode::decode_state;
use crate::snapshot::hash::hash_state;
use std::vec;

#[test]
fn test_snapshot_restore() {
    // Setup state
    const R: usize = 5;
    const D: usize = 2;
    const N: usize = 5;
    const E: usize = 5;
    let mut kernel = KernelState::<R, D, N, E>::new();

    // Apply some commands
    kernel.apply(&Command::InsertRecord { id: RecordId(0), vector: FxpVector::new_zeros() }).unwrap();
    kernel.apply(&Command::CreateNode { node_id: NodeId(0), kind: NodeKind::Record, record: Some(RecordId(0)) }).unwrap();
    
    // Checksum original
    let hash_orig = hash_state(&kernel);

    // Encode
    let mut buf = [0u8; 1024];
    let len = encode_state(&kernel, &mut buf).unwrap();
    
    // Decode
    let restored_kernel = decode_state::<R, D, N, E>(&buf[..len]).unwrap();

    // Verify
    let hash_restored = hash_state(&restored_kernel);
    assert_eq!(hash_orig, hash_restored);
    
    assert_eq!(kernel.version, restored_kernel.version);
    assert!(restored_kernel.records.get(RecordId(0)).is_some());
    assert!(restored_kernel.nodes.get(NodeId(0)).is_some());
}
