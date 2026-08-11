# S9 Capacity Model

Every formula below is derived from the real measurements in `summary.md`.
Each number is labeled **MEASURED**, **INTERPOLATED** (between two real
measured points), or **EXTRAPOLATED** (projected beyond the measured
range using the fitted model) — never presented as more certain than it
is.

## Memory model (BruteForce, the only index type measured across the
full dimension range)

**MEASURED** (4 points, dim 384/768/1024/1536, all at 20K vectors):

```
actual_bytes_per_vector(dim) ≈ 17.23 × dim + 1531
```

```
estimated_vector_memory(dim, vector_count) ≈
    baseline_rss (≈ 3.7 MB, MEASURED, constant across all tested configs)
    + vector_count × actual_bytes_per_vector(dim)
```

Cross-checked against the 100K/384D cells (not part of the fit — an
independent check): predicted ≈ 3.7 + 100,000 × 8,147 / 1e6 ≈ 781 MB;
measured restart RSS was 776.5 MB. **Within 1% — the model holds well
in the range it was validated against (10K-100K vectors, 384-1536
dimensions).**

## Recovery/restart overhead

**MEASURED**: restart RSS consistently exceeds live-insert peak RSS by
roughly 5-11% across every tested cell (e.g. 384D/100K: 750.7 MB insert
peak → 776.5 MB restart; 1536D/20K: 534.0 MB → 598.2 MB). **Capacity
planning must size for the restart peak, not the steady-state insert
peak** — this is a real, repeatable pattern, not noise from a single run.

## Disk model

**MEASURED** (20K vectors, BruteForce, per dimension):

```
disk_mb(dim) ≈ 384→31MB, 768→60MB, 1024→80MB, 1536→119MB
```

Roughly linear with dimension, disk ≈ 0.35-0.4x the in-memory footprint
for this workload (event log + snapshot, no compaction triggered at
this scale). **Not validated at larger vector counts or after multiple
snapshot rotations** — treat as an early-scale estimate only.

## Vector-count safe limits at 384D, 1GB / 0.5 vCPU (the real planned Free tier)

Three independent constraints, evaluated separately per the instruction
not to conflate physical limits with product quotas:

### Hard memory limit (X) — EXTRAPOLATED beyond 100K

Using the memory model above, solving for `estimated_vector_memory =
1024 MB` (100% of the container, i.e. the point of OOM risk):

```
1024 = 3.7 + N × 0.007774   =>   N ≈ 131,300 vectors
```

**EXTRAPOLATED** — the model was validated up to 100K (776.5 MB); this
solves slightly beyond that to the 1GB boundary itself, a modest
extrapolation (from a fitted model with <1% error at the one
cross-check point available, but still not a directly measured OOM
event). Not measured directly: no cell in this phase actually pushed a
1GB/384D container to real OOM.

### Safe operating limit (Y) — INTERPOLATED with an explicit safety margin

Per the instruction not to use 100% of measured capacity: applying a
30% safety margin to the hard limit above (chosen because the measured
restart-vs-insert overhead alone is 5-11%, plus real production
workloads have other transient memory needs — request buffering,
concurrent connections — this benchmark's single-client sequential
insert loop doesn't exercise):

```
Y ≈ 131,300 × 0.70 ≈ 92,000 vectors
```

**INTERPOLATED/derived**, not directly measured at exactly this count.

### Performance limit (Z) — INTERPOLATED between measured points

Search p50 measured at 20K (277 ms) and 50K (671 ms) at 384D — both
real. Linear interpolation for a genuinely interactive latency target
(≤400 ms p50, a reasonable but *product*, not physical, choice):

```
(400 - 277) / ((671-277)/(50000-20000)) + 20000 ≈ 29,300 vectors
```

**This is the actual binding constraint** — far below both X and Y.
BruteForce search is O(N × dim); it degrades continuously with no
"knee," so there is no natural physical ceiling here, only a product
judgment call about what latency is acceptable. At 100K vectors,
measured p50 was 1.3 s regardless of whether the container had 512MB
or 1GB — proving this is NOT a memory problem RAM upgrades would fix.

### Combined

```
Free-tier safe vector quota, 384D, BruteForce ≈ min(X, Y, Z) ≈ Z ≈ 29,300 vectors
```

**This is dramatically lower than the existing, unvalidated
`max_records_per_project = 1,000,000` free-tier quota** — by roughly
34x. That existing number was never benchmark-derived (confirmed in the
audit — "sized off the recommendation in the architecture discussion").

**Important caveat**: this whole calculation assumes BruteForce, because
that's the only index type measured across the full scale range. IVF
and BQ were only feasibility-tested at 10K vectors, where they perform
identically to BruteForce (nothing has differentiated them yet at that
small scale) — whether they meaningfully raise the *performance* limit
(Z) at 100K+ scale is genuinely unknown and is the single highest-value
S10 follow-up. If IVF or BQ preserve sub-linear search latency at scale
(as their algorithmic design suggests they should), the real product
quota could be substantially higher than 29,300 — but that must be
measured, not assumed, before being used to set a real quota.

## What this model does NOT cover

- Dimensions above 1536 (3072 mentioned in the plan schema as an
  enterprise `max_dimension` ceiling — not measured).
- Vector counts above 100K for BruteForce, or above 10K for any other
  index type.
- Multi-collection or multi-project memory-sharing effects at scale
  (the collections-scaling and multi-project scripts exist but weren't
  run at meaningfully large scale this session).
- CPU as an independently-varied axis (all cells used 0.5 vCPU; whether
  1 or 2 vCPU meaningfully improves the performance limit Z is
  unmeasured, though the near-50% CPU utilization observed at 0.5 vCPU
  during inserts strongly suggests CPU headroom would help).
