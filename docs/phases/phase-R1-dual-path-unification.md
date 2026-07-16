# Phase R1 — Dual-path unification: shared handlers + route-parity guard

## Goal

Kill the dual-path bug class documented in CLAUDE.md ("an endpoint added to
`server.rs` but not `cluster_server.rs` silently 404s — no compile error, no
test failure"). Establish the shared-handler pattern so endpoint logic is
written once and served by both routers, and add an automated guard that makes
any future route divergence a test failure.

## Delivered

| File | What |
|---|---|
| `crates/valori-node/tests/route_parity.rs` (new) | Parses the `.route()` declarations out of both server source files and asserts the `/v1` route sets — paths AND methods — are identical, modulo explicit `STANDALONE_ONLY` / `CLUSTER_ONLY` / `METHOD_GAPS` allowlists with documented reasons. Allowlist staleness is itself asserted (an entry that stops being one-sided fails the test). 2 tests. |
| `crates/valori-node/src/routes/mod.rs` (new) | The shared-handler pattern: per-domain `*Ops` trait for state-touching primitives; validation + response shaping written once as generic functions; both routers wire 3-line wrappers. Module doc explains the convention for future domains. |
| `crates/valori-node/src/routes/collections.rs` (new) | First migrated domain: `POST/GET /v1/namespaces`, `DELETE /v1/namespaces/:name`. `CollectionOps` trait (`resolve` / `create` / `drop_collection` / `list`) + shared `create_collection` / `list_collections` / `drop_collection` handler bodies with canonical M-2 name validation and status codes. |
| `crates/valori-node/src/server.rs` | Collection handlers replaced with wrappers; `impl CollectionOps for SharedEngine` (existence check + create under one write lock, so `created` stays race-free on this path). |
| `crates/valori-node/src/cluster_server.rs` | Collection handlers replaced with wrappers; `impl CollectionOps for DataPlaneState` (writes via `raft_write_data`, reads via state machine). The four private duplicate DTO structs (`CreateCollectionRequest/Response`, `CollectionInfo`, `ListCollectionsResponse`) deleted in favor of the canonical `api.rs` types. |
| `crates/valori-node/src/lib.rs` | `pub mod routes;` |
| `crates/valori-node/tests/collections.rs` | `drop_unknown_collection_is_400` → `drop_unknown_collection_is_404` (see Findings). |

## Findings

1. **The routers had already diverged in contract.** Dropping an unknown
   collection returned **400** on standalone but **404** on cluster — both
   asserted by their own test suites, so neither suite could see the
   disagreement. Canonicalized to 404 (correct HTTP semantics; matches
   cluster).
2. **The cluster path skipped M-2 name validation entirely.** Standalone
   rejected empty / >64-char / non-`[a-zA-Z0-9_-]` collection names; cluster
   committed them straight through Raft. Fixed by construction — validation
   now lives in the shared handler.
3. **Wire types were duplicated, not shared.** `cluster_server.rs` carried
   private copies of the collection DTOs instead of using `api.rs` — the same
   mechanism that produced the S12 graph wire-format incompatibility.
4. **Intentional asymmetries are now written down.** The parity test's
   allowlists document exactly which 10 routes are standalone-only, which 2
   are cluster-only, and the one method gap (`DELETE /v1/graph/node/:id` has
   no cluster implementation — open follow-up), each with a reason.

## Validation

- `cargo test -p valori-node` — **225 passed, 0 failed** (route_parity 2/2,
  collections 16/16, cluster_namespaces 17/17).
- `cargo test -p valori-kernel` — **66 passed, 0 failed**.
- `raft_metrics_appear_in_prometheus_output` flaked once under full-suite
  parallel load (in-memory Raft election timing); passes deterministically in
  its own suite. Pre-existing, unrelated to this phase.

## Follow-ups

- **Migrate the remaining domains** into `routes/` group by group: graph
  (node/edge/subgraph — includes deciding cluster semantics for
  `DELETE /v1/graph/node/:id`), records/delete, memory ops, proof/timeline,
  index/crypto/keys. Each migration deletes a duplicated body from both
  server files; the parity test holds the line meanwhile.
- **Cluster `DELETE /v1/graph/node/:id`** — currently in `METHOD_GAPS`;
  needs cascade-delete semantics as a Raft event.
- The parity test parses single-line `.route()` declarations by convention;
  if route formatting ever changes, its count sanity-checks (>40 routes per
  file) fail loudly rather than silently missing routes.
