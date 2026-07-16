"""
FAISS IVF comparison against Valori IVF.

Loads the exact same vectors exported by the Rust test:
  cargo test -p valori-node --lib --release ivf_export_bench_vectors -- --nocapture --ignored

Then builds a FAISS IVFFlat index with the same n_list values and sweeps n_probe,
so the only variable is the k-means + search implementation.

Usage:
  pip install faiss-cpu numpy
  python3 benchmarks/ivf_faiss_compare.py
"""

import struct, sys, time
import numpy as np

BIN_PATH = "/tmp/valori_ivf_bench.bin"

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
        "ivf_export_bench_vectors -- --nocapture --ignored"
    )

corpus  = flat[:n_corpus  * dim].reshape(n_corpus,  dim).astype(np.float32)
queries = flat[n_corpus   * dim:].reshape(n_queries, dim).astype(np.float32)
print(f"Loaded {n_corpus} corpus + {n_queries} query vectors  dim={dim}")

try:
    import faiss
except ImportError:
    sys.exit("faiss-cpu not installed. Run: pip install faiss-cpu")

# ── brute-force ground truth ──────────────────────────────────────────────────
def brute_top_k(corpus, query, k):
    diff = corpus - query
    dists = (diff * diff).sum(axis=1)
    return set(np.argpartition(dists, k)[:k])

truth = [brute_top_k(corpus, q, 10) for q in queries]

def mean_recall(I, truth, k):
    total = 0.0
    for row, t in zip(I, truth):
        total += len(set(row[:k]) & t) / k
    return total / len(truth)

# ── sweep n_list ──────────────────────────────────────────────────────────────
print(f"\n=== FAISS IVFFlat n_list sweep  n_probe=n_list/4  k=10 ===")
print(f"{'n_list':>8}  {'n_probe':>8}  {'Recall@10':>10}  {'p50 µs':>10}  {'build ms':>10}")

TRIALS = 200
K = 10

for n_list in [64, 128, 256, 512]:
    n_probe = max(1, n_list // 4)
    quantizer = faiss.IndexFlatL2(dim)
    index = faiss.IndexIVFFlat(quantizer, dim, n_list, faiss.METRIC_L2)
    t0 = time.perf_counter()
    index.train(corpus)
    index.add(corpus)
    build_ms = (time.perf_counter() - t0) * 1000

    index.nprobe = n_probe
    _, I = index.search(queries, K)
    recall = mean_recall(I, truth, K) * 100

    # latency
    times = []
    for _ in range(TRIALS):
        q = queries[np.random.randint(len(queries))].reshape(1, -1)
        t0 = time.perf_counter()
        index.search(q, K)
        times.append((time.perf_counter() - t0) * 1e6)
    times.sort()
    print(f"{n_list:>8}  {n_probe:>8}  {recall:>9.1f}%  {times[TRIALS//2]:>10.1f}  {build_ms:>10.1f}")

# ── n_probe sweep at fixed n_list=158 (Valori auto-scale value) ───────────────
print(f"\n=== FAISS IVFFlat n_probe sweep  n_list=158  k=10 ===")
print(f"{'n_probe':>10}  {'Recall@10':>10}  {'p50 µs':>10}")

n_list = 158
quantizer = faiss.IndexFlatL2(dim)
index158 = faiss.IndexIVFFlat(quantizer, dim, n_list, faiss.METRIC_L2)
index158.train(corpus)
index158.add(corpus)

for n_probe in [1, 2, 4, 8, 12, 16, 32, 64, 158]:
    index158.nprobe = n_probe
    _, I = index158.search(queries, K)
    recall = mean_recall(I, truth, K) * 100

    times = []
    for _ in range(TRIALS):
        q = queries[np.random.randint(len(queries))].reshape(1, -1)
        t0 = time.perf_counter()
        index158.search(q, K)
        times.append((time.perf_counter() - t0) * 1e6)
    times.sort()
    print(f"{n_probe:>10}  {recall:>9.1f}%  {times[TRIALS//2]:>10.1f}")
