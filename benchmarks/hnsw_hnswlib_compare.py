"""
hnswlib comparison against Valori HNSW.

Loads the exact same vectors exported by the Rust test:
  cargo test -p valori-node --lib --release hnsw_export_bench_vectors -- --nocapture --ignored

Then builds an hnswlib index with identical parameters and measures recall@10
at the same ef_search values, so the only variable is the HNSW implementation.

Usage:
  pip install hnswlib numpy
  python3 benchmarks/hnsw_hnswlib_compare.py
"""

import struct, sys, time
import numpy as np

BIN_PATH = "/tmp/valori_hnsw_bench.bin"

# ── load vectors exported by Rust ────────────────────────────────────────────
try:
    with open(BIN_PATH, "rb") as fh:
        dim, n_corpus, n_queries = struct.unpack("<III", fh.read(12))
        total = (n_corpus + n_queries) * dim
        flat  = np.frombuffer(fh.read(total * 4), dtype="<f4").copy()
except FileNotFoundError:
    sys.exit(
        f"Vector file not found: {BIN_PATH}\n"
        "Run first:\n"
        "  cargo test -p valori-node --lib --release "
        "hnsw_export_bench_vectors -- --nocapture --ignored"
    )

corpus  = flat[:n_corpus  * dim].reshape(n_corpus,  dim).astype(np.float32)
queries = flat[n_corpus   * dim:].reshape(n_queries, dim).astype(np.float32)
print(f"Loaded {n_corpus} corpus + {n_queries} query vectors  dim={dim}")

# ── brute-force ground truth (L2 squared, same as Valori) ───────────────────
def brute_top_k(corpus, query, k):
    diff = corpus - query
    dists = (diff * diff).sum(axis=1)
    return set(np.argpartition(dists, k)[:k])

truth = [brute_top_k(corpus, q, 10) for q in queries]

# ── hnswlib index: M=16, ef_construction=100 — identical to Valori default ──
try:
    import hnswlib
except ImportError:
    sys.exit("hnswlib not installed. Run: pip install hnswlib")

M               = 16
EF_CONSTRUCTION = 100
SPACE           = "l2"

index = hnswlib.Index(space=SPACE, dim=dim)
index.init_index(max_elements=n_corpus, ef_construction=EF_CONSTRUCTION, M=M)
index.add_items(corpus, num_threads=1)

def mean_recall(index, queries, truth, k, ef):
    index.set_ef(ef)
    total = 0.0
    for q, t in zip(queries, truth):
        labels, _ = index.knn_query(q.reshape(1, -1), k=k)
        total += len(set(labels[0]) & t) / k
    return total / len(queries)

# ── ef_search sweep ──────────────────────────────────────────────────────────
print(f"\n=== hnswlib ef_search sweep  M={M}  ef_construction={EF_CONSTRUCTION}  N={n_corpus}  k=10 ===")
print(f"{'ef_search':>10}  {'Recall@10':>10}")
print("-" * 24)
for ef in [10, 20, 50, 100, 200, 400, 800]:
    r = mean_recall(index, queries, truth, k=10, ef=ef)
    print(f"{ef:>10}  {r*100:>9.1f}%")

# ── latency at operating points ──────────────────────────────────────────────
TRIALS = 200
print(f"\n=== hnswlib latency  M={M}  ef_construction={EF_CONSTRUCTION}  N={n_corpus}  k=10 ===")
print(f"{'ef_search':>10}  {'p50 µs':>10}  {'p95 µs':>10}  {'Recall@10':>10}")
for ef in [100, 200, 400]:
    index.set_ef(ef)
    times = []
    for q in list(queries) * (TRIALS // len(queries) + 1):
        t0 = time.perf_counter()
        index.knn_query(q.reshape(1, -1), k=10)
        times.append((time.perf_counter() - t0) * 1e6)
    times = sorted(times[:TRIALS])
    p50 = times[TRIALS // 2]
    p95 = times[int(TRIALS * 0.95)]
    r   = mean_recall(index, queries, truth, k=10, ef=ef)
    print(f"{ef:>10}  {p50:>10.1f}  {p95:>10.1f}  {r*100:>9.1f}%")
