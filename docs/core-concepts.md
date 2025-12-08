# Core Concepts

Valori Kernel makes specific engineering tradeoffs to prioritize determinism and portability over raw flexibility. Understanding these concepts is key to using the kernel effectively.

## 1. Determinism & Portability

The primary goal of Valori is to guarantee that **State A + Command B = State C** is bit-identically true on *every* computer.

*   **The Problem**: Floating point math (`f32`) behaves differently on x86 vs ARM, and even with different compiler flags (e.g., FMA optimizations).
*   **The Solution**: We forbid `f32` in the core logic.

## 2. Fixed-Point Math (FXP)

Instead of floats, Valori uses **Fixed-Point Arithmetic** (specifically Q16.16).

*   **What is it?**: A real number is stored as an integer.
    *   `1.0` is stored as `65536` (`1 << 16`).
    *   `0.5` is stored as `32768`.
*   **Implication**: Inputs (from Python/Node) are converted from float to FXP integers immediately upon entering the kernel. All internal `dot_product` and `distance` calculations happen using integer instructions.
*   **Benefit**: Integer math is standardized across all CPUs.

## 3. Static Memory Model (`no_std`)

The kernel does not use the system allocator (heap) during runtime operations.

*   **Pools**: Records, Nodes, and Edges live in pre-allocated static arrays (Pools).
*   **References**: We don't use pointers. We use IDs (`u32` indexes) to refer to objects.
*   **Why?**: This makes the memory footprint predictable and allows the kernel to run in environments without an allocator (e.g., embedded devices, WASM).

## 4. Knowledge Graph

Valori isn't just a vector DB; it's a "Memory OS". It links raw vectors to semantic concepts.

*   **Record**: A raw vector embedding (e.g., from an LLM).
*   **Node**: A semantic entity (e.g., "User", "Conversation", "File"). A Node *may or may not* point to a Record.
*   **Edge**: A directed link between Nodes (e.g., "User" -> "OWNS" -> "File").

This allows for hybrid queries: "Find vectors near query Q, but only connected to Node N."

## 5. Snapshots

Because the state is deterministic and compact:

1.  We can serialize the entire memory block to a binary blob (`snapshot`).
2.  We can load that blob into a fresh kernel (`restore`) and resume exactly where we left off.
