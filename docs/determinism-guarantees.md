# Valori Determinism Guarantees

This document serves as the formal specification for Valori's determinism.
If two Valori nodes report identical proofs, their memory state is **mathematically identical**.

## Guaranteed Deterministic Operations

The following operations are guaranteed to be bit-identical across all supported platforms (x86_64, ARM64, WASM) and OSs (Linux, macOS, Windows):

1.  **Vector Storage**: Vectors are stored as Q16.16 fixed-point integers.
    *   Input `0.1` is converted to `6553` (0x1999) on all platforms.
    *   Rounding and clamping logic is strictly defined.
2.  **L2 Distance Calculation**: Square distances are computed using `i32`/`i64` integer arithmetic.
    *   No FMA (Fused Multiply-Add) variance.
    *   No non-associativity of float addition.
3.  **Graph Construction**:
    *   Node IDs are allocated sequentially based on "First Free Slot" logic.
    *   Edge links are deterministic given the same order of operations.
4.  **Indexing (Brute Force)**:
    *   Search results are sorted by (Score, ID).
    *   Ties are broken deterministically by ID (ASC).

## Non-Guarantees (Explicit Non-Goals)

Valori does **NOT** guarantee determinism for:

1.  **Neural Inference**: The upstream embedding models (e.g. PyTorch/ONNX) are often non-deterministic across GPUs/CPUs. Valori guarantees storage *after* the vector is produced.
2.  **Wall-Clock Timing**: Response latency and query throughput.
3.  **Unordered Ingestion**: If client A sends Record 1 then 2, and client B sends Record 2 then 1, the resulting internal state (Slot IDs) **WILL** differ, although semantic search results may be equivalent.
    *   *Note*: This will result in **Mismatching Proofs**, which is correct behavior (History Divergence).

## Formal Proof Definition

A node $N$ is valid if and only if:

$$
State(N) = \prod_{i=0}^{T} Apply(Command_i)
$$

Where:
*   $Command_0 \dots Command_T$ is the immutable WAL.
*   $Apply$ is the pure kernel transition function.

Any deviation from this function (bit rot, hardware error, software bug) is considered a critical failure and will be detected by `valori-verify`.
