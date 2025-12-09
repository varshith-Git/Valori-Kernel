# Core Concepts

Valori Kernel makes specific engineering tradeoffs to prioritize determinism and portability over raw flexibility. Understanding these concepts is key to using the kernel effectively.

## 1. Determinism & Portability

The primary goal of Valori is to guarantee that **State A + Command B = State C** is bit-identically true on *every* computer.

*   **The Problem**: Floating point math (`f32`) behaves differently on x86 vs ARM, and even with different compiler flags (e.g., FMA optimizations).
*   **The Solution**: We forbid `f32` in the core logic.

## 2. Fixed-Point Math (FXP)
Valori is not just a vector database. It is a **Deterministic Memory Engine** that fuses **Semantic Vectors** with a **Knowledge Graph**.

This hybrid approach allows AI agents to "remember" in two ways:
1.  **Similarity (Vague)**: "Find things related to 'apples'."
2.  **Structure (Precise)**: "Find the exact object linked to 'User:Alice' via 'Edge:Owns'."

---

## 🏗️ The Data Model

### 1. The Record (Vector)
The fundamental atomic unit of memory.
*   **What it is**: A dense fixed-point vector (e.g., 16-dim or 768-dim) representing meaning.
*   **Storage**: Stored in a contiguous memory pool for O(1) access.
*   **Addressing**: Identified by a `RecordId` (integer).

### 2. The Knowlege Graph
A lightweight graph overlay sitting on top of the vectors.
*   **Node**: A semantic entity. Can be a `Document`, a `Chunk`, a `User`, or a `Task`.
    *   *Note*: A Node implementation *points* to a Record. This means every node in the graph has a "semantic embedding" attached to it.
*   **Edge**: A directed link between nodes.
    *   Example: `Document (Node A)` -> `ParentOf` -> `Chunk (Node B)`.

### 3. The Index
The mechanism for finding records.
*   **Brute Force (Exact)**: Scans every record. Guaranteed 100% recall. Best for datasets < 1M.
*   **HNSW (Approximate)**: Navigate a graph of vectors. (Coming Soon).

---

## 🛡️ Determinism & Fixed-Point Math

Traditional databases use `float32` or `float64`. This is bad for distributed systems because `0.1 + 0.2 != 0.3` on all chips.

**Valori used Fixed-Point Math (Q16.16):**
*   We treat numbers like integers.
*   `1.0` is stored as `65536`.
*   Addition/Multiplication is just integer math.
*   **Result**: If you run Valori on a Raspberry Pi and a Supercomputer, the resulting database binary will be **identical bit-for-bit**.

This enables:
*   **Verifiable AI**: Prove that an agent's memory hasn't been tampered with.
*   **Instant Sync**: Sync state by just sending the binary snapshot. No "replication logs" needed.
