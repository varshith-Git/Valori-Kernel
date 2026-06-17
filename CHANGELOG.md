# Changelog

All notable changes to Valori are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **`valori_raft_state_hash_match` Prometheus gauge** — a background task on
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
