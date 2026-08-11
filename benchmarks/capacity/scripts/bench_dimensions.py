#!/usr/bin/env python3
"""Dimension scaling sweep: fixed 1GB/0.5CPU/BruteForce/20K vectors, only
`dim` varies. 20K chosen (not 50K/100K) to keep per-cell time reasonable
while still giving a true apples-to-apples comparison across dimensions —
documented explicitly as the tradeoff it is, not silently made."""
import json
import random
import subprocess
import time

import requests

IMAGE = "cloud-worker-a:latest"
VECTORS = 20000


def sh(*args, check=True, capture=True):
    return subprocess.run(args, check=check, capture_output=capture, text=True)


def docker_stats_mb(name):
    out = sh("docker", "stats", name, "--no-stream", "--format", "{{.MemUsage}}").stdout.strip()
    used = out.split("/")[0].strip()
    if used.endswith("GiB"):
        return float(used[:-3]) * 1024
    return float(used[:-3]) if used.endswith("MiB") else 0.0


def disk_usage_mb(volume):
    out = sh("docker", "run", "--rm", "-v", f"{volume}:/data", "alpine", "du", "-sm", "/data").stdout
    return int(out.split()[0]) if out.strip() else None


def wait_healthy(port, timeout_s=30):
    for _ in range(timeout_s * 2):
        try:
            if requests.get(f"http://localhost:{port}/health", timeout=2).status_code == 200:
                return True
        except requests.exceptions.RequestException:
            pass
        time.sleep(0.5)
    return False


def run_dim(dim, port):
    name = f"dim-{dim}"
    volume = f"{name}-data"
    sh("docker", "rm", "-f", name, check=False)
    sh("docker", "volume", "rm", "-f", volume, check=False)
    sh("docker", "run", "-d", "--name", name,
       "-e", f"VALORI_DIM={dim}", "-e", "VALORI_BIND=0.0.0.0:3000", "-e", "VALORI_INDEX=brute",
       "-e", "VALORI_EVENT_LOG_PATH=/data/events.log", "-e", "VALORI_SNAPSHOT_PATH=/data/state.snap",
       "-e", f"VALORI_MAX_RECORDS={VECTORS * 2}",
       "--memory", "1024m", "--cpus", "0.5",
       "-v", f"{volume}:/data", "-p", f"{port}:3000", IMAGE, check=False)
    if not wait_healthy(port, 30):
        return {"dimension": dim, "status": "failed_to_start"}

    r = requests.post(f"http://localhost:{port}/v1/namespaces", json={"name": "d"}, timeout=10)
    if r.status_code != 200:
        return {"dimension": dim, "status": "namespace_failed"}

    # axum's default request-body limit is 2MB; a JSON-encoded float
    # averages ~20 bytes, so keep each batch comfortably under that
    # regardless of dimension (found the hard way: dim=768+ at a fixed
    # batch of 200 hit a real 413 Payload Too Large — not a capacity
    # limit, just this test's own batching needing to scale with dim).
    batch_size = max(10, int(1_400_000 / (dim * 20)))
    rng = random.Random(123)
    t0 = time.time()
    for i in range(0, VECTORS, batch_size):
        n = min(batch_size, VECTORS - i)
        batch = [[rng.uniform(-1, 1) for _ in range(dim)] for _ in range(n)]
        r = requests.post(f"http://localhost:{port}/v1/vectors/batch-insert",
                           json={"batch": batch, "collection": "d"}, timeout=30)
        if r.status_code != 200:
            inspect = sh("docker", "inspect", name, "--format", "{{.State.Status}} {{.State.OOMKilled}}", check=False).stdout.strip()
            sh("docker", "rm", "-f", name, check=False)
            return {"dimension": dim, "status": "insert_failed", "detail": f"{r.status_code} at vec {i} ({inspect})"}
    insert_elapsed = time.time() - t0
    mem = docker_stats_mb(name)
    disk_mb = disk_usage_mb(volume)

    qv = [rng.uniform(-1, 1) for _ in range(dim)]
    times = []
    for _ in range(30):
        t = time.time()
        r = requests.post(f"http://localhost:{port}/v1/search", json={"query": qv, "k": 10, "collection": "d"}, timeout=15)
        if r.status_code == 200:
            times.append(time.time() - t)
    times.sort()

    before_hash = requests.get(f"http://localhost:{port}/v1/proof/state", timeout=10).json().get("final_state_hash")
    sh("docker", "stop", name)
    sh("docker", "start", name)
    restart_ok = wait_healthy(port, 60)
    hash_match = None
    restart_rss = None
    if restart_ok:
        restart_rss = docker_stats_mb(name)
        after_hash = requests.get(f"http://localhost:{port}/v1/proof/state", timeout=10).json().get("final_state_hash")
        hash_match = before_hash == after_hash

    sh("docker", "rm", "-f", name, check=False)
    sh("docker", "volume", "rm", "-f", volume, check=False)

    raw_vector_bytes_per_vec = dim * 4  # Q16.16 FxpScalar is i32 = 4 bytes/dim
    actual_bytes_per_vec = (mem * 1024 * 1024) / VECTORS if VECTORS else None

    return {
        "dimension": dim,
        "vectors": VECTORS,
        "raw_vector_memory_bytes_per_vector": raw_vector_bytes_per_vec,
        "actual_process_memory_bytes_per_vector": round(actual_bytes_per_vec, 1) if actual_bytes_per_vec else None,
        "peak_rss_mb": round(mem, 1),
        "restart_rss_mb": round(restart_rss, 1) if restart_rss else None,
        "disk_mb": disk_mb,
        "insert_elapsed_secs": round(insert_elapsed, 2),
        "insert_vectors_per_sec": round(VECTORS / insert_elapsed, 1) if insert_elapsed > 0 else None,
        "search_p50_ms": round(times[len(times) // 2] * 1000, 2) if times else None,
        "search_p95_ms": round(times[int(len(times) * 0.95)] * 1000, 2) if times else None,
        "search_p99_ms": round(times[int(len(times) * 0.99)] * 1000, 2) if times else None,
        "restart_hash_match": hash_match,
        "status": "supported" if hash_match else ("INTEGRITY_FAILURE" if hash_match is False else "restart_failed"),
    }


if __name__ == "__main__":
    import sys
    dims = [int(d) for d in sys.argv[1:]] or [384, 768, 1024, 1536]
    results = []
    for i, dim in enumerate(dims):
        r = run_dim(dim, 4000 + i)
        print(json.dumps(r, indent=2))
        results.append(r)
        if r.get("status") == "INTEGRITY_FAILURE":
            print("*** STOP: integrity failure ***")
            break
    out_path = "/Users/as-mac-0272/Desktop/sass/Valori-Kernel/benchmarks/capacity/results/dimension_comparison_768_1024_1536.json"
    with open(out_path, "w") as f:
        json.dump(results, f, indent=2)
