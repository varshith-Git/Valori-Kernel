# Phase 2.10c — Raft Metrics on Prometheus

**Status:** done · on `multinode`
**Roadmap:** Phase 2 (cluster mode), sub-phase 10 of 10, part c of d

## Goal

Make cluster health observable on the `/metrics` endpoint the node already
serves: leadership, term, replication/apply lag, and compaction progress —
the four signals an operator pages on.

## Delivered

**`spawn_raft_metrics_watcher`** (cluster.rs, started by
`bootstrap_cluster`): a task mirroring openraft's metrics watch-channel
into Prometheus gauges. Event-driven (`changed().await`), not polling; the
watch stream closes when the Raft core shuts down and the task exits with
it — no leaked task after `Raft::shutdown`.

| Gauge | Meaning |
|---|---|
| `valori_raft_term` | current term |
| `valori_raft_current_leader` | leader node id this node sees (0 = none) |
| `valori_raft_is_leader` | 1/0 — alert when a cluster has ≠1 |
| `valori_raft_last_log_index` | highest appended index |
| `valori_raft_last_applied_index` | highest applied index — **gap to last_log_index is apply lag** |
| `valori_raft_snapshot_index` | last snapshot coverage |
| `valori_raft_purged_index` | compaction floor |

All described in `telemetry.rs` alongside the existing kernel gauges; they
render on the same `/metrics` endpoint with no new infrastructure.

## Validation

- `cluster_boot.rs::raft_metrics_appear_in_prometheus_output` — boots a
  real node, commits writes, then polls the rendered Prometheus text:
  `valori_raft_is_leader 1`, `valori_raft_current_leader 1`, term present,
  and the applied-index gauge actually covering the writes (≥ 3). 3× stable.
- Full workspace: **252 passing, 0 failures.**

## Findings

- None. The `metrics` crate facade (0.21) the node already used composes
  directly; the only design choice was event-driven mirroring over polling,
  which falls out of openraft exposing a `watch::Receiver`.

## Follow-ups

- 2.10d (the last Phase 2 item): partition harness — a switchable
  transport behind `RaftNetwork` so both-sides-alive partitions and
  asymmetric link failures become testable; plus the gRPC decode cap
  noted in 2.4. This is an architectural change to the network layer.
- Grafana dashboard JSON (alert rules: `sum(valori_raft_is_leader) != 1`,
  apply lag > threshold) — deployment-docs scope, Phase 3.
