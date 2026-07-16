# Phase P6 — InsertReceipt: Cryptographic Insert Receipts

## Goal

Every `POST /v1/records` response includes a tamper-evident `InsertReceipt` that lets
any client independently verify what vector was committed, what state it landed in,
and where it sits in the audit chain — without trusting the node.

## Delivered

| File | What landed |
|---|---|
| `crates/valori-kernel/src/proof.rs` | `InsertReceipt` struct + `InsertReceipt::build()` + `InsertReceipt::verify()` + private `compute_self_hash()` |
| `crates/valori-node/src/api.rs` | `InsertReceiptJson` (hex-string HTTP form) + `From<InsertReceipt>` impl; `InsertRecordResponse` gains `receipt: InsertReceiptJson` |
| `crates/valori-node/src/server.rs` | Standalone `insert_record` handler captures `old_root` before insert, computes FXP values, captures `new_root` + `sequence` after, builds and returns receipt |
| `crates/valori-node/src/cluster_server.rs` | Cluster `insert_record` handler captures `old_root` before Raft write, uses `resp.log_index` as `sequence` and `resp.state_hash` as `new_root`, builds and returns receipt; `InsertResponse` gains `receipt` field |
| `crates/valori-kernel/tests/proof.rs` | 5 new `InsertReceipt` tests |
| `python/valoricore/remote.py` | `insert_with_receipt()` on both `SyncRemoteClient` and `AsyncRemoteClient` |

## Receipt fields

| Field | Type | Meaning |
|---|---|---|
| `record_id` | u32 | Allocated record ID |
| `old_root` | hex string | BLAKE3 state hash before this insert |
| `new_root` | hex string | BLAKE3 state hash after this insert |
| `proof` | hex string | Merkle root of the vector's Q16.16 FXP values (`generate_proof_bytes`) |
| `sequence` | u64 | Event-log height (WAL sequence or Raft log index) after this insert |
| `timestamp` | u64 | Unix seconds when the insert was committed |
| `state_hash` | hex string | BLAKE3 self-hash of this receipt — tamper-evident seal |

`state_hash` = `BLAKE3("valori-insert-receipt-v1" ‖ record_id ‖ old_root ‖ new_root ‖ proof ‖ sequence ‖ timestamp)`.
Any party who has the receipt fields can recompute `state_hash` and verify the receipt was not altered.

## How to verify a receipt (client-side)

```python
import hashlib, struct

def verify_receipt(r: dict) -> bool:
    h = hashlib.blake3()
    h.update(b"valori-insert-receipt-v1")
    h.update(struct.pack("<I", r["record_id"]))
    h.update(bytes.fromhex(r["old_root"]))
    h.update(bytes.fromhex(r["new_root"]))
    h.update(bytes.fromhex(r["proof"]))
    h.update(struct.pack("<Q", r["sequence"]))
    h.update(struct.pack("<Q", r["timestamp"]))
    return h.hexdigest() == r["state_hash"]
```

## HTTP response shape

```json
{
  "id": 42,
  "receipt": {
    "record_id": 42,
    "old_root": "aabb...cc",
    "new_root": "ddee...ff",
    "proof":    "1122...33",
    "sequence": 15,
    "timestamp": 1736000000,
    "state_hash": "5566...77"
  }
}
```

Existing clients that only read `response["id"]` are unaffected.

## Findings

- `state_before`/`state_after` in the standalone handler were already computed as hex strings.
  Refactored to capture as `[u8; 32]` first, then hex-encode for backward-compat fields — eliminates
  the redundant hex decode that would have been needed to pass them into `InsertReceipt::build`.
- Cluster path already has `resp.state_hash: [u8; 32]` from `ClientResponse` — zero extra hashing needed.
- `sequence` in the standalone path uses `EventCommitter::journal().committed_height()` captured
  after the insert completes (inside a read lock); 0 when the event log is not configured.

## Validation

```
cargo test -p valori-kernel
# 153 passed, 0 failed (was 148 before P6; +5 InsertReceipt tests)

cargo build -p valori-kernel -p valori-node
# 0 errors
```

## Follow-ups

- **P5** — Benchmark suite (deferred before P6; still needed)
- **P7** — WAL validation tests (partial records, checksum mismatch, truncated WAL)
- **P8** — CI hardening
- **Batch receipts** — `POST /v1/vectors/batch-insert` could return per-record receipts;
  currently deferred (batch has a different state-hash semantics: one root for N inserts)
