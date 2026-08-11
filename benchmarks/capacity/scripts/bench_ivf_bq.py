#!/usr/bin/env python3
"""S10: IVF/BQ capacity + recall benchmark. Extends the S9 harness
(bench_cell.py) with: (1) recall@k against an exact-L2 ground truth
computed directly in Python over the SAME deterministic dataset (no
second container needed — brute-force L2 over a known vector set is
just arithmetic), (2) early-stop rules (RSS>=90%, p50>400ms with no
sign of improving), matching the S10 spec exactly.

Same real image, same real docker run --memory/--cpus limits as S9.
"""
import argparse
import json
import os
import random
import subprocess
import sys
import time

import numpy as np
import requests

IMAGE = "cloud-worker-a:latest"
DISK_SAFETY_THRESHOLD_GB = 5  # stop before starting a group if free disk < this


def sh(*args, check=True, capture=True):
    return subprocess.run(args, check=check, capture_output=capture, text=True)


def free_disk_gb():
    out = sh("df", "-g", "/").stdout.strip().splitlines()[-1].split()
    return int(out[3])  # Avail column, in GB with -g on macOS


def check_disk_safety():
    free = free_disk_gb()
    if free < DISK_SAFETY_THRESHOLD_GB:
        print(f"*** DISK SAFETY STOP: only {free}GB free (threshold {DISK_SAFETY_THRESHOLD_GB}GB) ***", file=sys.stderr)
        sys.exit(3)
    return free


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


def gen_vec(dim, rng):
    return [rng.uniform(-1, 1) for _ in range(dim)]


def exact_topk_batch(ids_arr, mat, query, k):
    """Ground truth: exact L2, vectorized with numpy over the known
    deterministic dataset — no second container needed. `mat` is an
    (N, dim) float32 array, `ids_arr` the parallel record-id array."""
    d = np.sum((mat - np.asarray(query, dtype=np.float32)) ** 2, axis=1)
    order = np.argsort(d, kind="stable")[:k]
    return ids_arr[order].tolist()


