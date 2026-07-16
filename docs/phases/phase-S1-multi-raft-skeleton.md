# Phase S1 — Multi-Raft consensus skeleton

Branch: `Node-scaleup`.

## Goal

Prove that Valori's cluster can run **multiple independent Raft groups
("shards") in one process, sharing one gRPC listener**, each with its own
persistent log and state machine — the technical core a future
namespace-sharded, horizontally-scalable cluster needs. Deliberately scoped
down from the full sharding initiative: no namespace→shard routing, no
placement layer, no HTTP-layer changes. `VALORI_SHARD_COUNT` defaults to 1
and must be byte-identical to the pre-S1 single-Raft-group behavior.

## Delivered

### `crates/valori-consensus`

- **`src/types.rs`** — new `ShardId(pub u32)` newtype (`Copy`, `Hash`,
  `Ord`, `Serialize`/`Deserialize`), exported from `lib.rs`.
- **`proto/raft.proto`** — `RaftRequest`/`RaftReply` gained a `shard_id: u32`
  field (field 2; `payload` stays field 1). Documented as a breaking
  wire-format change: a node without this field decodes `shard_id` as 0
  (harmless at `shard_count=1`), but shard-aware and pre-S1 binaries cannot
  be mixed once `shard_count > 1` is in use. `build.rs` needed no changes —
  `tonic_build` regenerates the field automatically.
- **`src/network.rs`**:
  - `ValoriNetworkFactory`/`ValoriNetwork` gained a `shard: ShardId` field,
    set once at construction (`ValoriNetworkFactory::new(shard)` /
    `::with_tls(shard, tls)`), stamped on every outgoing `RaftRequest`.
  - `RaftRpcService` changed from wrapping one `Raft` to
    `HashMap<ShardId, Raft>`; each of the three RPC handlers reads
    `shard_id` off the request and dispatches to the right group, returning
    `tonic::Status::not_found` (+ `tracing::error!`) for an unknown shard —
    a real misconfiguration signal under symmetric placement.
  - `serve_raft`/`serve_raft_tls` now take `HashMap<ShardId, Raft>`. Added
    `serve_raft_single`/`serve_raft_tls_single` convenience wrappers
    (`ShardId(0)` only) so single-shard callers don't need to build a map.
