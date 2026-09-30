# Architecture rules

**Source of truth:** [`docs/architecture/layers.md`](../../docs/architecture/layers.md) — "Normative Architecture
Document." It already has the dependency graph, global invariants, a "Never do this" section, and
per-crate ownership. Read it before any structural change. This file is a pointer + the operational
checklist Claude specifically needs; it does not restate `layers.md`'s content.

Also read [`docs/architecture/control-plane.md`](../../docs/architecture/control-plane.md) before touching
anything under `crates/valori-daemon` or `desktop/` — it defines the daemon (local process lifecycle,
desktop-only) vs. node (data plane) boundary. **This repo has no SaaS/billing control plane** — no
Postgres, no Supabase, no tenant provisioning. If a task description assumes those exist here, it is
describing the separate `valori-ui` repo (`backend/`), not this one. Don't invent that infrastructure here.

## System boundaries (verified)

```
HTTP client (SDK / CLI / UI)
  → valori-node (server.rs standalone, or cluster_server.rs + openraft)
      → valori-consensus (ValoriStateMachine, cluster mode only)
      → valori-kernel (KernelState::apply_event_ns — the only place a KernelEvent is created)
          → valori-storage (WAL / event log / object store)
          → valori-metadata (redb: project config, collection mappings, shard topology)
```

Standalone vs. cluster is a real fork, not a detail — see `AGENTS.md`'s "single-node AND multi-node"
section before adding any endpoint. Every new endpoint needs both `server.rs` and `cluster_server.rs`,
enforced mechanically by `cargo test -p valori-node --test route_parity`.

## Durable vs. ephemeral state (verified against `docs/architecture/layers.md` layer ownership)

- **`valori-storage`** owns bytes on disk: WAL, append-only event log (V4), object-store backend (S3/file).
- **`valori-metadata`** (redb) owns control-plane-*for-this-node* persistence: project config, collection
  name→`NamespaceId` mappings, shard topology, snapshot catalog, execution history, planner cache.
- **`valori-consensus`** (redb, cluster mode) owns the Raft log + vote state, separate from `valori-metadata`.
- Object storage (`VALORI_OBJECT_STORE_URL`) owns durable snapshot/WAL archival across node loss — see
  `docs/DR.md`.
- Nothing owns "the" state except the reconstruction path in `INVARIANTS.md` I-11: snapshot + replay.
  Reading raw storage files any other way to synthesize state is prohibited.

## Trust boundaries

Client → `valori-node` HTTP (optionally `VALORI_AUTH_TOKEN` bearer) → Raft gRPC between peers (optionally
mTLS via `VALORI_TLS_*`) → object storage (S3 credentials). See `docs/THREAT_MODEL.md` for what's
explicitly in and out of scope (plaintext HTTP and AuthN/Z are currently **out of scope** per that doc —
don't assume protections that aren't there).

## Change policy

Before an architectural change (new crate dependency, new cross-crate call path, moving a responsibility
between crates):
1. Check `docs/architecture/layers.md`'s dependency graph — does the change point an edge the wrong
   direction (e.g. `valori-kernel` depending on something with `std`-only requirements)?
2. Identify which layer now owns what changed; update `layers.md`'s ownership table if it moved.
3. If it touches `valori-kernel`, re-read `.claude/rules/determinism.md` first — this is the most
   invariant-dense crate in the workspace.
4. Add/update the relevant integration test (`route_parity`, `api_contract.rs`, etc.) — don't rely on
   manual review to catch a boundary violation that a mechanical test already covers elsewhere.
