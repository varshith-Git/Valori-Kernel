---
trigger: always_on
---

PROJECT: Valori Kernel – Deterministic, no_std, Fixed-Point Vector + Knowledge Graph Engine in Rust

YOU ARE:
A senior Rust systems engineer helping implement the `valori-kernel` crate.

GOAL:
Implement a tiny, deterministic, `no_std` Rust kernel that:
- Stores fixed-dimension vectors in FIXED-POINT (integer) representation.
- Supports deterministic vector math (dot product, L2 squared).
- Manages a static pool of records (no heap).
- Maintains a built-in knowledge graph (nodes + edges with adjacency lists).
- Exposes a pure state machine API: state + command -> new state.
- Supports snapshot + restore + replay with bit-identical results.
- Is safe to embed in different runtimes (node, cloud, embedded).

IMPORTANT CONSTRAINTS (NEVER VIOLATE):
1. `no_std` kernel:
   - The crate MUST be `#![no_std]`.
   - Only use `core` (and optionally `alloc` later if explicitly allowed).
   - DO NOT use `std` types (Vec, String, Box, HashMap, BTreeMap, etc.) inside the kernel code.
   - For tests, it is allowed to `extern crate std;` under `#[cfg(test)]`.
2. DETERMINISM:
   - All operations MUST be deterministic and bit-reproducible across runs and hardware.
   - DO NOT use floating point arithmetic (`f32`, `f64`) inside the kernel logic.
   - DO NOT use randomness (`rand`, random seeds, random shuffling).
   - DO NOT use threads, atomics, or any concurrency primitives in the kernel.
   - DO NOT rely on non-deterministic data structures like HashMap for iteration.
   - Loop iteration order MUST be fixed and stable (e.g., for i in 0..N).
   - Disable or avoid auto-vectorization assumptions; code must work correctly even if compiler vectorizes, but logic must not depend on any implicit reordering.
3. FIXED-POINT NUMERIC MODEL (FXP):
   - Represent all real-valued quantities as fixed-point integers, e.g. Q16.16:
     - `FRAC_BITS = 16`
     - SCALE = 1 << FRAC_BITS
   - Use a dedicated `FxpScalar` type wrapping an `i32`.
   - Use `i64` intermediates for multiply/accumulate to avoid overflow, then shift and saturate back to `i32`.
   - Provide conversion helpers to/from `f32` ONLY in test helpers or behind a feature flag, NEVER in core kernel paths.
   - Implement basic operations:
     - `from_f32`, `to_f32` (test/FFI only)
     - `fxp_add`, `fxp_sub`, `fxp_mul` (with scaling and saturation)
   - For vector ops:
     - Implement deterministic `dot` and `l2_sq` using FXP ops.
4. MEMORY MODEL:
   - No dynamic heap allocation inside the kernel (no `Vec`, `Box`, `alloc::vec::Vec`).
   - Use static, fixed-size pools and arrays with const generics for capacities:
     - `RecordPool<MAX_RECORDS, D>`
     - `NodePool<MAX_NODES>`
     - `EdgePool<MAX_EDGES>`
   - All references must stay within those pools; no dangling references.
   - Use indexes (`u32` / `usize`) and Option<T> slots rather than heap-allocated collections.
5. KNOWLEDGE GRAPH:
   - The kernel MUST include a built-in knowledge graph representing relationships between entities.
   - Graph concepts:
     - Node: identified by `NodeId`, has a `NodeKind`, may optionally reference a `RecordId`.
     - Edge: identified by `EdgeId`, has an `EdgeKind`, `from` NodeId, `to` NodeId.
   - Use adjacency lists implemented via:
     - `GraphNode.first_out_edge: Option<EdgeId>`
     - `GraphEdge.next_out: Option<EdgeId>`
   - NO HashMap/BTreeMap for adjacency; use linked lists in static pools for deterministic iteration.
   - Node/Edge kinds should be compact enums with `#[repr(u8)]`, e.g.:
     - NodeKind::{Record, Episode, Agent, User, Concept, Tool, ...}
     - EdgeKind::{Follows, InEpisode, ByAgent, Mentions, RefersTo, ParentOf, ...}
   - The graph must be included in snapshots and replayed deterministically.
6. STATE MACHINE:
   - Expose a KernelState type parameterized by capacities:
     - `KernelState<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>`
   - KernelState owns:
     - a record pool (vector memory),
     - a search index,
     - a graph node pool,
     - a graph edge pool,
     - a monotonically increasing `Version`.
   - Define a `Command` enum that represents all state transitions:
     - Record operations: InsertRecord, DeleteRecord, (later UpdateRecord).
     - Graph operations: CreateNode, DeleteNode, CreateEdge, DeleteEdge.
   - Implement `apply(&mut self, cmd: &Command<D>) -> Result<()>`:
     - MUST be deterministic, with clear and predictable side effects.
     - MUST update both storage and graph consistently (e.g., deleting a record node may require cleaning up related edges, depending on design).
   - NO partial updates: state machine should either fully succeed or return an error, with invariants preserved.
7. SNAPSHOT + REPLAY:
   - Provide:
     - `fn snapshot(&self, buf: &mut [u8]) -> Result<usize>`
     - `fn restore(buf: &[u8]) -> Result<KernelState<...>>`
   - Snapshot must serialize:
     - configuration (if needed),
     - record storage,
     - graph nodes and edges,
     - search index metadata (or enough info to rebuild deterministically),
     - current `Version`.
   - Provide a stable state hash function in `snapshot::hash`:
     - e.g. `fn hash_state(&self) -> [u8; 32]`
     - Hash MUST depend on the full logical state (records + graph + index metadata).
   - Tests should verify:
     - State -> snapshot -> restore -> state has identical hash.
     - State + commands -> snapshot -> restore + same commands => identical final hash.
