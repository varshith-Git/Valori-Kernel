# RFC-0003: Receipt Specification

**Status:** Draft  
**Owner:** valori-effect  
**Stability:** Alpha — receipt hash formula is live (RFC-0003 canonical form) but the assembler is not yet wired into all endpoints  
**Last reviewed:** 2026-07-08  
**Depends on:** [`rfcs/0000-glossary.md`](0000-glossary.md), [`rfcs/0001-operation-lifecycle.md`](0001-operation-lifecycle.md), [`rfcs/0002-kernel-contract.md`](0002-kernel-contract.md)  
**Implements:** Phase A8 (`ReceiptAssembler` + `Receipt` type)

---

## 1. Motivation

Valori's core value proposition is "Git for AI memory" — verifiable, auditable,
tamper-evident knowledge. Today several ad-hoc proof structures exist:

- `EventProof` in `valori-storage` (BLAKE3 log hash + state hash)
- `Receipt` in `valori-mcp` (per-recall BLAKE3 receipt with `prev_hash`)
- `TreeRAG` receipt in `tree_rag.rs` (per-node citation chain)

Each was designed independently, has a different schema, and cannot be composed.
A caller who ran an ingest and then a search has two incompatible proof objects.

This RFC defines a single, versioned `Receipt` type that unifies all proof structures,
is self-describing (carries `KernelABI` and `PlannerFingerprint`), and supports a
Merkle DAG of parent receipts for composable proof chains.

---

## 2. Receipt schema

```rust
/// Versioned envelope — always the outermost type when serializing to disk or wire.
pub struct ReceiptEnvelope {
    pub version: u8,           // currently 1
    pub payload: Receipt,
}

pub struct Receipt {
    // Identity
    pub receipt_id: ReceiptId,           // ULID; unique per receipt
    pub receipt_hash: ReceiptHash,       // BLAKE3(canonical_encode(receipt minus this field))

    // What ran
    pub operation_hash: OperationHash,
    pub graph_hash: GraphHash,

    // Under what contract
    pub kernel_abi: KernelABI,
    pub planner_fingerprint: PlannerFingerprint,
    pub capability_set: CapabilitySet,

    // State transition proof
    pub state_hash_before: StateHash,
    pub state_hash_after: StateHash,     // == state_hash_before for read-only operations

    // Merkle DAG
    pub parent_receipts: Vec<ReceiptHash>,  // empty = root; single-parent today

    // Provenance
    pub shard_id: ShardId,
    pub committed_height: u64,           // Raft committed index at time of receipt
    pub produced_at: u64,                // Unix seconds; excluded from receipt_hash
}

pub struct ReceiptId(ulid::Ulid);
pub struct ReceiptHash([u8; 32]);
pub struct StateHash([u8; 32]);
```

### Read-only operations

For read-only operations (search, GraphRAG, memory search):

- `state_hash_before == state_hash_after`
- `parent_receipts` is empty or points to the last write receipt for this shard
- No `KernelEvent` is written to the audit log
- The state hash is the proof anchor — the caller can independently verify it against
  the node's `/v1/proof` endpoint

### Hash computation

`receipt_hash = BLAKE3(version ‖ receipt_id ‖ operation_hash ‖ graph_hash ‖ kernel_abi_bytes ‖ planner_fingerprint.hash ‖ capability_set_hash ‖ state_hash_before ‖ state_hash_after ‖ parent_receipt_hashes_sorted ‖ shard_id ‖ committed_height)`

`produced_at` is excluded from the hash — timestamps are metadata, not proof.
`parent_receipt_hashes` are sorted lexicographically before hashing to ensure
the hash is independent of insertion order.

---

## 3. ReceiptFragment

Tasks emit `ReceiptFragment` Effects. The `ReceiptAssembler` (owned by `EffectBus`)
collects fragments and assembles the final `Receipt`.

```rust
pub struct ReceiptFragment {
    pub effect_id: EffectId,
    pub execution_id: ExecutionId,
    pub task_id: TaskId,
    pub topological_index: u32,          // position in ExecutionGraph topo order
    pub state_hash_before: StateHash,    // snapshot before this task's kernel writes
    pub state_hash_after: StateHash,     // snapshot after this task's kernel writes
    pub kernel_events: Vec<KernelEventRef>,  // references to audit log entries
}

pub struct KernelEventRef {
    pub shard_id: ShardId,
    pub log_offset: u64,     // byte offset in the event log segment
    pub event_hash: [u8; 32], // BLAKE3 of the serialized KernelEvent
}
```

---

## 4. ReceiptAssembler

The `ReceiptAssembler` is owned by the `EffectBus` (not by `ExecutionContext`).
This decouples assembly from the execution runtime.

