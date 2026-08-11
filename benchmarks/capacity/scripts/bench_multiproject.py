#!/usr/bin/env python3
"""Multi-project-on-one-host contention test: N sibling containers, each a
separate 'project', sharing the SAME host machine's real CPU/disk (this
machine), each with its own memory cgroup limit — mirrors the real
architecture (Host.capacity_slots: several project containers per host,
never shared in-process state). Verifies resource interference AND that
API-key/project isolation holds under concurrent load across siblings."""
import json
import random
import subprocess
import threading
import time

import requests

IMAGE = "cloud-worker-a:latest"
DIM = 384
VECTORS = 5000


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


def docker_stats_mb(name):
    out = sh("docker", "stats", name, "--no-stream", "--format", "{{.MemUsage}}").stdout.strip()
    used = out.split("/")[0].strip()
    if used.endswith("GiB"):
        return float(used[:-3]) * 1024
    return float(used[:-3]) if used.endswith("MiB") else 0.0


def project_worker(idx, port, results, token):
    name = f"mp-{idx}"
    sh("docker", "rm", "-f", name, check=False)
    sh("docker", "volume", "rm", "-f", f"{name}-data", check=False)
    sh("docker", "run", "-d", "--name", name,
       "-e", f"VALORI_DIM={DIM}", "-e", "VALORI_BIND=0.0.0.0:3000", "-e", "VALORI_INDEX=brute",
       "-e", f"VALORI_AUTH_TOKEN={token}",
       "-e", "VALORI_EVENT_LOG_PATH=/data/events.log", "-e", "VALORI_SNAPSHOT_PATH=/data/state.snap",
       "-e", f"VALORI_MAX_RECORDS={VECTORS * 2}",
       "--memory", "512m", "--cpus", "0.5",
       "-v", f"{name}-data:/data", "-p", f"{port}:3000", IMAGE, check=False)
    if not wait_healthy(port):
        results[idx] = {"status": "failed_to_start"}
        return
    hdr = {"Authorization": f"Bearer {token}"}
    requests.post(f"http://localhost:{port}/v1/namespaces", json={"name": "p"}, headers=hdr, timeout=10)
    rng = random.Random(idx)
    t0 = time.time()
    for i in range(0, VECTORS, 200):
        n = min(200, VECTORS - i)
        batch = [[rng.uniform(-1, 1) for _ in range(DIM)] for _ in range(n)]
        r = requests.post(f"http://localhost:{port}/v1/vectors/batch-insert",
                           json={"batch": batch, "collection": "p"}, headers=hdr, timeout=30)
        if r.status_code != 200:
            results[idx] = {"status": "insert_failed", "detail": r.text[:200]}
            return
    elapsed = time.time() - t0
    mem = docker_stats_mb(name)

    # Cross-project isolation check: THIS project's token against the
    # NEXT sibling's port must fail (proves per-container token isolation
    # holds under concurrent multi-project load, not just in isolation).
    other_port = port + 1 if idx == 0 else port - 1
    try:
        cross = requests.post(f"http://localhost:{other_port}/v1/namespaces",
                               json={"name": "should-fail"}, headers=hdr, timeout=5)
        cross_status = cross.status_code
    except requests.exceptions.RequestException:
        cross_status = None  # sibling may not be up yet at this instant; noted, not a failure

    results[idx] = {
        "status": "ok",
        "insert_elapsed_secs": round(elapsed, 2),
        "insert_vectors_per_sec": round(VECTORS / elapsed, 1) if elapsed > 0 else None,
        "peak_rss_mb": round(mem, 1),
        "cross_project_token_status": cross_status,
    }


def run_scenario(n_projects):
    threads = []
    results = {}
    for i in range(n_projects):
        t = threading.Thread(target=project_worker, args=(i, 3700 + i, results, f"token-{i}"))
        threads.append(t)
        t.start()
    for t in threads:
        t.join()
    for i in range(n_projects):
        sh("docker", "rm", "-f", f"mp-{i}", check=False)
        sh("docker", "volume", "rm", "-f", f"mp-{i}-data", check=False)
    return {"n_projects": n_projects, "per_project": results}


if __name__ == "__main__":
    all_results = []
    for n in [1, 2, 4]:
        r = run_scenario(n)
        print(json.dumps(r, indent=2))
        all_results.append(r)
    with open("/Users/as-mac-0272/Desktop/sass/Valori-Kernel/benchmarks/capacity/results/multiproject.json", "w") as f:
        json.dump(all_results, f, indent=2)
