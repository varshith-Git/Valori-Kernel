#!/usr/bin/env python3
"""Concurrency test on a single 1GB/0.5CPU worker pre-loaded with 20K
vectors: N concurrent clients issuing a mixed read/write workload."""
import concurrent.futures
import json
import random
import subprocess
import time

import requests

IMAGE = "cloud-worker-a:latest"
DIM = 384
PRELOAD = 20000
PORT = 3800
DURATION_S = 15


def sh(*args, check=True, capture=True):
    return subprocess.run(args, check=check, capture_output=capture, text=True)


def wait_healthy(port, timeout_s=30):
    for _ in range(timeout_s * 2):
        try:
            if requests.get(f"http://localhost:{port}/health", timeout=2).status_code == 200:
                return True
        except requests.exceptions.RequestException:
            pass
        time.sleep(0.5)
    return False


def client_task(client_id, stop_at, latencies_read, latencies_write, errors):
    rng = random.Random(client_id)
    qv = [rng.uniform(-1, 1) for _ in range(DIM)]
    while time.time() < stop_at:
        is_write = rng.random() < 0.2  # 80/20 read-heavy mix
        t = time.time()
        try:
            if is_write:
                r = requests.post(f"http://localhost:{PORT}/v1/vectors/batch-insert",
                                   json={"batch": [[rng.uniform(-1, 1) for _ in range(DIM)]], "collection": "c"},
                                   timeout=10)
                latencies_write.append(time.time() - t)
            else:
                r = requests.post(f"http://localhost:{PORT}/v1/search",
                                   json={"query": qv, "k": 10, "collection": "c"}, timeout=10)
                latencies_read.append(time.time() - t)
            if r.status_code != 200:
                errors.append(r.status_code)
        except requests.exceptions.RequestException as e:
            errors.append(str(e)[:80])


def run_for(n_clients):
    latencies_read, latencies_write, errors = [], [], []
    stop_at = time.time() + DURATION_S
    with concurrent.futures.ThreadPoolExecutor(max_workers=n_clients) as ex:
        futs = [ex.submit(client_task, i, stop_at, latencies_read, latencies_write, errors) for i in range(n_clients)]
        concurrent.futures.wait(futs)
    latencies_read.sort()
    latencies_write.sort()

    def pct(arr, p):
        if not arr:
            return None
        return round(arr[int(len(arr) * p)] * 1000, 2) if int(len(arr) * p) < len(arr) else round(arr[-1] * 1000, 2)

    return {
        "n_clients": n_clients,
        "duration_s": DURATION_S,
        "total_reads": len(latencies_read),
        "total_writes": len(latencies_write),
        "read_throughput_per_sec": round(len(latencies_read) / DURATION_S, 1),
        "write_throughput_per_sec": round(len(latencies_write) / DURATION_S, 1),
        "read_p50_ms": pct(latencies_read, 0.5),
        "read_p95_ms": pct(latencies_read, 0.95),
        "read_p99_ms": pct(latencies_read, 0.99),
        "write_p50_ms": pct(latencies_write, 0.5),
        "write_p95_ms": pct(latencies_write, 0.95),
        "error_count": len(errors),
    }


def main():
    name = "conc-worker"
    sh("docker", "rm", "-f", name, check=False)
    sh("docker", "volume", "rm", "-f", f"{name}-data", check=False)
    sh("docker", "run", "-d", "--name", name,
       "-e", f"VALORI_DIM={DIM}", "-e", "VALORI_BIND=0.0.0.0:3000", "-e", "VALORI_INDEX=brute",
       "-e", "VALORI_EVENT_LOG_PATH=/data/events.log", "-e", "VALORI_SNAPSHOT_PATH=/data/state.snap",
       "-e", f"VALORI_MAX_RECORDS={PRELOAD * 3}",
       "--memory", "1024m", "--cpus", "0.5",
       "-v", f"{name}-data:/data", "-p", f"{PORT}:3000", IMAGE, check=False)
    if not wait_healthy(PORT):
        print(json.dumps({"status": "failed_to_start"}))
        return

    requests.post(f"http://localhost:{PORT}/v1/namespaces", json={"name": "c"}, timeout=10)
    rng = random.Random(1)
    for i in range(0, PRELOAD, 200):
        n = min(200, PRELOAD - i)
        batch = [[rng.uniform(-1, 1) for _ in range(DIM)] for _ in range(n)]
        requests.post(f"http://localhost:{PORT}/v1/vectors/batch-insert",
                       json={"batch": batch, "collection": "c"}, timeout=30)

    results = []
    for n in [1, 10, 25]:
        r = run_for(n)
        print(json.dumps(r))
        results.append(r)

    sh("docker", "rm", "-f", name, check=False)
    sh("docker", "volume", "rm", "-f", f"{name}-data", check=False)
    with open("/Users/as-mac-0272/Desktop/sass/Valori-Kernel/benchmarks/capacity/results/concurrency.json", "w") as f:
        json.dump(results, f, indent=2)


if __name__ == "__main__":
    main()
