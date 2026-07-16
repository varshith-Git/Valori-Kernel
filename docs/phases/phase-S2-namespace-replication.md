# Phase S2 — Raft-replicated namespace/collection creation

Branch: `Node-scaleup` (S1 merged: `6d53924`).

## Goal

Fix a pre-existing correctness gap discovered while scoping S3 (namespace→shard
routing, not yet started): in cluster mode, collection creation was not
actually replicated — every node kept its own private, in-memory
`name → NamespaceId` map, independently assigned. Make collection creation go
through Raft, exactly like every other write, so every node ends up with the
identical mapping, deterministically and durably. No shard routing in this
phase — that remains a future S3.

## Delivered

### `crates/valori-kernel`

- **`src/event.rs`** — two new `KernelEvent` variants, `AutoCreateNamespace { name }`
  (index 14) and `DropNamespace { name }` (index 15) in the hand-written
  custom `Serialize`/`Deserialize` impl (`KernelEventHelper` intermediate
  enum updated to match). Both carry a name, not a pre-resolved id — the id
  doesn't exist until the consensus layer allocates it at apply time.
- **`src/state/kernel.rs`** — `apply_event_ns` gained two arms delegating
  entirely to the *existing* `Command::CreateNamespace`/`Command::DropNamespace`
  logic (kernel.rs:528-576) — zero duplicated validation/cascade-delete code,
  matching how `AutoInsertRecord`/`AutoCreateNode`/`AutoCreateEdge` already
  delegate to their `Command::` equivalents. The kernel never learns the
  name; it only ever sees the integer id the caller already resolved.

### `crates/valori-consensus`

- **`src/state_machine.rs`** — new private `ClusterNamespaceRegistry` struct
  (`HashMap<String, u16>` + `next_id`) added to `StateMachineInner` as a
  third replicated-but-unhashed side-table, in the same category as the
  existing `dedup_set`/`dedup_order` (not `created_at`/`text_corpus`, which
  are locally-computed-per-node and genuinely not identical across
  replicas — the namespace registry IS identical across replicas, by the
  same reasoning that makes dedup consistent: both mutate inside the single
  Raft-ordered `apply()` critical section). Threaded through `SnapshotPayload`,
  `new()`, `with_db()`, `install_snapshot()`, `build_snapshot()`. `apply()`
  gained namespace resolution/allocation logic positioned alongside the
  existing `pre_alloc_id` block, with rollback-on-kernel-rejection for
  `AutoCreateNamespace` (undoes a speculative registry insert if the
  downstream kernel apply somehow fails) and confirmed-success-only removal
  for `DropNamespace`. Two new read accessors: `resolve_namespace()`,
  `list_namespaces()`.
- **`src/types.rs`** — `ClientResponse` gained `allocated_namespace_id: Option<u16>`
  (append-only, `#[serde(default)]`), following the `allocated_record_id`/
  `allocated_node_id`/`allocated_edge_id` precedent.

### `crates/valori-node`

- **`src/cluster_server.rs`** — `DataPlaneState.namespaces: Arc<Mutex<NamespaceRegistry>>`
  (the old, per-node, unreplicated registry) removed entirely.
  `create_collection_handler`/`drop_collection_handler` rewritten to go
  through `raft_write()` with the new `KernelEvent` variants, mirroring the
  existing `insert_record` pattern exactly. `list_collections_handler`
  becomes a local read via `sm.list_namespaces()` (eventually consistent,
  matching every other list-style read in this file). All 8 existing
  `.resolve()` call sites repointed to `state.sm.resolve_namespace(...).await`.
  `cluster_ingest`'s auto-create-on-missing kept the convenience but now
  tries a local `resolve_namespace()` fast path first, only paying a Raft
  round trip the first time a name is created.
- Standalone mode (`engine.rs`/`server.rs`) is **untouched** — investigated
  and confirmed `Engine::create_collection`/`drop_collection` bypass the
  audit log entirely today (durability comes from a JSON sidecar file, not
  the WAL), a separate, pre-existing, intentional design choice unrelated to
  the cross-node divergence problem this phase fixes (standalone has exactly
  one registry, no multi-node disagreement is possible by construction).

### Tests

- `crates/valori-kernel/tests/state_machine.rs` — 4 new tests: id
  registration, `MAX_NAMESPACES` rejection, cascade-delete on drop,
  namespace-0-cannot-be-dropped.
- `crates/valori-kernel/src/event.rs` — 3 new roundtrip/determinism tests.
- `crates/valori-consensus/tests/state_machine.rs` — 8 new tests: sequential
  id assignment, idempotent-by-name creation, drop removes from registry,
  drop-unknown rejected, snapshot round-trip preserves the registry
  (including `next_id` continuity), two independently-constructed state
  machines converge on identical ids from identical entries. (This file was
  also unblocked — see Findings.)
