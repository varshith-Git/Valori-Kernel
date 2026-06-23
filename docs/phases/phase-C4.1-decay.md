# Phase C4.1 — Kernel-native time decay (self-maintaining memory, pillar 1)

## Goal

Make the database itself recency-aware. Give every client — HTTP, Python SDK,
and the MCP agent-memory wedge — a read-time decay re-rank so older memories
fade in ranking, *without* breaking determinism, replication, or the audit
chain. This is the first of three self-maintaining-memory pillars (decay →
consolidation → contradiction) rebuilt where they belong: in the node, not in a
single-user Next.js upload route (see the C3 critique below).

## Why this replaces the old "Cortex" C3

The shipped C3 "self-maintaining memory" was UI-layer TypeScript: it lived only
in `ui/src/app/api/ingest/route.ts`, so the SDK, the MCP wedge, and the raw API
got none of it; its state sat in a non-replicated metadata sidecar (invisible to
the audit chain it was meant to be the moat *for*); its contradiction detector
fired on cosine *similarity* (agreement), not contradiction, into a review queue
backed by a `meta/list` endpoint that does not exist (always returns `[]`); and
it had **no decay at all**. C4 rebuilds these as node-native capabilities. C4.1
delivers decay.

## Delivered

### Decay core — `crates/valori-node/src/decay.rs` (new)

A pure, unit-tested module both search paths call:

| Item | Purpose |
|---|---|
| `decay_factor(age, half_life) -> f64` | Geometric weight `0.5^(age/half_life)` ∈ (0,1]; `half_life = 0` disables. |
| `DecayHit { id, distance, created_at }` | Input candidate (L2 distance, optional creation time). |
| `DecayedHit { id, distance, factor, age_secs }` | Output: **original** distance preserved, factor + age reported. |
| `rerank(hits, now, half_life, k)` | Over-fetched pool → re-rank by `distance / factor` → top-k, stable by id. |

For L2 distance (lower = better), age penalises by **inflating** distance:
`adjusted = distance / factor`. A record one half-life old has its effective
distance doubled, so a fresh near-match can overtake a stale better one. Unknown
or future creation times → factor 1.0 (we never penalise what we cannot date).

### Determinism (the load-bearing property)

Decay is a **read-time re-rank**. It never mutates kernel state, never emits a
committed event, and never touches the BLAKE3 state hash. Creation time lives in
`Engine.created_at: HashMap<u32,u64>` — a *derived* map in the same category as
`record_to_node` (not hashed, not persisted), stamped on **live inserts only**
(`insert_record_from_f32_ns`, `insert_batch_ns`), never during recovery replay.
Test `decay_does_not_mutate_state_hash` pins this: the `as_of_state_hash` is
byte-identical with and without decay.

### Endpoints

`POST /search` and `POST /v1/memory/search_vector` accept an optional
`decay_half_life_secs`. When set (> 0, or via the `VALORI_DECAY_HALF_LIFE_SECS`
server default), results are decay-re-ranked and each hit gains `decay_factor`
and `age_secs`. `score` stays the **true, undecayed** distance for honesty.
Absent / `0` → byte-identical to the old response (no decay fields serialized).
Decay is intentionally **not** applied to `as_of` point-in-time queries.

### Config — `VALORI_DECAY_HALF_LIFE_SECS`

Optional global default half-life. A request value wins (including an explicit
`0` to disable per-call).

### Python SDK — all four clients

`search(..., decay_half_life_secs=None)` added to `SyncRemoteClient` /
`AsyncRemoteClient`; the cluster clients forward it via `**kwargs`. Hits carry
`decay_factor` / `age_secs` when decay is active.

### MCP wedge — `memory_recall`

The `memory_recall` tool now takes `decay_half_life_secs`; the `NodeClient`
trait, `HttpBackend`, and the RECALL arm thread it through. The receipt still
verifies over the (decayed) result set — recency-aware recall *with* proof.

## Findings

- **Cluster decay is deferred to C4.1b.** Per-record creation time is tracked in
  the standalone `Engine`; the cluster data plane reads `KernelState` directly
  via the consensus state machine and has no equivalent. The cluster `/search`
  accepts `decay_half_life_secs` for wire-compatibility (one SDK call works
  against both node types) but currently treats it as neutral. Wiring it means
  tracking creation time in `ValoriStateMachine` — the most invariant-sensitive
  crate — so it gets its own phase.
- **Cross-restart durability is a known v1 boundary.** `created_at` is in-memory
  and starts empty after a restart, so recovered records rank neutrally until
  re-stamped. This is *safe* (never over-penalises) and honest; the durable fix
  (persist commit timestamps in the WAL and restore them) is a follow-up. It is
  deliberately **not** the sidecar approach the C3 critique flagged.
- **Exact matches (distance 0) don't decay below other exact matches** —
  `0 / factor == 0`. Documented; a tunable additive-penalty formulation is a
  possible follow-up.

## Validation

```
cargo test -p valori-kernel -p valori-node -p valori-mcp
kernel 50 + node 193 + mcp 34 = 277 passing, 0 failing   (was 265)
```

New tests:
- `valori-node` lib `decay::tests` (7) — factor endpoints; fresh-beats-old under
  a short half-life; unknown age neutral; future timestamp not penalised; huge
  half-life preserves distance order; truncate + stable tie-break; exact match
  not dragged.
- `valori-node/tests/api_decay.rs` (4) — no-decay is clean/backward-compatible;
  decay reports factor + ages; **decay does not mutate the state hash**; explicit
  `0` disables.
- `valori-mcp/tests/integration_node.rs` (+1) — `memory_recall` with decay
  against a real node returns weighted hits **and** a verifiable receipt.
- `python/tests/test_decay_sdk.py` (new) — sync + async SDK decay round-trip:
  `sync: plain=3 decayed=3 factor0=1.0000 OK`.

Manual: `python3 python/tests/test_decay_sdk.py` → PASS.

## Follow-ups

| Item | Phase |
|---|---|
| Cluster decay — track creation time in `ValoriStateMachine` | C4.1b |
| Durable creation time — persist commit timestamps in the WAL, restore on recovery | C4.1b |
| **Consolidation** — scheduled merge → `SoftDeleteRecord(parents)` + `AutoInsertRecord(summary)` + `RefersTo` edges, all committed | C4.2 |
| **Contradiction** — claim-level NLI, verdict committed as a `RefersTo` edge event, surfaced via a real graph query (replaces the dead `meta/list` queue) | C4.3 |
| Graph-proximity re-rank composing with decay | C4.x |
