# Valori Kernel: End-to-End Embedding & Proof Workflow Report

This report provides a detailed, function-level breakdown of the end-to-end process in the `valori-kernel` ecosystem. It follows a vector from the point of input generation on the Python client all the way through validation, persistence, consensus/commit, and cryptographic proof generation on the backend.

---

## 1. Input & Embedding Generation
**Domain**: Python Client (`python/valoricore/`)

**Workflow**:
1. The user provides raw text or an image.
2. The data is passed through an embedding model (e.g., via LangChain, LlamaIndex, or raw HuggingFace models via adapters like `SentenceTransformersAdapter`).
3. An embedding vector is generated (e.g., an array of `f32` floats).

**Internal Cases**:
- *Happy Path*: A standard dimensionality array (e.g., `132`, `384`, `1536`) is returned.
- *Mismatch Case*: If the vector dimension does not match the dimension the `valori-kernel` Node was initialized with, the Node will reject it during the ingestion phase.

## 2. Collection Setup (Logical Grouping)
**Domain**: Python Client / API Layer

**Workflow**:
1. In Valori, "collections" are logically represented via `tags` or `document/chunk` node architectures rather than distinct SQL tables.
2. The user groups embeddings via `tag` fields or by attaching vectors to a specific Document `NodeId`.

**Implementation**:
- The client structures the grouping using `MemoryUpsertVectorRequest` (to link chunks to documents) or standard insertions using a `tag` integer (`InsertRecordRequest`).

## 3. Node Initialization
**Domain**: Rust Backend (`node/src/engine.rs` -> `Engine::new`)

**Workflow**:
1. The Valori server boots up.
2. It initializes the `Engine` orchestrator, which spins up:
   - `KernelState` (The `no_std` deterministic core).
   - `VectorIndex` (BruteForce, HNSW, or IVF).
   - `EventCommitter` and `EventLogWriter` (The persistent WAL).

**Internal Function**:
`Engine::new(cfg: &NodeConfig) -> Self`
- Creates an empty `KernelState`.
- Opens the Write-Ahead Log (WAL) located at `cfg.event_log_path`.
- Bootstraps the `EventJournal` to track ongoing events.

## 4. Python Client Interaction & Connection Layer
**Domain**: `python/valoricore/client.py` ➔ `node/src/server.rs`

**Workflow**:
1. The Python client uses `httpx` (async) or `requests` (sync) to serialize the embedding payload into JSON.
2. The request hits the `Axum` HTTP router on the Valori server.
3. The connection is authenticated via a Bearer token (`auth_guard` middleware).

**API Route Handler**:
`async fn insert_record(State(state): State<SharedEngine>, Json(payload): Json<InsertRecordRequest>) -> Result<Json<InsertRecordResponse>, EngineError>`

## 5. Range Validation & Quantization (f32 -> FXP)
**Domain**: Rust Backend (`node/src/engine.rs`)

**Workflow**:
The core kernel is strictly `no_std` and purely deterministic, meaning floating-point arithmetic is outlawed. Before a vector touches the kernel, it is quantized.

**Internal Function**:
`Engine::insert_record_from_f32(&mut self, values: &[f32]) -> Result<u32, EngineError>`
```rust
let mut fxp_data = Vec::with_capacity(values.len());
for &v in values {
    fxp_data.push(FxpScalar((v * SCALE as f32) as i32));
}
```
**Constraints & Saturation**:
- The `SCALE` is `1 << 16` (Q16.16 fixed-point format).
- Rust's `as i32` cast from `f32` is mathematically saturating. If an embedding value exceeds `[-32768.0, 32767.99]`, it will safely clamp to `i32::MIN` or `i32::MAX`, preserving stability and determinism without crashing.

## 6. Record Creation (Event Generation)
**Domain**: Rust Backend (`node/src/engine.rs`)

**Workflow**:
Once the fixed-point vector `FxpVector` is created, the system must formulate this state transition as a deterministic event.

