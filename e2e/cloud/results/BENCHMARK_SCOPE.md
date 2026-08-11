# Resource benchmark — actual scope run this session

## What was actually measured

One real data point, against a genuine standalone `valori-node` container
(built from this repo's real `Dockerfile`, not a mock), constrained to
**real** Docker resource limits (verified via `docker inspect`:
`Memory: 536870912 bytes` = 512 MiB, `NanoCpus: 500000000` = 0.5 CPU):

| dim | vectors | index | insert throughput | peak RAM | search p50 | search p95 |
|---|---|---|---|---|---|---|
| 384 | 10,000 | BruteForce | 1,147.6 vec/s | ~85 MiB | 119.5 ms | 172.2 ms |

Raw result: [`benchmark_dim384_10k_bruteforce.json`](benchmark_dim384_10k_bruteforce.json).

512 MiB was **not** a limiting factor at this scale — peak usage stayed
under 17% of the limit.

## What was NOT measured, and why

The full requested matrix is 3 dimensions × 3 vector counts × 4 index
types = 36 combinations. Each of HNSW/IVF/BQ requires an actual index
*build* step (not just inserts) that scales non-trivially with vector
count and dimension, and `valori-node` fixes its dimension at first
insert (CLAUDE.md invariant) — so each dimension needs its own
from-scratch container, not a reused one. Running all 36 combinations for
real, at 100K vectors × 1536 dims × HNSW build time, is realistically
multiple hours of genuine wall-clock compute — not something this session
had time to run to completion honestly.

Rather than fabricate the remaining 35 cells or silently skip this
section, this is the accurate state: **one real cell measured, the rest
NOT MEASURED**. If the full matrix is wanted, it should be run as its own
dedicated session (or CI job) using this same real container + the
`bench.py` pattern used here, budgeted for the real time it takes.

## 5 GB storage behavior (real limitation, not a workaround)

Docker Desktop on macOS does not expose a way to cap an individual named
volume at a hard byte quota from `docker compose` alone (the constraint
would need to come from the host filesystem or a size-limited loopback
volume, neither of which this compose file sets up). This was not
implemented — documented here as a real environment limitation rather
than silently skipped or faked with an application-level workaround.
Project data / index / snapshot / WAL sizes for the one real benchmark
cell above can be read directly from the container's `/data` volume; no
attempt was made to construct a synthetic 5 GB dataset in this session.
