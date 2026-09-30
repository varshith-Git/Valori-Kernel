# Benchmark

**Source of truth:** [`benchmarks/README.md`](../../benchmarks/README.md) — already documents methodology,
exact scripts, and exact commands, written in direct response to a specific review (dated, attributed).
This skill teaches using that suite correctly, not re-deriving benchmark methodology.

## When to use
A user asks to measure or compare performance, recall, or determinism-related precision — explicitly, not
as a side effect of a correctness change (a benchmark run doesn't substitute for the correctness tests in
`.claude/rules/testing.md`).

## Real scripts (verified) — use the one that matches the question

| Question | Script | Command |
|---|---|---|
| Insert throughput, search latency, index comparison, snapshot — up to 1M records, no server | `benchmarks/local_perf.py` | `python3 benchmarks/local_perf.py --million` |
| Public retrieval quality (BEIR SciFact) — pure vector vs. Valori HTTP GraphRAG | `benchmarks/live_local_db_comparison.py` | `python benchmarks/live_local_db_comparison.py --dbs faiss valori-http --queries 100 --docs 300 --k 3 --valori-url http://127.0.0.1:3307` |
| Three-arm RAG quality: float32 vs. Q16.16 vs. Q16.16+graph | `benchmarks/run_benchmark.py` | `python3 benchmarks/run_benchmark.py` |
| Identical BLAKE3 state hash across CPU architectures | `benchmarks/multi_arch_hash.py` | `python3 benchmarks/multi_arch_hash.py --url http://localhost:3000` |
| Recall@10 of Q16.16 vs. float32 ground truth at real embedding dims | `benchmarks/q16_precision.py` | `python3 benchmarks/q16_precision.py --dim 384 [--st] [--openai]` |
| Quick capped stress test (50k vectors) | `make stress` | (wraps `scripts/stress_test_million.py --max-n 50000 --skip-charts`) |

Several of these need the node running first (`local_perf.py`/`run_benchmark.py`/`multi_arch_hash.py`
against `:3000` by default) — check the script's own `--help` / README row before assuming.

## Distinguish these — don't conflate them

- **Correctness** — a benchmark is not a correctness test; a faster-but-wrong result is still wrong. Use
  `.claude/rules/testing.md`'s matrix for correctness.
- **Determinism** — `multi_arch_hash.py` specifically; a throughput benchmark says nothing about whether
  the same run reproduces the same hash on a different architecture.
- **Throughput / latency** — `local_perf.py`.
- **Precision/recall** — `q16_precision.py`, `run_benchmark.py`, `live_local_db_comparison.py`.
- **Index/snapshot size** — covered inside `local_perf.py`'s output, not a separate script today.

## Reproducibility — what to capture (per `benchmarks/README.md`'s own precedent, e.g. `LIVE_LOCAL_RESULTS.json`)
- CPU/architecture, OS.
- `rustc --version` / `cargo --version` if the binary was rebuilt for this run.
- Valori commit (`git rev-parse HEAD`).
- Dataset identity and size (e.g. "300 evidence-preserving documents, 100 claims" — `run_benchmark.py`'s
  own results record this level of detail; match it).
- Vector dimension, index type, record count, any non-default parameters.
- Warmup and repetition count if the script doesn't already fix these internally — check the script before
  assuming it warms up.

## Never do
- **Never claim an improvement from one noisy run.** Repeat, or note explicitly that this is a single run
  and the claim is provisional.
- **Never silently change the dataset, dimension, or config between a "before" and "after" run** — if
  something had to change (e.g. a new index type needs different params), say so explicitly rather than
  letting an apples-to-oranges comparison look like a clean regression/improvement.
- Never present a hosted/API-only competitor result pulled from elsewhere as directly comparable to a
  local run — `live_local_db_comparison.py`'s own design deliberately excludes those from local runs for
  this reason.
