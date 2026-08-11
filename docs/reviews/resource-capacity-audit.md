# Resource Capacity Audit (S9)

Read-only audit of the existing memory/storage model, done before any
benchmark numbers were collected, per the S9 instruction not to guess.

## 1. valori-kernel storage/memory model

- **Records**: `RecordPool` (`crates/valori-kernel/src/storage/pool.rs`) is
  a slab: `Vec<Option<Record>>`. Every record — live or soft-deleted —
  occupies a slot until compaction; there is no separate on-disk
  page/mmap layer for records, they live fully in the process's heap.
- **Fully memory-resident**: yes. `KernelState` (records, nodes, edges,
  namespace registry) is entirely in-process memory. Nothing is
  paged/mmap'd from disk during normal operation — disk is written to
  (event log, snapshot) but never read from except at startup recovery.
- **Persisted to disk**: the event log (canonical, append-only,
  BLAKE3-chained) and periodic/shutdown snapshots. The snapshot is a
  full serialization of `KernelState`; the event log is the individual
  `KernelEvent`s. Both are real files under the configured
  `VALORI_EVENT_LOG_PATH`/`VALORI_SNAPSHOT_PATH`.
- **Index rebuild after restart**: confirmed via S8 investigation —
  `Engine::try_recover()`'s event-log path calls `self.rebuild_index()`
  unconditionally after replay (`crates/valori-engine/src/engine.rs`).
  The index itself (HNSW graph, IVF centroids, BQ codes) is **not**
  persisted — it's rebuilt from the record pool every restart. This
  means recovery time scales with (record count × index-build cost for
  the configured index kind), not just event-replay cost.
- **Index memory scaling** (`crates/valori-index/src/`):
  - `brute_force.rs` — no separate index structure; search scans the
    record pool directly. Memory cost is ~0 beyond the records
    themselves.
  - `hnsw.rs` — a real HNSW graph (multi-layer, `M`/`ef_construction`
    configurable via `VALORI_HNSW_*`). Graph edges are additional
    memory proportional to `N × M` (roughly), on top of the vectors
    themselves.
  - `ivf.rs` — centroids (`VALORI_IVF_N_LIST`, auto-scaling to
    `max(16, sqrt(N))` if unset) plus inverted lists mapping centroid →
    member record ids. Memory overhead is small relative to HNSW
    (centroids are few; inverted lists are just id lists).
  - `bq.rs` — binary-quantized codes, one bit per dimension per vector
    instead of the full Q16.16 scalar — the only index type that
    actually *reduces* per-vector memory versus the raw record itself.
  These are architectural facts from reading the source; **actual
  memory deltas per index type are measured, not assumed** — see the
  Index Results section of the phase doc.
- **Base node memory before loading vectors**: real, measured (not
  assumed) via the benchmark harness — see `baseline_rss_mb` in every
  result. Observed ~3.7 MB RSS at container health-check time before any
  insert, across all tested configurations (dim/index don't affect
  baseline since nothing is allocated yet).
- **Collections/namespaces**: a namespace is an intrusive linked list
  threaded through the SAME record pool (`next_in_ns`/`prev_in_ns` on
  each `Record`) plus a small fixed array (`namespace_record_heads`,
  `namespace_node_heads`, both `Vec<u32>` sized to `MAX_NAMESPACES =
  1024` regardless of how many are actually used). Marginal memory cost
  per *empty* collection is small and fixed; cost scales with the
  vectors placed in it, not with the collection itself. Measured in the
  Collections Results section.
- **Multiple projects sharing one worker**: architecturally, "worker" in
  the product sense is a physical/virtual **host** that can run several
  project **containers** (`Host.capacity_slots`, default 10 in
  `valori-ui/backend/apps/api/src/models.rs`) — never multiple projects
  inside the same `valori-node` process. So "sharing a worker" means
  resource contention between sibling containers on the same host
  machine, not in-process state sharing (project isolation at the
  process level is absolute — separate containers, separate ports,
  separate `worker_auth_token`s). What's actually at risk is **host-level
  resource contention** (CPU/disk I/O), tested in the Multi-Project
  Results section.
