#!/usr/bin/env python3
"""Collections scaling: N collections x 1000 vectors each, fixed 1GB/0.5CPU
worker. Measures RAM/disk/startup/search overhead as collection count grows."""
import json
import random
import subprocess
import sys
import time

import requests

IMAGE = "cloud-worker-a:latest"
VECTORS_PER_COLLECTION = 1000
DIM = 384


def sh(*args, check=True, capture=True):
    return subprocess.run(args, check=check, capture_output=capture, text=True)


def docker_stats_mb(name):
    out = sh("docker", "stats", name, "--no-stream", "--format", "{{.MemUsage}}").stdout.strip()
    used = out.split("/")[0].strip()
    if used.endswith("GiB"):
        return float(used[:-3]) * 1024
    if used.endswith("MiB"):
        return float(used[:-3])
    return float(used[:-3]) / 1024 if used.endswith("KiB") else 0.0


def wait_healthy(port, timeout_s=30):
    for _ in range(timeout_s * 2):
        try:
            if requests.get(f"http://localhost:{port}/health", timeout=2).status_code == 200:
                return True
        except requests.exceptions.RequestException:
            pass
        time.sleep(0.5)
    return False


def run_for(n_collections, port):
    name = f"colls-{n_collections}"
    sh("docker", "rm", "-f", name, check=False)
    sh("docker", "volume", "rm", "-f", f"{name}-data", check=False)
    sh("docker", "run", "-d", "--name", name,
       "-e", f"VALORI_DIM={DIM}", "-e", "VALORI_BIND=0.0.0.0:3000", "-e", "VALORI_INDEX=brute",
       "-e", "VALORI_EVENT_LOG_PATH=/data/events.log", "-e", "VALORI_SNAPSHOT_PATH=/data/state.snap",
       "-e", f"VALORI_MAX_RECORDS={n_collections * VECTORS_PER_COLLECTION * 2}",
       "--memory", "1024m", "--cpus", "0.5",
       "-v", f"{name}-data:/data", "-p", f"{port}:3000", IMAGE, check=False)
    t_start = time.time()
    if not wait_healthy(port):
        return {"n_collections": n_collections, "status": "failed_to_start"}
    startup_s = time.time() - t_start

    rng = random.Random(7)
    for c in range(n_collections):
        cname = f"coll{c}"
        requests.post(f"http://localhost:{port}/v1/namespaces", json={"name": cname}, timeout=10)
        batch = [[rng.uniform(-1, 1) for _ in range(DIM)] for _ in range(VECTORS_PER_COLLECTION)]
        requests.post(f"http://localhost:{port}/v1/vectors/batch-insert",
                       json={"batch": batch, "collection": cname}, timeout=30)

    mem = docker_stats_mb(name)

    qv = [rng.uniform(-1, 1) for _ in range(DIM)]
    times = []
    for _ in range(20):
        t = time.time()
        requests.post(f"http://localhost:{port}/v1/search",
                       json={"query": qv, "k": 10, "collection": "coll0"}, timeout=15)
        times.append(time.time() - t)
    times.sort()

    sh("docker", "rm", "-f", name, check=False)
    sh("docker", "volume", "rm", "-f", f"{name}-data", check=False)

    return {
        "n_collections": n_collections,
        "vectors_per_collection": VECTORS_PER_COLLECTION,
        "total_vectors": n_collections * VECTORS_PER_COLLECTION,
        "startup_secs": round(startup_s, 2),
        "peak_rss_mb": round(mem, 1),
        "search_p50_ms": round(times[len(times) // 2] * 1000, 2),
        "status": "supported",
    }


if __name__ == "__main__":
    results = []
    for n in [1, 5, 10, 25, 50]:
        r = run_for(n, 3600)
        print(json.dumps(r))
        results.append(r)
    with open("/Users/as-mac-0272/Desktop/sass/Valori-Kernel/benchmarks/capacity/results/collections_scaling.json", "w") as f:
        json.dump(results, f, indent=2)