```rust
pub struct ReceiptAssembler {
    pending: DashMap<ExecutionId, Vec<ReceiptFragment>>,
    kernel_abi: KernelABI,          // constant for the lifetime of the node
    planner_fingerprint: PlannerFingerprint,
}

impl ReceiptAssembler {
    pub fn submit(&self, fragment: ReceiptFragment) { ... }
    pub fn assemble(&self, execution_id: ExecutionId, graph: &ExecutionGraph) -> Option<Receipt> { ... }
}
```

### Assembly algorithm

```
1. Retrieve all ReceiptFragments for execution_id.
2. Sort fragments by topological_index (ascending) — NOT by completion time.
3. Compute state_hash_before = fragment[0].state_hash_before.
4. Compute state_hash_after = fragment[last].state_hash_after.
5. Compute receipt_hash over all fields per §2.
6. Return Receipt.
```

If any fragment is missing (task failed), assembly returns `None` and the execution
is recorded as `Failed` in `ExecutionHistory`. A partial `Receipt` is never produced.

---

## 5. Merkle DAG

`parent_receipts: Vec<ReceiptHash>` enables a Merkle DAG of receipts:

```
Receipt A (ingest doc v1)
    │
    ▼
Receipt B (search against v1)
    │
    ▼
Receipt C (ingest doc v2 — supersedes)
    │   └── parent: [Receipt A.hash]
    ▼
Receipt D (search against v2)
        └── parent: [Receipt C.hash]
```

A verifier can walk `parent_receipts` backward to reconstruct the complete
provenance chain for any query result.

**Today**: single-parent only. Multi-parent (e.g. a consolidation receipt that
has both the old and new ingest receipts as parents) is supported by the schema
but not yet produced by any handler.

---

## 6. Offline verification

A `Receipt` is self-describing: it carries everything needed to verify it without
calling a live node.

```
verify_receipt(receipt: Receipt, event_log_segments: &[Path]) -> VerifyResult:
    1. Recompute receipt_hash — compare against receipt.receipt_hash. Fail if mismatch.
    2. Check kernel_abi matches KERNEL_ABI_VERSION expected by the verifier.
    3. Replay KernelEvents referenced by receipt.kernel_events from the log segments.
    4. Verify running BLAKE3 chain matches log entries.
    5. Verify final KernelState hash == receipt.state_hash_after.
    6. For each parent_receipt: recursively verify (or load from archive).
```

The `valori-verify` binary implements this algorithm. It depends only on
`valori-kernel` and `valori-storage`.

---

## 7. Migration from existing proof structures

| Existing type | Location | Migration |
|---|---|---|
| `EventProof` | `valori-storage/src/events/event_proof.rs` | Wrap in `Receipt` with empty `parent_receipts`, `state_hash_before/after` from `EventProof.final_state_hash`. |
| MCP `Receipt` | `valori-mcp/src/main.rs` | Replace with unified `Receipt`; `prev_hash` becomes `parent_receipts[0]`. |
| Tree-RAG receipt | `valori-node/src/tree_rag.rs` | Replace breadcrumb chain with `Receipt` per tree query; parent chain via `parent_receipts`. |

Migration is additive — existing endpoints continue to return their current format
until Phase A8 lands. Phase A8 introduces the unified type and updates all three paths.

---

## 8. CapabilitySet

`CapabilitySet` records which capabilities were active during the execution,
allowing a verifier to check that the node was authorized to perform the operation.

```rust
pub struct CapabilitySet {
    pub embed: bool,
    pub llm: bool,
    pub object_store: bool,
    pub cluster: bool,
    pub shard_count: u8,
}
```

The `CapabilitySet` is embedded in the `Receipt` but does not affect the
`receipt_hash` computation — it is informational metadata about the node's
configuration, not a cryptographic commitment.

---

## 9. Wire format

Receipts are serialized as `ReceiptEnvelope { version: 1, payload: Receipt }` using
`bincode` v2 with fixed-endian encoding. The `version` byte is the first byte on the wire.

Receipts returned over HTTP (e.g. `/v1/proof/receipt`) are additionally JSON-encoded
with hex-encoded byte arrays for human readability.

---

## 10. Files to create/modify

| File | Change |
|---|---|
| `crates/valori-node/src/receipt.rs` | `Receipt`, `ReceiptEnvelope`, `ReceiptId`, `ReceiptHash`, `StateHash`, `ReceiptFragment`, `KernelEventRef`, `CapabilitySet` |
| `crates/valori-node/src/receipt_assembler.rs` | `ReceiptAssembler`, assembly algorithm |
| `crates/valori-storage/src/events/event_proof.rs` | Add `From<EventProof> for Receipt` migration helper |
| `crates/valori-mcp/src/main.rs` | Replace ad-hoc receipt with unified `Receipt` |
| `crates/valori-node/src/tree_rag.rs` | Replace `TreeReceipt` with unified `Receipt` |
| `crates/valori-verify/src/main.rs` | Implement `verify_receipt` algorithm using `Receipt` |