- `crates/valori-consensus/tests/fault_tolerance.rs` — 1 new test: a 3-node
  real-Raft cluster, `AutoCreateNamespace` submitted on the leader, every
  node (including followers that never received the write directly) agrees.
- `crates/valori-node/tests/cluster_namespaces.rs` (new file) — 7 tests: HTTP
  create replicates cross-node (in-process read on a node that never got the
  HTTP request), idempotent retry, drop replicates cross-node, drop-default
  rejected, list reflects committed creates, follower redirects a namespace
  create (the bug, made observable), explicit `shard_count=1` regression
  check.

## Findings

1. **`crates/valori-consensus/tests/state_machine.rs` was already broken on
   `main`**, independent of this work (confirmed via `git stash` before
   starting, same as an S1 finding) — `ValoriStateMachine::new()` missing
   its `dim` argument. Fixed as a one-line, in-scope correction since this
   phase needed to add tests to the same file and verify them.
2. **A second pre-existing, unrelated test bug surfaced once that file
   compiled**: `apply_rejects_unknown_schema_version` asserted
   `sm.apply(...).is_err()`, contradicting the code's own documented H-4
   design (schema-version rejection is application-level, via
   `ClientResponse.rejected`, specifically so a `StorageError` never halts
   the node). Fixed to assert `replies[0].rejected.is_some()` instead,
   matching the documented intent.
3. **`client_write()` only guarantees the LEADER applied an entry** — it does
   not wait for followers to replicate and apply it too. The first draft of
   the cross-node HTTP tests asserted follower state immediately after the
   HTTP response returned and flaked/failed because of this. Fixed with a
   small poll-with-timeout helper (`wait_for_namespace`), matching the
   async nature of real replication rather than assuming instantaneous
   convergence — the same pattern other cross-node tests in this codebase
   already use.
4. **A third `KernelEvent` exhaustive-match site existed beyond the two
   found during S1's equivalent search**: `crates/valori-cli/src/commands/timeline.rs`'s
   `describe_event()` (the `valori timeline` CLI's human-readable event
   formatter). Caught by a full `cargo build --workspace` pass, not by
   `-p valori-kernel -p valori-node` alone — reinforcing the S1 finding that
   a type shared across crate boundaries needs a workspace-wide build to
   catch every consumer.
5. **No kernel changes beyond the two new event variants were needed** —
   `Command::CreateNamespace`/`Command::DropNamespace` already existed and
   already had the exact validation/cascade-delete logic required; the
   kernel is unaware sharding or cluster-mode replication exist at all.

## Validation

```
cargo build -p valori-kernel --target wasm32-unknown-unknown   # clean
cargo build --workspace --exclude valoricore-ffi                # clean
                                                                  # (valoricore-ffi fails to LINK on this
                                                                  #  machine independent of this work — a
                                                                  #  pre-existing Python/PyO3 toolchain issue)
cargo test -p valori-kernel        # 62 passed, 0 failed
cargo test -p valori-consensus     # 74 passed, 0 failed, 1 ignored (pre-existing)
cargo test -p valori-node          # 221 passed, 0 failed
cargo test -p valori-cli           # 11 passed, 0 failed
```

**Manual smoke test** — real 3-node cluster (`VALORI_CLUSTER_MEMBERS`, no
`VALORI_SHARD_COUNT` set, i.e. S1's default `shard_count=1`):

```
POST /v1/namespaces {"name":"tenant-acme"} → follower (node 2)
  → 307, Location: http://127.0.0.1:42000 (the leader)   ✓ the bug, fixed

POST /v1/namespaces {"name":"tenant-acme"} → leader (node 1)
  → 200 {"name":"tenant-acme","id":1,"created":true}

GET /v1/namespaces on ALL THREE nodes (including the two that never
received the create request directly):
  → {"collections":[{"name":"default","id":0},{"name":"tenant-acme","id":1}]}
  identical on all three                                  ✓ the fix, proven

DELETE /v1/namespaces/tenant-acme → leader → 204
GET /v1/namespaces on all three → back to just "default" on all three  ✓
```

## Follow-ups

- **S3 — namespace→shard routing**: the actual "front desk." Now that the
  name→id mapping is reliably cluster-wide-consistent, wire a
  `NamespaceId → ShardId` placement layer and route the ~23 HTTP handlers
  that resolve a collection today (the S1 investigation's original finding)
  to the correct shard's Raft group instead of always shard 0.
- **Standalone-mode audit gap**: `Engine::create_collection`/`drop_collection`
  bypass the WAL/audit log (JSON sidecar durability instead). Not a
  correctness bug (single process, no divergence possible), but a tamper-
  evidence/parity gap worth a dedicated follow-up — every other mutation in
  standalone mode is audited, this one isn't.
- **Composite external ids / composable state-hash receipt**: unchanged from
  the S1 phase doc's follow-ups — still blocked on S3's real data sharding.
