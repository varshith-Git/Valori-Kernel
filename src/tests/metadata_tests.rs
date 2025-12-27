// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::state::kernel::KernelState;
use crate::event::KernelEvent;
use crate::types::id::RecordId;
use crate::types::vector::FxpVector;
use crate::snapshot::encode::encode_state;
use crate::snapshot::decode::decode_state;
use crate::snapshot::hash::hash_state;
use crate::storage::record::Record;
use crate::error::KernelError;
use crate::config::MAX_METADATA_SIZE;
use alloc::vec;
use alloc::vec::Vec;

const MAX_RECORDS: usize = 100;
const D: usize = 4;
const MAX_NODES: usize = 10;
const MAX_EDGES: usize = 10;

#[test]
fn test_snapshot_roundtrip_metadata() {
    let mut state = KernelState::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>::new();
    
    let id = RecordId(0);
    let vector = FxpVector::new_zeros();
    let metadata = vec![0xDE, 0xAD, 0xBE, 0xEF];
    
    let evt = KernelEvent::InsertRecord {
        id,
        vector: vector.clone(),
        metadata: Some(metadata.clone()),
    };
    
    state.apply_event(&evt).expect("Apply event failed");
    
    // Verify stored
    let record = state.get_record(id).expect("Record should exist");
    assert_eq!(record.metadata.as_ref().unwrap(), &metadata);
    
    // Snapshot
    let mut buf = vec![0u8; 1024 * 64];
    let len = encode_state(&state, &mut buf).expect("Encode failed");
    let encoded = &buf[..len];
    
    // Restore
    let restored = decode_state::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>(encoded)
        .expect("Decode failed");
        
    let restored_record = restored.get_record(id).expect("Restored record should exist");
    assert_eq!(restored_record.metadata.as_ref().unwrap(), &metadata);
    
    // Hash check
    assert_eq!(hash_state(&state), hash_state(&restored));
}

#[test]
fn test_metadata_changes_hash() {
    let mut state1 = KernelState::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>::new();
    let mut state2 = KernelState::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>::new();
    
    let id = RecordId(0);
    let vector = FxpVector::new_zeros();
    
    let evt1 = KernelEvent::InsertRecord {
        id,
        vector: vector.clone(),
        metadata: Some(vec![1, 2, 3]),
    };
    
    let evt2 = KernelEvent::InsertRecord {
        id,
        vector: vector.clone(),
        metadata: Some(vec![1, 2, 4]), // Different byte
    };
    
    state1.apply_event(&evt1).unwrap();
    state2.apply_event(&evt2).unwrap();
    
    assert_ne!(hash_state(&state1), hash_state(&state2), "Different metadata must produce different hash");
    
    let mut state3 = KernelState::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>::new();
    let evt3 = KernelEvent::InsertRecord {
        id,
        vector,
        metadata: None,
    };
    state3.apply_event(&evt3).unwrap();
    
    assert_ne!(hash_state(&state1), hash_state(&state3), "Metadata vs No metadata must differ");
}

#[test]
fn test_cannot_insert_metadata_over_limit() {
    let mut state = KernelState::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>::new();
    let id = RecordId(0);
    let vector = FxpVector::new_zeros();
    
    let big_metadata = vec![0u8; MAX_METADATA_SIZE + 1];
    
    let evt = KernelEvent::InsertRecord {
        id,
        vector,
        metadata: Some(big_metadata),
    };
    
    let res = state.apply_event(&evt);
    assert!(matches!(res, Err(KernelError::MetadataTooLarge)));
}

// Mock test for legacy V1 snapshot compatibility
// We manually construct a V1 buffer (mocking what encode used to do)
#[test]
fn test_legacy_snapshot_loads_without_metadata() {
    // Manually build V1 blob
    // This requires duplicating V1 encode logic or relying on the fact that V2 append is cleaner.
    // However, V2 changed the Header Version too.
    
    // Header V1
    let mut buf = vec![0u8; 1024];
    let mut offset = 0;
    
    buf[0..4].copy_from_slice(b"VALK"); offset += 4;
    
    // Version = 1
    offset += 4; buf[4..8].copy_from_slice(&1u32.to_le_bytes()); // Schema Ver
    
    offset += 8; buf[8..16].copy_from_slice(&100u64.to_le_bytes()); // State Ver
    
    // Capacities
    offset += 4; buf[16..20].copy_from_slice(&(MAX_RECORDS as u32).to_le_bytes());
    offset += 4; buf[20..24].copy_from_slice(&(D as u32).to_le_bytes());
    offset += 4; buf[24..28].copy_from_slice(&(MAX_NODES as u32).to_le_bytes());
    offset += 4; buf[28..32].copy_from_slice(&(MAX_EDGES as u32).to_le_bytes());
    
    // Records Count = 1
    let rec_count: u32 = 1;
    let rec_count_pos = offset;
    offset += 4; buf[rec_count_pos..rec_count_pos+4].copy_from_slice(&rec_count.to_le_bytes());
    
    // Record 0
    let rid: u32 = 55;
    let rid_pos = offset; offset += 4;
    buf[rid_pos..rid_pos+4].copy_from_slice(&rid.to_le_bytes());
    
    // Flags
    let flags: u8 = 0;
    buf[offset] = flags; offset += 1;
    
    // Vector (D=4 -> 4 * 4 = 16 bytes)
    for _ in 0..D {
        let val: i32 = 0;
        let p = offset; offset += 4;
        buf[p..p+4].copy_from_slice(&val.to_le_bytes());
    }
    
    // NOTE: In V1, we STOP here for the record. NO Metadata len.
    
    // Nodes Count = 0
    let node_count: u32 = 0;
    let nc_pos = offset; offset += 4;
    buf[nc_pos..nc_pos+4].copy_from_slice(&node_count.to_le_bytes());

    // Edges Count = 0
    let edge_count: u32 = 0;
    let ec_pos = offset; offset += 4;
    buf[ec_pos..ec_pos+4].copy_from_slice(&edge_count.to_le_bytes());

    let encoded = &buf[..offset];
    
    // Decode
    let restored = decode_state::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>(encoded)
        .expect("Should decode V1 successfully");
        
    let rec = restored.get_record(RecordId(55)).expect("Record should exist");
    assert!(rec.metadata.is_none(), "V1 snapshot should default to None metadata");
}
