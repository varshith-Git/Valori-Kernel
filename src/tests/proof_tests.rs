// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::state::kernel::KernelState;
use crate::state::command::Command;
use crate::types::id::RecordId;
use crate::types::vector::FxpVector;
use crate::snapshot::encode::encode_state;
use crate::verify::kernel_state_hash;
use crate::replay::replay_and_hash;
use crate::types::scalar::FxpScalar;
use std::vec::Vec;

fn write_wal_header(dim: u32, buf: &mut Vec<u8>) {
    // [Version: u32][Encoding: u32][Dim: u32][ChecksumLen: u32]
    buf.extend_from_slice(&1u32.to_le_bytes()); // Version
    buf.extend_from_slice(&1u32.to_le_bytes()); // Encoding
    buf.extend_from_slice(&dim.to_le_bytes()); // Dim
    buf.extend_from_slice(&0u32.to_le_bytes()); // ChecksumLen (0 for test)
}

#[test]
fn test_deterministic_replay() {
    // Constants
    const MAX_RECORDS: usize = 16;
    const D: usize = 4;
    const MAX_NODES: usize = 16;
    const MAX_EDGES: usize = 16;

    // 1. Create Initial State
    let mut state = KernelState::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>::new();
    
    // Add a record
    let id = RecordId(0);
    let mut vector = FxpVector::<D>::default();
    vector.data[0] = FxpScalar::ONE;
    
    let cmd1: Command<D> = Command::InsertRecord { id, vector };
    state.apply(&cmd1).unwrap();
    
    let hash_t1 = kernel_state_hash(&state);
    
    // 2. Snapshot T1
    let mut snap_buf = vec![0u8; 4096];
    let len = encode_state(&state, &mut snap_buf).unwrap();
    let snapshot_bytes = &snap_buf[0..len];
    
    // 3. Verify Replay (Snapshot Only)
    // Empty WAL implies no header needed? Or logic says "if !slice.is_empty() -> check header".
    // Passing empty slice should work as No-Op.
    let replay_hash = replay_and_hash::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>(
        snapshot_bytes,
        &[] 
    ).unwrap();
    
    assert_eq!(replay_hash, hash_t1, "Replay of snapshot should match live state");
    
    // 4. Create WAL for T2 (Add another record)
    let id2 = RecordId(1);
    let mut vector2 = FxpVector::<D>::default();
    vector2.data[1] = FxpScalar::ONE;
    
    let cmd2: Command<D> = Command::InsertRecord { id: id2, vector: vector2 };
    
    // Apply to live
    state.apply(&cmd2).unwrap();
    let hash_t2 = kernel_state_hash(&state);
    
    assert_ne!(hash_t1, hash_t2, "State hash should change after mutation");

    // Serialize WAL
    let mut wal_bytes = Vec::new();
    write_wal_header(D as u32, &mut wal_bytes); // Add Header

    let config = bincode::config::standard();
    let mut cmd_buf = [0u8; 1024];
    let len = bincode::serde::encode_into_slice(&cmd2, &mut cmd_buf, config).unwrap();
    wal_bytes.extend_from_slice(&cmd_buf[0..len]);
    
    // 5. Verify Replay (Snapshot + WAL)
    let replay_hash_t2 = replay_and_hash::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>(
        snapshot_bytes,
        &wal_bytes
    ).unwrap();
    
    assert_eq!(replay_hash_t2, hash_t2, "Replay of snapshot + WAL should match live state T2");
}

#[test]
fn test_multiple_commands_wal() {
    const MAX_RECORDS: usize = 16;
    const D: usize = 4;
    const MAX_NODES: usize = 16;
    const MAX_EDGES: usize = 16;

    // Init Snapshot (Empty)
    let state = KernelState::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>::new();
    let mut snap_buf = vec![0u8; 1024];
    let len = encode_state(&state, &mut snap_buf).unwrap();
    let snapshot_bytes = &snap_buf[0..len];
    
    // WAL
    let mut wal_bytes = Vec::new();
    write_wal_header(D as u32, &mut wal_bytes); // Add Header
    
    let config = bincode::config::standard();
    let mut buf = [0u8; 256];
    
    for i in 0..3 {
        let cmd: Command<D> = Command::InsertRecord {
            id: RecordId(i),
            vector: FxpVector::default(),
        };
        // Append encoded command
        let len = bincode::serde::encode_into_slice(&cmd, &mut buf, config).unwrap();
        wal_bytes.extend_from_slice(&buf[0..len]);
    }
    
    // Replay
    let result = replay_and_hash::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>(
        snapshot_bytes,
        &wal_bytes
    );
    
    assert!(result.is_ok(), "Should replay multiple commands successfully");
}

#[test]
fn test_bad_wal_header() {
    const MAX_RECORDS: usize = 16;
    const D: usize = 4;
    const MAX_NODES: usize = 16;
    const MAX_EDGES: usize = 16;
    
    let mut wal_bytes = Vec::new();
    write_wal_header(1234, &mut wal_bytes); // Bad Dimension (1234 != 4)
    
    let result = replay_and_hash::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>(
        &[], // Empty snapshot creates new state
        &wal_bytes
    );
    
    assert!(result.is_err(), "Should detect invalid dimension in header");
}

#[test]
fn test_wal_no_header() {
    const MAX_RECORDS: usize = 16;
    const D: usize = 4;
    const MAX_NODES: usize = 16;
    const MAX_EDGES: usize = 16;
    
    // Create random bytes that look like commands but have NO header
    let wal_bytes = vec![1, 2, 3, 4, 5]; 
    
    let result = replay_and_hash::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>(
        &[], 
        &wal_bytes
    );
    
    // Should fail because it tries to read header and fails length check (5 < 16)
    // Or if it reads header, version/dim will be garbage.
    assert!(result.is_err(), "Should fail on WAL without header");
}

#[test]
fn test_structural_hashing() {
    // We want to verify that position matters.
    // State A: Record at 0, 2 (Hole at 1)
    // State B: Record at 0, 1 (Hole at 2)
    // Even if records are identical content, state hash MUST differ.
    
    const MAX_RECORDS: usize = 4;
    const D: usize = 4;
    const MAX_NODES: usize = 4; // Minimal
    const MAX_EDGES: usize = 4;
    
    let base_vec = FxpVector::<D>::default();
    
    // State A
    let mut state_a = KernelState::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>::new();
    // Insert 0
    state_a.records.records[0] = Some(crate::storage::record::Record { 
        id: RecordId(0), 
        vector: base_vec, 
        flags: 0 
    });
    // Insert 2 (Manual injection to simulate hole at 1 since Insert strictly follows first-free)
    state_a.records.records[2] = Some(crate::storage::record::Record { 
        id: RecordId(2), 
        vector: base_vec, // Identical content
        flags: 0 
    });
    
    // State B
    let mut state_b = KernelState::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>::new();
    // Insert 0
    state_b.records.records[0] = Some(crate::storage::record::Record { 
        id: RecordId(0), 
        vector: base_vec, 
        flags: 0 
    });
    // Insert 1
    state_b.records.records[1] = Some(crate::storage::record::Record { 
        id: RecordId(1), 
        vector: base_vec, // Identical content
        flags: 0 
    });
    
    let hash_a = kernel_state_hash(&state_a);
    let hash_b = kernel_state_hash(&state_b);
    
    assert_ne!(hash_a, hash_b, "Hash must distinguish [R, None, R] from [R, R, None]");
}
