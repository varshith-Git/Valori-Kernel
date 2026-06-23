# Phase C4.3 — Contradiction detection (self-maintaining memory, pillar 3)

## Goal

Detect when two memories conflict and record the verdict as a committed,
auditable graph edge. This is the third self-maintaining-memory pillar (decay →
consolidation → **contradiction**), and the direct fix for the broken C3
detector, which fired on cosine *similarity* (agreement) into a review queue that
did not exist. C4.3 makes the verdict a first-class hashed event.

## Delivered

### New EdgeKind — `crates/valori-kernel/src/types/enums.rs`

| Variant | Value | Meaning |
|---|---|---|
| `EdgeKind::Contradicts` | 8 | New record contradicts an existing one (NLI verdict). |

(`Supersedes = 7` lands in the same edit for C4.2.) `from_u8` round-trips both;
`no_std`-safe.

### Cosine similarity over Q16.16 — `crates/valori-node/src/engine.rs`

`Engine::cosine_similarity(id_a, id_b) -> Option<f32>` reads both records' fixed-
point vectors, computes `dot(a,b) / (|a|·|b|)` via the kernel's `dist::dot_product`,
and returns `None` if either record is missing, non-searchable, or zero-magnitude.
The cluster path inlines the same math against state-machine records
(`cosine_similarity_from_records`).

### Standalone endpoint — `POST /v1/memory/contradict` (`crates/valori-node/src/server.rs`)

Request `{ record_a, record_b, threshold?, collection? }` (threshold default
0.85). The handler computes similarity under a read lock; if it meets the
threshold it takes the write lock and commits chunk nodes for both records plus an
`AutoCreateEdge(record_a → record_b, Contradicts)`. Below threshold, nothing is
written.

Response `{ record_a, record_b, similarity, contradicts, edge_id?, state_hash }`
— `edge_id` present only when `contradicts` is true.

### Cluster endpoint — `POST /v1/memory/contradict` (`crates/valori-node/src/cluster_server.rs`)

Same surface, backed by Raft. Similarity is computed in a `with_state` read
closure; if it contradicts, nodes + edge are committed via `raft_write_data` with
allocated IDs threaded from the apply responses (no pre-read race). Gated behind
the readiness check so a catching-up node does not answer from stale state.

### Python SDK — `python/valoricore/remote.py`

`contradict(record_a, record_b, threshold=, collection=)` on all four clients;
cluster variants route to the leader.

## Findings

- **"Contradiction" here is a structural proxy, not semantic NLI.** The C4 plan
  describes claim-level natural-language inference; this phase ships cosine
  similarity ≥ threshold, which detects *near-duplicate* vectors, not logically
  opposed claims. The kernel/graph machinery (the `Contradicts` event in the
  hashed chain) is the durable part and is signal-agnostic: a real NLI score can
  replace the cosine gate at the node layer with **zero kernel change**. This is
  a deliberate v1 boundary, documented so it is not mistaken for full NLI.
- High cosine similarity actually indicates *agreement/duplication*. Using it as
  the contradiction trigger is a stand-in to exercise the event path; teams
  wiring a true entailment model should invert/replace the gate. Flagged
  prominently so C4.3 is not shipped to users as semantic contradiction.
- Read-then-write (similarity under read lock, edge under write lock) means a
  concurrent delete between the two phases could leave a Contradicts edge to a
  just-deleted record. Acceptable: soft-deleted records remain in the chain.

## Validation

- `cargo test -p valori-node` — 193 passed, 0 failed.
- `cargo test -p valori-kernel -p valori-consensus` — 112 passed, 0 failed.
- `cargo build -p valori-kernel --target wasm32-unknown-unknown` — clean.
- `remote.py` parses.
- Manual smoke test (standalone): insert two near-identical vectors,
  `POST /v1/memory/contradict` → `contradicts: true`, `edge_id` set; insert two
  orthogonal vectors → `contradicts: false`, no edge, no state-hash change.

## Follow-ups

- Replace the cosine gate with a real claim-level NLI signal (entailment model
  output) at the node layer; the `Contradicts` event and graph traversal stay
  as-is. Owner: future NLI phase.
- Surface contradiction edges in GraphRAG receipts so an answer can carry "these
  two cited memories conflict" provenance.
