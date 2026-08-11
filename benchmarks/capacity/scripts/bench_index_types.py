#!/usr/bin/env python3
"""Index-type comparison at a FIXED, modest scale (10K vectors, 1GB/0.5CPU)
— deliberately small so this measures relative memory/latency behavior
between index types quickly, not absolute capacity at scale (that's the
separate, much more expensive per-cell RAM-boundary tests). Any index
type that fails outright at this small scale is real, useful information
regardless of scale."""
import json
import random
import subprocess
import time

import requests

IMAGE = "cloud-worker-a:latest"
DIM = 384
VECTORS = 10000


def sh(*args, check=True, capture=True):
    return subprocess.run(args, check=check, capture_output=capture, text=True)


def docker_stats_mb(name):
    out = sh("docker", "stats", name, "--no-stream", "--format", "{{.MemUsage}}").stdout.strip()
    used = out.split("/")[0].strip()
    if used.endswith("GiB"):
        return float(used[:-3]) * 1024
    return float(used[:-3]) if used.endswith("MiB") else 0.0


def wait_healthy(port, timeout_s=30):
    for _ in range(timeout_s * 2):
        try:
            if requests.get(f"http://localhost:{port}/health", timeout=2).status_code == 200:
                return True
        except requests.exceptions.RequestException:
            pass
        time.sleep(0.5)
    return False


def run_index(index_kind, port):
    name = f"idx-{index_kind}"
    sh("docker", "rm", "-f", name, check=False)
    sh("docker", "volume", "rm", "-f", f"{name}-data", check=False)
    sh("docker", "run", "-d", "--name", name,
       "-e", f"VALORI_DIM={DIM}", "-e", "VALORI_BIND=0.0.0.0:3000", "-e", f"VALORI_INDEX={index_kind}",
       "-e", "VALORI_EVENT_LOG_PATH=/data/events.log", "-e", "VALORI_SNAPSHOT_PATH=/data/state.snap",
       "-e", f"VALORI_MAX_RECORDS={VECTORS * 2}",
       "--memory", "1024m", "--cpus", "0.5",
       "-v", f"{name}-data:/data", "-p", f"{port}:3000", IMAGE, check=False)
    if not wait_healthy(port, 30):
        return {"index": index_kind, "status": "failed_to_start"}

    r = requests.post(f"http://localhost:{port}/v1/namespaces", json={"name": "x"}, timeout=10)
    if r.status_code != 200:
        return {"index": index_kind, "status": "namespace_failed", "detail": r.text[:200]}

    rng = random.Random(99)
    t0 = time.time()
    for i in range(0, VECTORS, 200):
        n = min(200, VECTORS - i)
        batch = [[rng.uniform(-1, 1) for _ in range(DIM)] for _ in range(n)]
        r = requests.post(f"http://localhost:{port}/v1/vectors/batch-insert",
                           json={"batch": batch, "collection": "x"}, timeout=30)
        if r.status_code != 200:
            inspect = sh("docker", "inspect", name, "--format", "{{.State.Status}} {{.State.OOMKilled}}", check=False).stdout.strip()
            sh("docker", "rm", "-f", name, check=False)
            return {"index": index_kind, "status": "insert_failed", "detail": f"{r.status_code}: {r.text[:200]} ({inspect})"}
    insert_elapsed = time.time() - t0
    mem_after_insert = docker_stats_mb(name)

    qv = [rng.uniform(-1, 1) for _ in range(DIM)]
    times = []
    for _ in range(30):
        t = time.time()
        r = requests.post(f"http://localhost:{port}/v1/search", json={"query": qv, "k": 10, "collection": "x"}, timeout=15)
        if r.status_code == 200:
            times.append(time.time() - t)
    times.sort()

    before_hash = requests.get(f"http://localhost:{port}/v1/proof/state", timeout=10).json().get("final_state_hash")
    sh("docker", "stop", name)
    t_restart = time.time()
    sh("docker", "start", name)
    restart_ok = wait_healthy(port, 60)
    recovery_s = time.time() - t_restart
    hash_match = None
    if restart_ok:
        after_hash = requests.get(f"http://localhost:{port}/v1/proof/state", timeout=10).json().get("final_state_hash")
        hash_match = before_hash == after_hash

    sh("docker", "rm", "-f", name, check=False)
    sh("docker", "volume", "rm", "-f", f"{name}-data", check=False)

    return {
        "index": index_kind,
        "vectors": VECTORS,
        "dimension": DIM,
        "insert_elapsed_secs": round(insert_elapsed, 2),
        "insert_vectors_per_sec": round(VECTORS / insert_elapsed, 1) if insert_elapsed > 0 else None,
        "peak_rss_mb_after_insert": round(mem_after_insert, 1),
        "search_p50_ms": round(times[len(times) // 2] * 1000, 2) if times else None,
        "search_p95_ms": round(times[int(len(times) * 0.95)] * 1000, 2) if times else None,
        "recovery_secs": round(recovery_s, 2),
        "restart_hash_match": hash_match,
        "status": "supported" if hash_match else ("INTEGRITY_FAILURE" if hash_match is False else "restart_failed"),
    }


if __name__ == "__main__":
    results = []
    for idx, kind in enumerate(["brute", "hnsw", "ivf", "bq"]):
        r = run_index(kind, 3900 + idx)
        print(json.dumps(r, indent=2))
        results.append(r)
        if r.get("status") == "INTEGRITY_FAILURE":
            print("*** STOP: integrity failure ***")
            break
    with open("/Users/as-mac-0272/Desktop/sass/Valori-Kernel/benchmarks/capacity/results/index_comparison.json", "w") as f:
        json.dump(results, f, indent=2)
