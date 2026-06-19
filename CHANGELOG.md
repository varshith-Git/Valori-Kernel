# Changelog

All notable changes to Valori are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added (Phase 3.2 — Rolling Upgrades)
- **`schema_version` field on `ClientRequest`** (`valori-consensus`) — the
  leader stamps `CURRENT_SCHEMA_VERSION` (currently `0`) on every proposal. Old
  nodes decode the field as `0` via `#[serde(default)]`.
- **`CURRENT_SCHEMA_VERSION: u8 = 0`** constant (`valori-consensus::types`) —
  single source of truth for the cluster wire version. Bump when a new
  `KernelEvent` variant or breaking field change requires newer followers.
- **Schema version gate in `ValoriStateMachine::apply()`** — followers reject
  entries with `schema_version > CURRENT_SCHEMA_VERSION` with `StorageError`
  (halts replication on that node; cluster continues through remaining quorum).
  State and audit log are untouched on rejection.
- **`valori cluster upgrade --url … --target-version …`** CLI command — interactive
  guided rolling upgrade: discovers topology, upgrades non-leaders first then
  leader, polls `/health` after each restart, waits for re-election before
  declaring the leader step complete.
- **`docs/COMPATIBILITY.md`** — schema version history, rolling-window rules,
  coexistence matrix, and the procedure for bumping `CURRENT_SCHEMA_VERSION`.

### Fixed (Phase 3.2)
- `corrupted_snapshot_payload_is_refused_and_state_kept` snapshot corruption
  test was flipping byte `bytes.len() / 2` which, for V6 snapshots (8318 bytes),
  lands in the namespace sentinel region not covered by `hash_state_blake3`.
  Fixed to corrupt `bytes.last_mut()` (last byte of the `state_hash` tail),
  which always triggers the hash mismatch check regardless of format version.

---

## [0.2.1] — 2026-06-19

### Added
- **Multi-tenant collections** — up to 1 024 named namespaces per node.
  `POST /v1/namespaces`, `GET /v1/namespaces`, `DELETE /v1/namespaces/:name`.
  All data endpoints accept an optional `"collection"` field. Records are
  isolated at the kernel level via intrusive per-namespace linked lists enforced
  at three independent points (event-commit, WAL replay, `build_index`).
- **`AutoCreateNode` / `AutoCreateEdge` kernel events** — graph mutations with
  IDs assigned at apply time for deterministic cluster-mode graph operations.
- **Persistent Raft state machine** — when `VALORI_RAFT_LOG_PATH` is set, the
  state machine shares the redb file and persists `last_applied`, membership,
  and the latest snapshot, preventing duplicate audit-log writes on restart.
- **Replay suppression** — `replay_until` suppresses already-written audit
  entries when openraft replays committed log entries after a restart.
- **`GET /v1/cluster/role`** — current node's Raft role for load-balancer routing.
- **`state_hash_match` Prometheus gauge** — cluster-wide hash-convergence metric.
- **Snapshot V6 format** — per-record `namespace_id` + linked-list pointers,
  2 × 1 024 × 4 = 8 KB namespace heads arrays, and a backward-compatible NSRG
  section (namespace registry as JSON, detected by `"NSRG"` magic tag).
- **Python SDK collection API** — `create_collection`, `list_collections`,
  `drop_collection` on both `SyncRemoteClient` and `AsyncRemoteClient`;
  `collection` parameter on all data methods; `consistency` parameter on search.
- **Threat model** (`docs/THREAT_MODEL.md`).
- **Capacity planning** (`docs/CAPACITY.md`).
- **DR & multi-region runbook** (`docs/DR.md`).
- **Multi-arch hash benchmark** (`benchmarks/multi_arch_hash.py`).
- **Q16.16 precision benchmark** (`benchmarks/q16_precision.py`).
- **Helm snapshot CronJob** (`deploy/helm/valori/templates/snapshot-cronjob.yaml`).
- **CI test-count workflow** (`.github/workflows/test-count.yml`).

### Fixed
- `LeaderClient::get_proof()` wire-format mismatch — server returns
  `{"final_state_hash":"<hex>"}` but client expected `[u8; 32]`. Added
  `LeaderProof { final_state_hash: String }` and updated hex comparison in replication.
- Snapshot buffer too small for V6 in `format.rs` and `snapshot_roundtrip.rs`
  (4 KB → 16 KB).
- `spawn_state_hash_watcher` held `Arc<Database>` indefinitely, blocking redb
  file re-open on restart. Now returns `JoinHandle`, stored in `ClusterHandle`,
  aborted and awaited before shutdown.
- arXiv paper title corrected from *"Deterministic Memory: A Substrate for
  Verifiable AI Agents"* to *"Valori: A Deterministic Memory Substrate for
  AI Systems"* in README and BibTeX.
- Hardcoded test count badge (271) replaced with CI-driven workflow badge.
- Python SDK version badge corrected from v0.1.11 to v0.2.1.
- Apply-vs-audit ordering invariant now explicitly documented with crash-window
  analysis in `valori-consensus/README.md`.
- Comparison table "No" cells now cite competitor documentation.