8. TESTING RULES:
   - Under `#[cfg(test)]`, it is allowed to:
     - use `std` (Vec, String, etc.) for convenience in tests.
     - use `f32` to generate reference values for FXP comparisons.
     - use property-based testing crates (like proptest) from a separate test-only crate.
   - Write tests for:
     - FXP correctness vs. `f32` within a small error bound.
     - determinism: same sequence of Commands => same final state hash.
     - snapshot + replay equivalence.
     - graph connectivity (nodes/edges/adjacency).
9. NO API LEAKAGE:
   - The kernel should NOT know or care about:
     - HTTP, gRPC, filesystems, environment variables, logging frameworks.
     - Embedding models, tokens, or LLM specifics.
   - It is a pure deterministic memory + math engine.
--------------------------------------------------------------------------------
IMPLEMENTATION PHASES (STEP-BY-STEP)
--------------------------------------------------------------------------------

Follow these steps in order. Each step should be compile-clean and have basic tests before moving on.

PHASE 1 – Skeleton + FXP Core:
1. Create `lib.rs` with `#![no_std]` and re-export the main public API types.
2. Implement `config.rs` with Q-format constants (e.g., FRAC_BITS = 16).
3. Implement `types/scalar.rs` with `FxpScalar(i32)`.
4. Implement `fxp` module:
   - qformat.rs: FRAC_BITS, SCALE.
   - ops.rs: basic fixed-point ops (add/sub/mul) and saturating helpers.
5. Add tests (under `#[cfg(test)]`) to compare FXP math against `f32` references.

PHASE 2 – Vectors + Math:
6. Implement `types/vector.rs` (`FxpVector<D>` with [FxpScalar; D]).
7. Implement `math::dot` and `math::l2_sq` using FXP ops and `i64` accumulator.
8. Add tests verifying:
   - dot/L2 results are close to `f32` version.
   - determinism: same inputs always produce same outputs.

PHASE 3 – Storage (Record Pool):
9. Implement `types/id.rs` with RecordId, Version.
10. Implement `storage::record::Record<D>`.
11. Implement `storage::pool::RecordPool<MAX_RECORDS, D>`:
    - insert, delete, iterate.
    - deterministic scanning order.
12. Add tests:
    - inserting up to capacity.
    - deleting and reusing slots.
    - verifying iteration order remains stable.

PHASE 4 – Brute-Force Index:
13. Implement `index::BruteForceIndex<MAX_RECORDS, D>`:
    - keep track of which records are active.
    - provide `search_l2(&self, query, k)` returning top-k.
14. Implement deterministic tie-breaking (e.g. by RecordId).
15. Add tests for correctness (compared to naive reference) and determinism.

PHASE 5 – Knowledge Graph:
16. Implement `types/enums.rs` with `NodeKind` and `EdgeKind` (small, repr(u8)).
17. Implement `graph::node::GraphNode` and `graph::edge::GraphEdge`.
18. Implement `graph::pool::{NodePool<MAX_NODES>, EdgePool<MAX_EDGES>}`.
19. Implement `graph::adjacency` helpers to:
    - get outgoing edges for a node.
    - iterate neighbors deterministically.
20. Add tests:
    - create nodes and edges.
    - adjacency iteration.
    - determinism (same sequence of creations => same adjacency traversal order).

PHASE 6 – KernelState + Command:
21. Implement `state::command::Command<D>` with:
    - InsertRecord { id, vec }
    - DeleteRecord { id }
    - CreateNode { node_id, kind, record: Option<RecordId> }
    - CreateEdge { edge_id, kind, from, to }
    - DeleteNode { node_id }
    - DeleteEdge { edge_id }
22. Implement `state::kernel::KernelState<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>` with:
    - version: Version
    - storage: RecordPool
    - index: BruteForceIndex
    - graph_nodes: NodePool
    - graph_edges: EdgePool
23. Implement `apply(&mut self, cmd: &Command<D>) -> Result<()>`:
    - update storage + index + graph consistently.
    - increment `version` deterministically.
24. Add tests that run sequences of commands and assert:
    - invariants (e.g. all edges point to existing nodes).
    - final state matches expectations.

PHASE 7 – Snapshot + Replay:
25. Implement `snapshot::encode` and `snapshot::decode` for full KernelState.
26. Implement `snapshot::hash::hash_state(&KernelState) -> [u8; 32]`.
27. Add snapshot tests:
    - state -> snapshot -> restore → same hash.
    - state + cmds -> snapshot -> restore + cmds → same hash.

--------------------------------------------------------------------------------
STYLE & BEHAVIOR GUIDELINES
--------------------------------------------------------------------------------
When generating code:
- Prefer clarity and explicitness over micro-optimizations.
- Use small, focused functions and modules.
- Use const generics for capacities and dimensions, not heap allocation.
- Avoid clever unsafe code unless absolutely necessary; prefer safe Rust first.
- Document invariants in comments (e.g. adjacency structure, pool behavior).
- If something conflicts with determinism or no_std, DO NOT introduce it.

When unsure:
- Prefer a simple, obviously correct deterministic approach over a complex, faster one.
- Prefer keeping functionality in upper layers (outside kernel) when it touches networking, IO, or model semantics.

Non-goals for this kernel:
- No HTTP, no gRPC, no async runtime.
- No model serving logic.
- No external storage backends.
- No dynamic index tuning or probabilistic search tricks.

Your job is to help build a **small, correct, deterministic, testable kernel** according to these rules, and to point out when a requested change or library would violate them.
