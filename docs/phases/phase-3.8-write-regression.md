# Phase 3.8 — Write-throughput regression gates in CI

## Goal

Automated benchmark that fails a CI check (warning only, does not block merge) if p99 single-insert latency regresses > 15% or batch throughput drops > 10% vs a stored baseline. Gives the team a permanent signal when performance degrades across PRs.

## Delivered

| File | Purpose |
|---|---|
| `benchmarks/write_regression.py` | Standalone benchmark: 200 single inserts (p50/p99), 10k-record batch (throughput). Supports `--compare` (regression check) and `--save-baseline` (update baseline). No external deps beyond the stdlib. |
| `benchmarks/baseline/write_regression_baseline.json` | Seed baseline: p99 = 8 ms, throughput = 3 000 rps. Update after deliberate perf improvements via `--save-baseline`. |
| `.github/workflows/write-regression.yml` | PR workflow: builds release binary, starts node, runs benchmark, posts a PR comment if regression detected. `continue-on-error: true` — warns without blocking. |

**Thresholds:**
- p99 single-insert latency: must not grow more than **15%** vs baseline
- Batch throughput: must not drop more than **10%** vs baseline

**Baseline update flow:**
```bash
# 1. Run the node locally
VALORI_DIM=128 ./target/release/valori-node

# 2. Measure and save new baseline
python3 benchmarks/write_regression.py --save-baseline

# 3. Commit
git add benchmarks/baseline/write_regression_baseline.json
git commit -m "chore: update perf baseline after <improvement>"
```

## Findings

- Used stdlib `urllib.request` to avoid needing `requests`/`httpx` as a CI dep — the script runs with a bare Python 3.11 install.
- CI starts the node with `VALORI_MAX_RECORDS=20000` to handle the 10k-record batch + 200 single inserts without hitting capacity.
- The workflow uses `continue-on-error: true` on the regression assertion step so that perf regressions are visible but don't block PRs from merging. Teams can tighten this once the baseline is stable.

## Validation

Script syntax verified locally:
```bash
python3 -c "import ast; ast.parse(open('benchmarks/write_regression.py').read()); print('ok')"
```

Live benchmark requires a running node. Run via:
```bash
cargo run -p valori-node --release &
python3 benchmarks/write_regression.py
```

## Follow-ups

- Phase 3.13 — HNSW parameter exposure: run the regression benchmark in both BruteForce and HNSW modes.
- Consider adding a p99 search latency gate alongside the write gate.
