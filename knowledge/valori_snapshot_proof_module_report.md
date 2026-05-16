# Valori Kernel: Module Analysis - Snapshots & Cryptographic Proofs

This is the final report in our core architecture deep dive. Having established how the engine does math, stores data, links relationships, and orchestrates commands, we now look at its primary unique selling proposition: **Cryptographic Auditability**.

Valori is designed so that at any point, the entire state of the vector engine can be dumped to disk, and a cryptographic "receipt" can be generated to prove that a specific set of vectors exists in exactly that state.

---

## 1. Snapshots: Binary Serialization

**Location**: `src/snapshot/encode.rs` and `src/snapshot/decode.rs`

To be deterministic, Valori does not rely on Serde's default JSON or MessagePack implementations for its global state (though it uses `bincode` for the event log). Instead, it implements a manual, byte-exact binary format.

### `encode_state(state: &KernelState, buf: &mut [u8]) -> Result<usize>`
This function serializes the `KernelState` into a raw `u8` slice using `to_le_bytes()` for explicit Little Endian conversion.
1. **Header**: Writes the magic bytes `VALK`, the schema version (`3`), and the `state.version`.
2. **Capacities**: Writes the capacities/lengths of the `records`, the vector `dimension`, `nodes`, and `edges`.
3. **Records Dump**:
   - It iterates through the `raw_records()` (which includes `None` holes).
   - If a slot is filled (`Some`), it writes a `1` (Presence Marker), the `RecordId`, the Q16.16 `FxpScalars`, and finally the binary `metadata`.
   - If a slot is empty, it writes a `0` (Absence Marker) and moves on. This is *crucial* because recovering the snapshot must perfectly recreate the array indexing layout.
4. **Graph Dump**: Recursively walks `nodes` and `edges`, translating `Option<T>` fields into `[1, value]` or `[0]` sentinel bytes.

### `decode_state(buf: &[u8]) -> Result<KernelState>`
Performs the exact inverse. 
- It uses `resize()` on the internal `alloc::vec::Vec` pools when it encounters an object.
- Because it respects the Presence/Absence markers from the dump, the array indices mapped into the restored `RecordPool`, `NodePool`, and `EdgePool` perfectly match the ones present at the time of the snapshot.

---

## 2. Cryptographic Proofs: BLAKE3 & FNV-1a

**Location**: `src/snapshot/blake3.rs` and `src/snapshot/hash.rs`

Valori supports two hash implementations: a fast FNV-1a hash (for internal checksums) and a Cryptographically Secure **BLAKE3** hash (for external proofs and Merkle trees).

### `hash_state_blake3(state: &KernelState) -> [u8; 32]`
This is the canonical state hash used for all proof generation.
1. **Engine Version**: It seeds the hasher with the `Version`.
2. **Records Phase**: It iterates through the `RecordPool`. For every record, it updates the `blake3::Hasher` with the `id`, `flags`, and the `i32` fixed-point `vector.data` points in Little Endian.
3. **Topology Phase (Graph)**: It hashes the exact structural layout of the graph. It writes the `node_id`, `kind`, and then the `first_out_edge` ID. If an edge doesn't exist, it writes a sentinel (`u32::MAX`).
4. **Determinism Guarantee**: Because floats do not exist, and because iteration order over the `alloc::vec::Vec` is perfectly linear, this function will yield the exact same 32-byte hash whether executed on a 32-bit ARM chip or a 64-bit x86 processor.

---

## 3. The Merkle Tree (Individual Receipts)

**Location**: `src/proof.rs`

While `hash_state_blake3` proves the *global* state, clients often want to prove that their specific vector is included in the engine.

### `DeterministicProof`
```rust
pub struct DeterministicProof {
    pub kernel_version: u64,
    pub snapshot_hash: [u8; 32],
    pub wal_hash: [u8; 32],
    pub final_state_hash: [u8; 32],
}
```
This object serves as a cryptographically secure receipt returned to the user when they interact with the API (`node/src/server.rs -> get_proof`).

### `generate_proof_bytes(fixed_values: &[i32]) -> Vec<u8>`
If a client wants to verify an embedding independently:
1. The function takes the raw fixed-point components (`&[i32]`).
2. It hashes each component into a Leaf Hash, prefixing it with a cryptographic domain separator (`b"VALORI_LEAF"`).
3. It recursively pairs up the leaves, hashing them together with the `b"VALORI_NODE"` separator, until a single 32-byte Merkle Root is produced.

*This enables zero-trust setups, where a user can re-compute the Merkle Root of their embedding locally, and verify it against the `DeterministicProof` returned by the remote server.*

---

### Summary of Module Edge Cases
1. **Endianness Safety**: Every integer in the encoding and hashing logic forces `to_le_bytes()`. If this was not strictly enforced, Big-Endian architectures would produce entirely different state hashes for the same data.
2. **Sentinel Safety**: When hashing `Option` types (like a Node that doesn't reference a Record), the system forces a predictable sentinel byte sequence (`u32::MAX`) into the hasher rather than skipping it, preventing hash collisions between a Node with `id: 0` and a Node with no `id`.
3. **Strict Bounds Checking**: `decode.rs` rigorously checks `offset + size > buf.len()` at every single read step to ensure malformed or maliciously crafted snapshot files cannot trigger panics or buffer over-reads during recovery.