- **Memory exhaustion behavior**: the container is a real cgroup with a
  hard memory limit (`docker run --memory`); if the process actually
  exceeds it, the kernel OOM-killer terminates the container (`docker
  inspect --format '{{.State.OOMKilled}}'` reports this). `valori-node`
  itself has no internal memory-pressure back-pressure or graceful
  degradation — it will run right up to the cgroup limit and then be
  killed by the OS, not by its own logic. Confirmed both by reading the
  code (no memory-pressure handling anywhere in `valori-engine`) and by
  observing a real 512MB/100K-vector run sit at 99-100% memory
  utilization for several minutes without proactively rejecting new
  writes.
- **Disk-full behavior**: not separately handled in code — a failed
  write to the event log or snapshot file surfaces as a normal I/O
  error (`std::io::Error`) propagated up as an `EngineError`. Not
  independently exercised at the OS/disk-quota level in this phase (see
  Follow-ups — Docker Desktop on macOS doesn't expose a hard per-volume
  byte quota to `docker compose` without extra host setup, same
  limitation noted in the E2E phase's benchmark scope doc).
- **Index-doesn't-fit-in-memory behavior**: no explicit guard. The
  process will attempt to build the configured index regardless of
  size; if it doesn't fit, the OS OOM-kills the container the same way
  an oversized record pool would. There is no "index too large, falling
  back to X" logic anywhere in `valori-index` or `valori-engine`.

## 2. valori-ui provisioning model

- **Worker sizing IS already schema-defined**, but not confirmed
  deployed/enforced end-to-end this phase:
  `supabase/migrations/20260804000000_plan_runtime_limits.sql` sets
  `container_memory_mb`/`container_cpu_millis` per plan:
  - `free`: 1024 MB / 500 millicpu (0.5 vCPU)
  - `pro`: 4096 MB / 2000 millicpu (2 vCPU)
  - `enterprise`: 16384 MB / 8000 millicpu (8 vCPU)
  Comment in the migration: "Sized off the recommendation in the
  architecture discussion" — i.e., these were a starting guess, not
  benchmark-derived. That is exactly what S9 exists to validate or
  correct.
- **Project provisioning**: `backend/apps/api/src/provision/{docker,dokploy}.rs`
  — one project = one dedicated container, deployed onto a host chosen
  by `placement.rs`'s `place()` function.
- **Placement/scheduling**: `placement.rs::place()` — picks
  `replication` distinct hosts in the target region matching the
  project's `worker_class`, preferring hosts with the most free
  capacity slots first (spread load). Fails closed
  (`InsufficientCapacity`) rather than silently placing a project on
  the wrong host class.
- **Multiple projects per worker (host)**: yes, by design —
  `Host.capacity_slots` (default 10 in test fixtures; real value is a
  deployment/ops decision, not hardcoded) caps how many project
  containers one host will accept.
- **Replication**: `replication` is a real parameter on `place()` (1 =
  single node, 3 = Raft cluster), but this phase did **not** benchmark
  replication resource cost — see Follow-ups; the Valori-Kernel-side
  Raft/consensus layer exists and is tested at the protocol level
  (`crates/valori-consensus`), but end-to-end resource-cost-of-replication
  measurement (memory/CPU/network overhead of running N replicas of one
  project) was out of this phase's time budget.

## 3. What this audit does NOT cover (deliberately, per S9's own scope)

Full exhaustive line-by-line review of every crate listed in the S9
prompt (`valori-daemon`, `valori-domain`, `valori-models`) was not done —
those crates are Studio/desktop-facing and do not participate in the
Cloud worker's request or storage path at all (confirmed by their
absence from any `valori-node` dependency chain touched by this
benchmark). Time was spent instead on the crates that actually determine
Cloud worker capacity: `valori-kernel`, `valori-engine`, `valori-index`,
`valori-storage`, `valori-node`.