### `valori_raft_state_hash_match` Prometheus gauge — a background task on
  each cluster node periodically calls `/v1/proof/state` on every peer and
  publishes `1` when all reachable nodes agree on the BLAKE3 state hash, `0`
  when any peer diverges. Mismatches are also logged at `ERROR` level and
  counted by `valori_raft_divergence_detections_total`. Configurable via
  `VALORI_STATE_HASH_CHECK_SECS` (default 30 s; `0` disables).
- **`GET /v1/cluster/role`** endpoint — returns `{"role":"leader"|"follower",
  "node_id":N,"current_leader":N}` on any node. Designed for load-balancer
  health-check routing: steer writes at the pod that answers `"leader"` to
  avoid 307 redirect round trips on every write.
- **Proptest event-sequence fuzz** (`crates/valori-consensus/tests/proptest_event_fuzz.rs`)
  — 32 randomly generated insert/soft-delete/delete sequences applied through
  a 3-node in-process cluster, asserting all nodes converge to the same BLAKE3
  state hash after each sequence. Shrinks failing cases automatically.
- **Helm chart** (`deploy/helm/valori/`) — production StatefulSet with
  PersistentVolumeClaims for `events.log` and `raft.redb`, headless service
  for stable pod DNS, client service, and configurable liveness/readiness
  probes pointing at `/v1/cluster/health` and `/health`. Topology spread
  anti-affinity keeps pods on separate availability zones by default.

- **Automatic `events.log` rotation** on both write paths — the standalone
  `EventCommitter` and the cluster `EventLogAuditSink` seal the live segment to
  `events.log.NNNNNN` once it passes `VALORI_EVENT_LOG_ROTATION_BYTES` (default
  256 MiB; `0` disables), opening a fresh segment that splices from the sealed
  one's chain head.
- **Multi-segment recovery** — recovery now discovers and replays every local
  segment (sealed archives + live file) in sequence order and verifies each
  splice point.

- **Linearizable reads via the read-index protocol** (now the default read
  consistency). The leader serves through openraft's `ensure_linearizable()`;
  a follower fetches the leader's read index from the new
  `GET /v1/cluster/read-index` endpoint, then waits for its own apply to catch
  up before scanning local state. Clients can opt into a faster,
  eventually-consistent read with `consistency: "local"` (Python SDK:
  `search(..., consistency="local")`).

### Fixed
- Rotated logs previously recovered **only the live segment**, silently dropping
  all pre-rotation history; recovery is now multi-segment and lossless.
- Archive segments are named by monotonic segment sequence instead of a
  wall-clock timestamp, so two rotations within the same second no longer
  collide and clobber an earlier archive.

## [0.2.0] — 2026-06-13

The multi-node release. Valori graduates from a single standalone node to a
Raft-replicated cluster with verifiable, crash-symmetric state on every replica.

### Added
- **Raft consensus layer** (`valori-consensus`) over openraft 0.9: replicated
  log store (in-memory + persistent `redb`), `KernelState` state machine with
  the audit-log write at apply time, and a tonic/gRPC peer transport.
- **Cluster mode** for `valori-node`: boot-time dispatch on
  `VALORI_CLUSTER_MEMBERS`, leader-redirect (`307 + Location`) for writes,
  local reads on any replica, and a `/v1/cluster/*` management plane
  (status, health, add-node, remove-node).
- **Mutual TLS** on the Raft channel (`VALORI_TLS_*`), enforced at the
  handshake against a shared cluster CA.
- **Persistent Raft log** via embedded `redb` (`VALORI_RAFT_LOG_PATH`) — the
  log and vote survive process restarts.
- **Raft metrics** exported on `/metrics` (term, leader, log/apply lag,
  snapshot/purge indexes).
- **State-machine ID allocation** (`KernelEvent::AutoInsertRecord`): record IDs
  are assigned deterministically at apply time, removing the per-node insert
  mutex and retry loop.
- **Cluster data-plane endpoints**: `/v1/delete`, `/v1/soft-delete`,
  `/v1/vectors/batch_insert`, `/v1/proof/state`.
- **Interactive setup wizard** (`valori setup`): pick architecture and node
  count, start an in-process cluster, and drive inserts/search/membership from
  a live menu. Projects persist to `~/.valori/projects.json`.
- **`valori cluster` CLI**: operate a running cluster (status, health,
  add-node, remove-node) against any node's HTTP API.
- **Docker deployment**: distroless multi-stage `Dockerfile` with a built-in
  `--health-check` TCP probe, and a 3-node `docker-compose.yml`.
- **Partition harness**: in-memory switchable-transport test suite covering
  leader isolation, re-election, partition heal/convergence, and the
  minority-cannot-commit invariant.

### Changed
- Cluster search now uses the kernel's maintained index via `search_l2`
  instead of an ad-hoc record-pool scan.
- Workspace versioning unified at `0.2.0` via `[workspace.package]`; all crates
  inherit version, edition, and license.

### Fixed
- `Dockerfile` now copies all workspace member manifests so workspace
  resolution succeeds; healthcheck no longer references a non-existent flag.

### Repository
- Removed scratch and stale top-level files; relocated manual/e2e/benchmark
  scripts under `scripts/`.
- Tightened `.gitignore` for runtime database directories and caches.

[Unreleased]: https://github.com/valori-db/valori-kernel/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/valori-db/valori-kernel/releases/tag/v0.2.0
