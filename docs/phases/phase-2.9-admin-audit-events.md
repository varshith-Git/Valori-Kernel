# Phase 2.9 — Admin Audit Events in the Chain

**Status:** done · on `multinode`
**Roadmap:** Phase 2 (cluster mode), sub-phase 9 of 10

## Goal

Implement the Phase 1.6 principle: **any action that changes cluster
membership is a first-class entry in the same BLAKE3 hash chain as the
data.** Membership cannot change without it appearing — tamper-evidently —
between the data events it interleaves with.

## Delivered

**valori-wire** — `LogEntry::Admin(AdminEvent)`, the third (append-only)
variant, plus the `AdminEvent` enum: `NodeJoined { node_id, raft_addr,
api_addr, authorized_by }` and `NodeLeft { node_id, authorized_by }`.
`authorized_by` is the credential hash at action time — rotating the
credential later does not change historical attribution; all-zeros means
"no authentication configured" (until Phase 3 RBAC). The remaining 1.6
events (CertRotated, TenantKey*, EraseRecord, ClusterCaRotated) are
documented as the next append slots.

**cluster_api** — `cluster_router` now takes an optional audit-writer
handle; successful `add-node` / `remove-node` append the matching
`AdminEvent` to the chained log. A failed audit append is logged, not
surfaced: the membership change has already committed through Raft (the
source of truth) and a full audit disk must not unwind it.

**Every LogEntry consumer updated** (the compiler found them all):
verifier replay (chain-verifies admin entries, prints them under
`--trace`, never applies them to kernel state), node recovery + event-log
reopen (chained, skipped for state), CLI timeline (magenta `Admin` rows) +
inspect + forensic engine.

## Findings

- The append-only evolution policy did its job: the v2/v3-era fixtures in
  `evolution.rs` decode unchanged with the new variant present — the only
  edits were new match arms, which the compiler enumerated exhaustively.
  This is the first real exercise of the policy since it was written.

## Validation

- `evolution.rs::admin_events_chain_and_roundtrip` — encode/decode/chain
  two admin events at the wire level; fixtures still decode forever.
- `cluster_api.rs::membership_changes_are_chained_admin_events` — a real
  cluster join + removal through the HTTP API, then a raw walk of the
  on-disk log: **chain unbroken across data and admin entries**, both
  events present, in order, with their addresses.
- Full workspace: **242 passing, 0 failures.**

## Follow-ups

- Phase 3 (RBAC): populate `authorized_by` from the admin token hash;
  emit `TenantKeyCreated/Revoked`, `EraseRecord`.
- Phase 2.10: `CertRotated` lands with mTLS rotation.
- valori-verify report JSON could count admin events separately
  (`admin_events_seen`) — cosmetic, with the next verifier touch.