**Internal Representation**:
```rust
let event = valori_kernel::event::KernelEvent::InsertRecord {
    id: rid,               // RecordId
    vector,                // FxpVector
    metadata: None,        // Binary payload
    tag: 0,                // Logical collection mapping
};
```

## 7. The 4-Step Commit Pipeline (Crucial Durability Layer)
**Domain**: Rust Backend (`node/src/events/event_commit.rs`)

This is the safety wall. The `EventCommitter::commit_event()` orchestrates a strict sequence.

### Step 1: Write to WAL (Event Logging)
`self.event_log.append(&entry)?`
- The `KernelEvent` is serialized via `bincode`.
- It is appended to the `.log` file via `EventLogWriter::append`.
- **Guarantee**: A full `fsync` is called immediately. If the server dies exactly after this, the event will be recovered on next boot.

### Step 2: Buffer / Shadow Space
`self.journal.append_buffered(event.clone());`
- The event is loaded into the volatile memory buffer.
- `ShadowExecutor::from_state(&self.live_state)?` clones the live state by fast heap-based snapshotting.

### Step 3: Shadow Apply (Pre-Commit Verification)
`shadow.shadow_apply(&event)`
- The system attempts to apply the event to the isolated "Shadow" state.
- **Fail Case**: If it violates invariants (e.g., node out of bounds, invalid dimension), the state throws a `KernelError`. The buffer is flushed (`rollback_buffer()`) and the server returns an error. The WAL contains a failed event that is skipped on replay.

### Step 4: Live Persist & Commit
`self.journal.commit_buffer();`
`self.live_state.apply_event(&event)`
- The commit boundary is crossed.
- The `live_state` (the actual memory model) applies the changes deterministically.
- `index.insert()` is called to update the volatile similarity search structures (HNSW/IVF).

## 8. Proof Generation & Verification
**Domain**: Rust Backend (`valori_kernel::proof`, `valori_kernel::verify`)

**Workflow**:
To prove the state of the engine cryptographically, the system computes a BLAKE3 Merkle Tree of the entire kernel architecture.

**Internal Function**: `kernel_state_hash(state: &KernelState) -> [u8; 32]`
1. Hashes the engine dimension.
2. Iterates over raw memory pools (`records`, `nodes`, `edges`).
3. For every memory slot, it hashes the physical location (`(i as u32)`), a presence flag, and the fixed-point (`i32`) data payload sequentially.
4. It outputs a 32-byte root hash.

**Verification (Leaf Proofs)**:
In `src/proof.rs` (`generate_proof_bytes(fixed_values: &[i32])`), individual vectors can be reduced to a Merkle root to prove a specific embedding matches the hash returned in an API receipt.

## 9. Final Persistence & Snapshot Manager
**Domain**: Rust Backend (`node/src/engine.rs`)

**Workflow**:
Periodically, the state needs to be checkpointed to avoid replaying gigabytes of WAL logs on reboot.

**Internal Function**:
`Engine::save_snapshot()` and `snapshot::encode_state`
1. Converts the entire `no_std` deterministic kernel into a flat binary representation.
2. Appends `Metadata` blocks and binary `VectorIndex` structures.
3. Saves it locally or uploads to cloud storage.

## 10. Return Response
**Domain**: Rust Backend (`node/src/server.rs`)

**Workflow**:
The server sends the `RecordId`, Node references, and optionally the `final_state_hash` back to the Python client confirming durable insertion.

```json
{
  "id": 1042
}
```

---

## Edge Cases Handled Internally
1. **Dimension Mismatch**: Handled during WAL loading or Index validation.
2. **Buffer Overflow in Metadata**: Limited by bincode bounds; custom `Serialize` limits binary payloads natively.
3. **Unexpected Process Termination**: Because `file.sync_all()` happens *before* memory updates, `Valori` will seamlessly replay the unapplied WAL entries at reboot via `Engine::restore_from_components()`.
4. **Non-Deterministic Ordering**: Iterations over dictionaries are outlawed. Everything in the core uses positional index arrays to ensure hashes are bit-exact on every platform.