- **`tests/multi_shard.rs`** (new) — the only test exercising the new
  `RaftRpcService` routing table: 2 shards × 3 nodes, **one shared gRPC
  listener per node multiplexing both shards** (not two isolated listeners —
  that topology would never catch a `shard_id` routing bug). Four tests:
  independent leader election per shard, write isolation (a shard-0 write
  never appears in shard 1's state), independent BLAKE3 convergence with a
  cross-shard hash-inequality check, and shard-0-leader-failure not
  affecting shard-1 liveness.
- **`tests/grpc_cluster.rs`, `mtls.rs`, `fault_tolerance.rs`,
  `snapshot_transfer.rs`** — updated to the new `serve_raft_single`/
  `ValoriNetworkFactory::new(ShardId(0))` call shape. No behavioral changes.

### `crates/valori-node`

- **`src/cluster.rs`**:
  - `ClusterConfig` gained `shard_count: u32`, parsed from
    `VALORI_SHARD_COUNT` in `from_env()` (default 1; present-but-unparseable
    is a hard boot error — `ClusterConfigError::BadShardCount`, matching the
    file's existing "typo = stop the process" philosophy).
  - New `ShardHandle { raft, state_machine, startup_committed_index }`.
    `ClusterHandle` kept every existing field byte-for-byte (still meaning
    "shard 0") and gained `shards: BTreeMap<ShardId, ShardHandle>`. This
    was the key design choice enabling the "no HTTP-layer changes" goal:
    `main.rs`, `cluster_server.rs`, and `cluster_api.rs` needed **zero
    changes** — verified by a clean `cargo build -p valori-node` with no
    edits to any of those three files.
  - `bootstrap_cluster` now loops `0..cfg.shard_count`, building one
    `(network, log_store, state_machine, raft)` tuple per shard. Only shard
    0 gets the caller's `audit: Box<dyn AuditSink>`; shards ≥ 1 get an
    internal `NullAuditSink` — a deliberate scope trim (see Findings) rather
    than the factory-closure design originally sketched in planning.
    Redb paths are suffixed via a new `shard_path()` helper
    (`{stem}-shard{N}{ext}`, exactly `base` unchanged when `shard_count==1`).
    One shared `serve_raft`/`serve_raft_tls` call binds every shard's `Raft`
    behind the process's single gRPC listener. `cfg.init` initializes every
    shard from the same member list (symmetric placement).
  - `spawn_raft_metrics_watcher`/`spawn_state_hash_watcher`/
    `check_state_hash_agreement` gained a `shard: ShardId` parameter and
    (see Findings) a `label_shards: bool` gate. Shards ≥ 1 get a no-op
    state-hash watcher with a one-time warning — there's no HTTP
    `/v1/proof/state` surface for them in this slice.
- **`tests/cluster_read_index.rs`, `cluster_boot.rs`, `cluster_data_plane.rs`,
  `cluster_api.rs`** — added `shard_count: 1,` to every `ClusterConfig`
  struct literal (8 sites total, more than the 4 files originally estimated
  during planning — `cluster_boot.rs` alone has 4 literals).

### `crates/valori-cli`

- **`src/commands/wizard.rs`** (the `valori setup` interactive wizard) and
  **`tests/cluster_cli.rs`** each construct `ClusterConfig` directly and
  needed the same `shard_count: 1,` addition — caught by a full
  `cargo build --workspace` pass, not by `-p valori-node`/`-p
  valori-consensus` alone. No behavioral changes; the wizard always starts
  at `shard_count=1`, symmetric with today.

## Findings

1. **The audit-sink design in the original plan draft was over-engineered.**
   A factory-closure signature (`impl Fn(ShardId) -> Box<dyn AuditSink>`)
   would have forced `main.rs` to juggle capturing shard 0's
   `EventLogWriter` handle out of a closure for the `/v1/proof/event-log`
   endpoint. Since S1 wires no HTTP traffic into any shard beyond 0, there
   is nothing for a shard ≥ 1 audit sink to ever record — so
   `bootstrap_cluster`'s public signature stayed **unchanged**
   (`audit: Box<dyn AuditSink>`), with shards ≥ 1 silently getting
   `NullAuditSink` internally. This also meant `main.rs` needed zero edits,
   which the original plan hadn't anticipated as cleanly.

2. **Prometheus metric labels are a real backward-compatibility surface, not
   just a "harmless" cosmetic note.** The plan initially treated
   unconditionally adding a `shard="0"` label at `shard_count=1` as a minor,
   acceptable trade-off. Running the actual test suite proved otherwise:
   `cluster_boot.rs::raft_metrics_appear_in_prometheus_output` does exact
   substring matching (`"valori_raft_is_leader 1"`) and failed immediately.
   Fixed by threading a `label_shards: bool` (`cfg.shard_count > 1`) through
   the watcher functions so the label is only emitted when more than one
   shard actually exists — `shard_count=1` now produces the *exact*
   pre-S1 metric text, verified both by the test suite and by a manual
   `curl /metrics` smoke test. This is the kind of gap that "looks fine on
   paper" but only surfaces empirically — worth remembering for any future
   phase that adds a label to an existing metric.

3. **`ClusterConfig` struct-literal call sites were undercounted in
   planning.** The plan estimated 4 test files needing the new
   `shard_count` field; the actual count was 4 *files* but 8 *literals*
   (`cluster_boot.rs` alone has 4 separate `ClusterConfig { ... }`
   constructions across its test functions). Mechanical `perl -0pi` fix,
   no surprises beyond the count.

4. **No kernel changes were needed, confirming the architectural premise.**
   `KernelState`/`ValoriStateMachine` are fully shard-agnostic — sharding is
   entirely a consensus/transport-layer concern. `cargo build -p
   valori-kernel --target wasm32-unknown-unknown` stayed green throughout.

5. **`cargo test -p valori-node -p valori-consensus` (the CLAUDE.md-listed
   command) is not sufficient regression coverage for a `ClusterConfig`
   signature change.** `crates/valori-cli`'s setup wizard
   (`commands/wizard.rs`) and its test (`tests/cluster_cli.rs`) also
   construct `ClusterConfig` directly and only surfaced as broken under a
   full `cargo build --workspace` pass. Worth remembering for any future
   change to a type shared across crate boundaries.

## Validation

```
cargo build -p valori-kernel --target wasm32-unknown-unknown   # clean
cargo build --workspace --exclude valoricore-ffi               # clean
                                                                 # (valoricore-ffi fails to LINK on this
                                                                 #  machine independent of this work —
                                                                 #  confirmed via git stash on main)
cargo test -p valori-consensus                                  # 53 passed, 0 failed
cargo test -p valori-node                                       # 214 passed, 0 failed
cargo test -p valori-cli                                        # 11 passed, 0 failed
```

(`crates/valori-consensus/tests/state_machine.rs` fails to compile on `main`
independent of this work — confirmed via `git stash` before starting — and
was left untouched as out of scope.)

`multi_shard.rs` specifically (the new correctness proof, run 3× to rule out
timing flakiness — stable every time):

```
cargo test -p valori-consensus --test multi_shard
running 4 tests
test each_shard_elects_its_own_leader_independently ... ok
test write_to_shard_0_does_not_appear_in_shard_1 ... ok
test shard_0_leader_failure_does_not_affect_shard_1_liveness ... ok
test each_shard_converges_independently_and_hashes_differ ... ok
test result: ok. 4 passed; 0 failed
```

**Manual smoke test** (`VALORI_SHARD_COUNT=3`, single node, real binary):
- 3 independent redb files created: `raft-shard0.redb`, `raft-shard1.redb`,
  `raft-shard2.redb`
- Log shows 3 independent "shard initialized by this node" lines and 3
  independent leader elections
- `curl /v1/cluster/status`, `/v1/proof/state`, `/health`, `POST /records`
  all behave exactly as the standalone-cluster contract describes (shard 0's
  surface)
- `curl /metrics` shows `valori_raft_is_leader{shard="0"} 1`,
  `{shard="1"} 1`, `{shard="2"} 1`

**Regression check** (`VALORI_SHARD_COUNT` unset, i.e. default 1):
- Single `raft.redb` file, no `-shard0` suffix
- `curl /metrics` shows `valori_raft_is_leader 1` — no label at all,
  byte-identical to pre-S1 output

## Follow-ups

- **S2 — placement + namespace routing**: a meta-shard (or simple
  `namespace_id % shard_count` mapping) assigning `NamespaceId → ShardId`,
  and wiring the ~24 HTTP call sites in `server.rs`/`cluster_server.rs`
  that currently resolve a collection name to a `NamespaceId` to also
  resolve a `ShardId` and route the write/read accordingly. This is what
  actually delivers capacity scaling — S1 proves the mechanism but every
  node still runs every shard (symmetric placement).
- **Asymmetric placement**: today every configured `VALORI_CLUSTER_MEMBERS`
  node is a voter in every shard. Real horizontal scaling needs shard A's
  replicas on a different node subset than shard B's — deferred until S2's
  placement layer exists to assign it.
- **Composite external IDs**: `RecordId`/`NodeId`/`EdgeId` are still bare
  `u32` on the wire, unique only within one `KernelState`. Once shards hold
  disjoint data (post-S2), external IDs need a `(shard_id, id)` composite —
  planned for `valori-wire`, not the kernel.
- **Composable state-hash receipt**: `event_log_proof()`/`final_state_hash`
  is still single-shard. A Merkle root over `{shard_id: shard_state_hash}`
  pairs would let the audit/receipt story compose across shards without
  changing the per-shard verification path — not needed until S2 makes
  multiple shards hold real data.
- **Rolling upgrade**: the `shard_id` proto field is a breaking wire change.
  There is no rolling-upgrade path from `shard_count=1` to `shard_count>1`
  across a live cluster today — every node must be upgraded together before
  `VALORI_SHARD_COUNT` is raised anywhere. Worth a `docs/COMPATIBILITY.md`
  note if S2 makes multi-shard a real deployment option.
