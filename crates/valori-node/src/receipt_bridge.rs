// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Receipt bridge — Phase A10 (initial), upgraded to real OperationHash in A11.
//!
//! Thin shim between existing HTTP handlers and `ReceiptAssembler`. Handlers
//! call `emit_write` / `emit_read` after their existing logic so that
//! `GET /v1/proof/receipt` returns real per-operation receipts.
//!
//! The operation hash is the canonical RFC-0003 hash:
//! `BLAKE3(kind_discriminant ‖ bincode(inputs) ‖ bincode(policy))`
//! which is reproducible from the planning parameters alone (no data, no time).
use valori_core::id::ExecutionId;
use valori_effect::{ReceiptAssembler, ReceiptFragment, ReceiptStore};
use valori_planner::operation::{
    compute_operation_hash, ExecutionPolicy, OperationInputs, OperationKind,
};

pub use valori_planner::operation::{ConsistencyLevel, OperationInputs as Inputs};

fn now_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn make_assembler(
    kind: OperationKind,
    inputs: &OperationInputs,
    ns_id: u16,
    shard_id: u8,
    committed_height: u64,
    cluster_mode: bool,
) -> ReceiptAssembler {
    let op_hash = compute_operation_hash(kind, inputs, &ExecutionPolicy::default()).to_hex();
    let nanos = now_nanos();
    ReceiptAssembler::new(
        ExecutionId { hi: nanos, lo: ns_id as u64 },
        op_hash,
        "bridge-v0".into(), // graph_hash — full planner graph hash arrives in A12
        "bridge-v0".into(), // fp_hash   — full planner fp hash arrives in A12
        1,
        false,
        cluster_mode,
        1,
        shard_id,
        committed_height,
        vec![],
    )
}

/// Emit a receipt for a **mutating** operation (insert, delete, batch-insert).
///
/// `state_before` and `state_after` are lowercase hex BLAKE3 digests of the
/// kernel state before and after the write. `committed_height` is the WAL
/// event count (standalone) or Raft log index (cluster).
pub fn emit_write(
    store: &ReceiptStore,
    kind: OperationKind,
    inputs: &OperationInputs,
    ns_id: u16,
    shard_id: u8,
    committed_height: u64,
    cluster_mode: bool,
    state_before: String,
    state_after: String,
) {
    let asm = make_assembler(kind, inputs, ns_id, shard_id, committed_height, cluster_mode);
    asm.push(ReceiptFragment {
        task_index: 0,
        state_hash_before: state_before,
        state_hash_after: state_after,
        mutated: true,
        fragment_hash: String::new(),
    });
    store.insert(asm.assemble());
}

/// Emit a receipt for a **read-only** operation (search, proof queries).
pub fn emit_read(
    store: &ReceiptStore,
    kind: OperationKind,
    inputs: &OperationInputs,
    ns_id: u16,
    shard_id: u8,
    committed_height: u64,
    cluster_mode: bool,
    state_hash: String,
) {
    let asm = make_assembler(kind, inputs, ns_id, shard_id, committed_height, cluster_mode);
    asm.push(ReceiptFragment::read_only(0, state_hash));
    store.insert(asm.assemble());
}