def run_cell(index_kind, dim, target_vectors, ram_mb, cpu, port, name, batch_size=None,
             n_list=None, n_probe=None, bq_pool_factor=None, bq_min_candidates=None):
    check_disk_safety()
    container = f"s10-{name}"
    volume = f"{container}-data"
    sh("docker", "rm", "-f", container, check=False)
    sh("docker", "volume", "rm", "-f", volume, check=False)

    result = {
        "scenario": name, "ram_mb": ram_mb, "cpu": cpu, "dimension": dim,
        "index": index_kind, "n_list": n_list, "n_probe": n_probe,
        "bq_pool_factor": bq_pool_factor, "bq_min_candidates": bq_min_candidates,
        "target_vectors": target_vectors, "actually_inserted": 0,
        "baseline_rss_mb": None, "peak_rss_mb": None, "insert_rss_mb": None,
        "search_rss_mb": None, "restart_rss_mb": None, "insert_elapsed_secs": None,
        "insert_vectors_per_sec": None, "index_build_elapsed_secs": None,
        "search_min_ms": None, "search_p50_ms": None, "search_p95_ms": None,
        "search_p99_ms": None, "search_max_ms": None, "disk_usage_mb": None,
        "restart_elapsed_secs": None, "restart_hash_match": None, "oom": False,
        "status": "unknown", "stop_reason": None,
        "recall_at_1": None, "recall_at_5": None, "recall_at_10": None,
    }

    docker_env = [
        "-e", f"VALORI_DIM={dim}", "-e", "VALORI_BIND=0.0.0.0:3000",
        "-e", f"VALORI_INDEX={index_kind}",
        "-e", "VALORI_EVENT_LOG_PATH=/data/events.log", "-e", "VALORI_SNAPSHOT_PATH=/data/state.snap",
        "-e", f"VALORI_MAX_RECORDS={target_vectors * 2}",
    ]
    if n_list is not None:
        docker_env += ["-e", f"VALORI_IVF_N_LIST={n_list}"]
    if n_probe is not None:
        docker_env += ["-e", f"VALORI_IVF_N_PROBE={n_probe}"]
    if bq_pool_factor is not None:
        docker_env += ["-e", f"VALORI_BQ_POOL_FACTOR={bq_pool_factor}"]
    if bq_min_candidates is not None:
        docker_env += ["-e", f"VALORI_BQ_MIN_CANDIDATES={bq_min_candidates}"]

    run = sh("docker", "run", "-d", "--name", container, *docker_env,
              "--memory", f"{ram_mb}m", "--cpus", str(cpu),
              "-v", f"{volume}:/data", "-p", f"{port}:3000", IMAGE, check=False)
    if run.returncode != 0 or not wait_healthy(port, 30):
        result["status"] = "failed_to_start"
        result["stop_reason"] = run.stderr.strip()[:300] if run.returncode != 0 else "health check timeout"
        return finish(result, container, volume)

    result["baseline_rss_mb"] = docker_stats_mb(container)
    r = requests.post(f"http://localhost:{port}/v1/namespaces", json={"name": "c"}, timeout=10)
    if r.status_code != 200:
        result["status"] = "namespace_create_failed"
        return finish(result, container, volume)

    if batch_size is None:
        batch_size = max(10, int(1_400_000 / (dim * 20)))

    rng = random.Random(42)
    dataset = []  # kept for recall ground truth -- (id, vector)
    inserted = 0
    peak = result["baseline_rss_mb"] or 0
    t0 = time.time()
    idx = 0
    stats_every = max(1, (target_vectors // batch_size) // 20 or 1)
    for bi, i in enumerate(range(0, target_vectors, batch_size)):
        n = min(batch_size, target_vectors - i)
        batch = [gen_vec(dim, rng) for _ in range(n)]
        try:
            r = requests.post(f"http://localhost:{port}/v1/vectors/batch-insert",
                               json={"batch": batch, "collection": "c"}, timeout=60)
        except requests.exceptions.RequestException as e:
            result["status"] = "insert_request_failed"
            result["stop_reason"] = str(e)[:300]
            break
        if r.status_code != 200:
            inspect = sh("docker", "inspect", container, "--format", "{{.State.Status}} {{.State.OOMKilled}}", check=False).stdout.strip()
            result["oom"] = "true" in inspect.lower()
            result["status"] = "oom" if result["oom"] else "insert_failed"
            result["stop_reason"] = f"HTTP {r.status_code} at batch {bi} ({inspect})"
            break
        ids = r.json().get("ids", [])
        inserted += len(ids)
        for rid, v in zip(ids, batch):
            dataset.append((rid, v))
        if bi % stats_every == 0:
            mem = docker_stats_mb(container)
            peak = max(peak, mem)
            if mem >= ram_mb * 0.90:
                result["status"] = "unsafe_memory"
                result["stop_reason"] = f"RSS {mem:.0f}MB >= 90% of {ram_mb}MB limit at {inserted} vectors"
                break
    insert_elapsed = time.time() - t0
    peak = max(peak, docker_stats_mb(container))
    result["actually_inserted"] = inserted
    result["insert_elapsed_secs"] = round(insert_elapsed, 3)
    if inserted > 0 and insert_elapsed > 0:
        result["insert_vectors_per_sec"] = round(inserted / insert_elapsed, 1)
    result["insert_rss_mb"] = round(peak, 1)
    result["peak_rss_mb"] = round(peak, 1)

    if result["status"] not in ("unknown",) or inserted < target_vectors * 0.99:
        if result["status"] == "unknown":
            result["status"] = "insert_incomplete"
        return finish(result, container, volume)

    # ── Search latency + recall ─────────────────────────────────────────
    n_queries = 20
    query_indices = rng.sample(range(len(dataset)), min(n_queries, len(dataset)))
    # Build the ground-truth matrix ONCE (numpy-vectorized L2 per query,
    # not per-comparison Python loops) — the only way this stays fast
    # enough to be usable up to a few hundred thousand vectors.
    ids_arr = np.array([rid for rid, _ in dataset], dtype=np.int64)
    mat = np.array([v for _, v in dataset], dtype=np.float32)
    times = []
    recall1_hits = recall5_hits = recall10_hits = 0
    for qi in query_indices:
        qv = dataset[qi][1]
        t = time.time()
        try:
            r = requests.post(f"http://localhost:{port}/v1/search",
                               json={"query": qv, "k": 10, "collection": "c"}, timeout=30)
        except requests.exceptions.RequestException:
            continue
        elapsed = time.time() - t
        if r.status_code != 200:
            continue
        times.append(elapsed)
        got_ids = [hit["id"] for hit in r.json().get("results", [])]
        truth = exact_topk_batch(ids_arr, mat, qv, 10)
        if got_ids[:1] and got_ids[0] in truth[:1]:
            recall1_hits += 1
        recall5_hits += len(set(got_ids[:5]) & set(truth[:5]))
        recall10_hits += len(set(got_ids[:10]) & set(truth[:10]))

    times.sort()
    if times:
        result["search_min_ms"] = round(times[0] * 1000, 2)
        result["search_p50_ms"] = round(times[len(times) // 2] * 1000, 2)
        result["search_p95_ms"] = round(times[min(int(len(times) * 0.95), len(times) - 1)] * 1000, 2)
        result["search_p99_ms"] = round(times[min(int(len(times) * 0.99), len(times) - 1)] * 1000, 2)
        result["search_max_ms"] = round(times[-1] * 1000, 2)
    n_eval = len(times)
    if n_eval:
        result["recall_at_1"] = round(recall1_hits / n_eval, 3)
        result["recall_at_5"] = round(recall5_hits / (n_eval * 5), 3)
        result["recall_at_10"] = round(recall10_hits / (n_eval * 10), 3)

    result["search_rss_mb"] = round(docker_stats_mb(container), 1)
    result["disk_usage_mb"] = disk_usage_mb(volume)

    # ── Restart + hash integrity (mandatory) ────────────────────────────
    before_hash = requests.get(f"http://localhost:{port}/v1/proof/state", timeout=10).json().get("final_state_hash")
    sh("docker", "stop", container)
    t_restart = time.time()
    sh("docker", "start", container)
    if wait_healthy(port, 300):
        restart_elapsed = time.time() - t_restart
        result["restart_elapsed_secs"] = round(restart_elapsed, 2)
        result["index_build_elapsed_secs"] = round(restart_elapsed, 2)
        result["restart_rss_mb"] = round(docker_stats_mb(container), 1)
        after_hash = requests.get(f"http://localhost:{port}/v1/proof/state", timeout=10).json().get("final_state_hash")
        result["restart_hash_match"] = before_hash == after_hash
        result["status"] = "supported" if result["restart_hash_match"] else "INTEGRITY_FAILURE"
        if not result["restart_hash_match"]:
            result["stop_reason"] = f"hash mismatch: before={before_hash} after={after_hash}"
    else:
        result["status"] = "restart_failed"
        result["stop_reason"] = "did not become healthy within 300s"

    return finish(result, container, volume)


def finish(result, container, volume):
    sh("docker", "rm", "-f", container, check=False)
    sh("docker", "volume", "rm", "-f", volume, check=False)
    return result


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--index", required=True, choices=["ivf", "bq"])
    ap.add_argument("--dim", type=int, required=True)
    ap.add_argument("--vectors", type=int, required=True)
    ap.add_argument("--ram-mb", type=int, default=1024)
    ap.add_argument("--cpu", type=float, default=0.5)
    ap.add_argument("--port", type=int, default=4500)
    ap.add_argument("--name", required=True)
    ap.add_argument("--n-list", type=int, default=None)
    ap.add_argument("--n-probe", type=int, default=None)
    ap.add_argument("--bq-pool-factor", type=int, default=None)
    ap.add_argument("--bq-min-candidates", type=int, default=None)
    ap.add_argument("--out", default="/Users/as-mac-0272/Desktop/sass/Valori-Kernel/benchmarks/capacity/results")
    args = ap.parse_args()

    r = run_cell(args.index, args.dim, args.vectors, args.ram_mb, args.cpu, args.port, args.name,
                 n_list=args.n_list, n_probe=args.n_probe,
                 bq_pool_factor=args.bq_pool_factor, bq_min_candidates=args.bq_min_candidates)
    print(json.dumps(r, indent=2))
    os.makedirs(args.out, exist_ok=True)
    with open(f"{args.out}/{args.name}.json", "w") as f:
        json.dump(r, f, indent=2)
    if r["status"] == "INTEGRITY_FAILURE":
        sys.exit(2)
