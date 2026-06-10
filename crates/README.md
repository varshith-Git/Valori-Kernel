# valori-cli

A command-line tool for inspecting, debugging, and verifying Valori AI memory databases directly from disk — no running server required.

See **[crates/cli/README.md](cli/README.md)** for the full documentation.

---

## Quick reference

```bash
# Install
cargo install --path crates/cli

# Check database health
valori inspect --dir ./my_valori_db

# Verify a snapshot file
valori verify snapshot.val

# Print the full event history
valori timeline events.log

# Replay to event #200 and run a search
valori replay-query --snapshot snapshot.val --log events.log --at 200 \
  --query '[0.1, -0.5, 0.8]' --top-k 5

# Compare state between event #150 and #200
valori diff --snapshot snapshot.val --log events.log --from 150 --to 200 \
  --query '[0.1, -0.5, 0.8]'
```

---

## Architecture

The crate contains two main pieces:

**`valori` binary** — the five-command CLI described above.

**Benchmark binaries** — standalone programs for measuring kernel performance on SIFT1M data:

| Binary | Measures |
|---|---|
| `bench_ingest` | End-to-end ingestion throughput (events/second) |
| `bench_1m` | Memory bandwidth breakdown: I/O, parsing, fixed-point math |
| `bench_filter` | Tag-filtered search correctness |
| `bench_persistence` | Snapshot save and load round-trip latency |
| `bench_recall` | Recall@1 and Recall@10 vs brute-force ground truth |

All benchmarks require SIFT1M vectors at `data/sift/sift/sift_base.fvecs`.
