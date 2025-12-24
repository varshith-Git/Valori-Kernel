# Deterministic Proofs & Verifiability

Valori is designed to provide **externally verifiable memory**.
This document explains how Valori guarantees that two separate machines replay the same history to the exact same state.

## The Promise

> Given a valid **Snapshot** ($S_0$) and a **WAL** ($W$), replay($S_0$, $W$) always produces State Hash $H_F$.

This property holds true regardless of:
- CPU Architecture (x86 vs ARM vs WASM)
- Operating System (Linux vs macOS vs Windows)
- Wall-clock time or network latency
- Compiler version (assuming strict contract: Rust $\ge$ 1.85)

## Hashing Strategy

We use **BLAKE3** (256-bit) for all cryptographic hashes due to its speed and security.

### 1. Kernel State Hash `H(K)`

The state hash uniquely identifies the semantic contents of the kernel. It is computed by sorting and hashing components in a canonical order:

1.  **Records** (Sorted by `RecordId`):
    - `Hash(ID | Flags | VectorData)`
    - Vector data is hashed using raw `i32` fixed-point representations.

2.  **Nodes** (Sorted by `NodeId`):
    - `Hash(ID | Kind | RecordLink | EdgeHead)`

3.  **Edges** (Sorted by `EdgeId`):
    - `Hash(ID | Kind | From | To | NextOut)`

### 4. Scope & Exclusions

**Strict Kernel Scope**:
This proof system covers the **Deterministic Kernel State ONLY**.
It explicitly includes:
- Kernel Version
- Records, Nodes, Edges.
- Tne entire memory structure (including empty slots/holes) to ensure `[A, None] != [None, A]`.

It **EXCLUDES**:
- **Node-level metadata**: HTTP headers, auth tokens, user sessions.
- **Auxiliary Index structures**: HNSW/IVF layers (which are derived deterministic properties).
- **Runtime caches**.
- **Timestamps**: Wall-clock times are never hashed.

Any state outside the `KernelState` struct is considered "Ephemeral" or "derived" and is not part of the cryptographic proof.

### 2. Snapshot Hash `H(S)`

The hash of the **Canonical Snapshot Encoding**.
This is the SHA-256/BLAKE3 hash of the binary bitstream of the `snapshot.bin` file.

### 3. WAL Hash `H(W)`

The hash of the **Command Log**.
The WAL file **MUST** start with a 16-byte header, followed by the sequence of commands.

**WAL Header Format (Little Endian):**
| Offset | Field | Type | Description |
|---|---|---|---|
| 0 | Version | u32 | Format version (currently 1) |
| 4 | Encoding | u32 | Command encoding (1 = Bincode) |
| 8 | Dim (D) | u32 | Vector dimension (must match Snapshot) |
| 12 | CksumLen | u32 | Length of checksum (0 if unused) |

Commands are hashed in strict sequence as they appear in the WAL file (excluding header? No, usually hash *content*).
*Clarification*: `wal_hash()` hashes the **entire file content** (Header + Commands) to ensure the header is also tampered-proof.
`replay_and_hash` validates the header before processing.

## Deterministic Proof Structure

A formatted proof looks like this:

```json
{
  "kernel_version": 1,
  "snapshot_hash": "a1b2...",
  "wal_hash": "c3d4...",
  "final_state_hash": "e5f6..."
}
```

This proof serves as a receipt. If a user trusts `snapshot_hash` and `wal_hash` (e.g., via blockchain commitment or trusted source), they can mathematically verify `final_state_hash` by running the verification tool.

## The `valori-verify` Tool

We provide a standalone, zero-trust CLI tool to verify proofs offline.

```bash
cargo run --bin valori-verify -- snapshot.bin wal.bin
```

This tool:
1.  Loads the snapshot.
2.  Replays the WAL commands deterministically using the embedded exact-math kernel.
3.  Computes the final hash.
4.  Outputs the JSON proof.

You can verify this output against the server's claimed state hash.

## Protocol V1 Constants

For the current `valori-verify` binary (Version 1), the following constants are frozen:

| Constant | Value | Description |
|---|---|---|
| `MAX_RECORDS` | 1024 | Maximum records in pool |
| `MAX_NODES` | 1024 | Maximum graph nodes |
| `MAX_EDGES` | 2048 | Maximum graph edges |
| `D` (Dimension) | 16 | Fixed-point vector dimension |
| `Q` (Quantization) | Q16.16 | Fixed-point precision |

Future versions may read these from the snapshot metadata or support dynamic dispatch.
For now, proofs are implicitly tied to this configuration.

## Safety & Limitations

- **Floating Point**: We strictly avoid native `f32` operations in the kernel. All math is `Q16.16`.
- **Concurrency**: The kernel is single-threaded. Command order is strictly serialized by the WAL.
- **Non-Guarantee**: Changes to the implementation of `valori-verify` or the hashing algorithm itself (e.g., v1 -> v2) will change the hash. Proofs are valid only relative to a specific `kernel_version`.

