# Phase A11 — Receipt Wiring, Python SDK, & UI Proof Dashboard

## Goal

Complete the receipt bridge wiring across remaining standalone and cluster write handlers (`ingest`, `ingest_update`, `memory_upsert_vector`), expose receipt retrieval methods in all Python SDK clients, and integrate an interactive cryptographic receipt verification card into the UI Proof Dashboard.

## Delivered

### `crates/valori-node/src/ingest.rs`
- Injected `Extension<Arc<ReceiptStore>>` into `ingest` and `ingest_update` standalone handlers.
- Captured pre- and post-operation state hashes (`hash_state_blake3(&engine.state)`) around document ingestion and chunk vector insertions.
- Emitted mutating write receipts via `receipt_bridge::emit_write` for complete audibility of document pipelines.

### `crates/valori-node/src/server.rs` & `cluster_server.rs`
- Wired `emit_write` into `memory_upsert_vector` (standalone) and `cluster_ingest`, `cluster_ingest_update`, and `cluster_memory_upsert` (cluster mode).
- Ensured cluster handlers extract real Raft log commit indices and shard-level state machine hashes (`sm.state_hash().await`) for verifiable multi-node provenance.

### `python/valoricore/remote.py`
- Added `get_receipt() -> Dict[str, Any]` and `get_receipt_by_id(receipt_id: str) -> Dict[str, Any]` across:
  - `SyncRemoteClient`
  - `AsyncRemoteClient`
  - `SyncReplicatedClient`
  - `AsyncReplicatedClient`
- Added full unit test suite in `python/tests/test_remote_receipts.py` verifying request formatting and payload unwrapping.

### UI Proof Dashboard (`ui/`)
- **API Endpoint**: Created `src/app/api/proof/receipt/route.ts` bridging frontend client calls to backend `/v1/proof/receipt`.
- **SWR Hook**: Created `src/lib/hooks/useReceipt.ts` with auto-refresh and type-safe payload parsing.
- **Interactive Card**: Created `src/components/proof/ReceiptCard.tsx` displaying:
  - Operation receipt ID and operation hash (BLAKE3).
  - Planner fingerprint hash.
  - State transition hashes (`state_hash_before` → `state_hash_after`).
  - Interactive verification button checking receipt integrity and visual status badges (Read-Only vs. State Transition).
- Integrated `ReceiptCard` directly into the hero section of `src/app/proof/page.tsx`.

## Validation

```bash
cargo check -p valori-node               # Clean
cargo test -p valori-node                # All unit & integration replication tests passed
./.venv/bin/pytest python/tests/test_remote_receipts.py # 2/2 SDK tests passed
cd ui && npm run build                   # Static & dynamic Next.js build completed with 0 errors
```
