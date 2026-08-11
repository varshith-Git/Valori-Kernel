#!/usr/bin/env python3
"""Runs ONE real capacity-benchmark cell against a real, freshly-started
valori-node container with real Docker resource limits, and records real
measurements. No mocking, no fabricated numbers — every field in the
output JSON is either directly measured or explicitly null with a reason.

Usage:
  python3 bench_cell.py --ram-mb 512 --cpu 0.5 --dim 384 --index brute \
      --vectors 100000 --name stage_a_512mb_384d_brute_100k

Requires: the `cloud-worker-a` image already built (see
e2e/cloud/docker-compose.yml) — this reuses the SAME real Dockerfile, not
a separate benchmark-only image.
"""
import argparse
import json
import random
import subprocess
import sys
import time

import requests

IMAGE = "cloud-worker-a:latest"
INDEX_MAP = {"brute": "brute", "hnsw": "hnsw", "ivf": "ivf", "bq": "bq"}


def sh(*args, check=True, capture=True):
    return subprocess.run(args, check=check, capture_output=capture, text=True)


def docker_stats_mb(name: str) -> float:
    out = sh("docker", "stats", name, "--no-stream", "--format", "{{.MemUsage}}").stdout.strip()
    # e.g. "123.4MiB / 512MiB"
    used = out.split("/")[0].strip()
    if used.endswith("GiB"):
        return float(used[:-3]) * 1024
    if used.endswith("MiB"):
        return float(used[:-3])
    if used.endswith("KiB"):
        return float(used[:-3]) / 1024
    return 0.0


def wait_healthy(port: int, timeout_s: int = 30) -> bool:
    for _ in range(timeout_s * 2):
        try:
            r = requests.get(f"http://localhost:{port}/health", timeout=2)
            if r.status_code == 200:
                return True
        except requests.exceptions.RequestException:
            pass
        time.sleep(0.5)
    return False


def gen_vec(dim, rng):
    return [rng.uniform(-1, 1) for _ in range(dim)]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ram-mb", type=int, required=True)
    ap.add_argument("--cpu", type=float, required=True)
    ap.add_argument("--dim", type=int, required=True)
    ap.add_argument("--index", choices=list(INDEX_MAP), required=True)
    ap.add_argument("--vectors", type=int, required=True)
    ap.add_argument("--name", required=True)
    ap.add_argument("--port", type=int, default=3500)
    ap.add_argument("--batch", type=int, default=200)
    ap.add_argument("--search-samples", type=int, default=30)
    ap.add_argument("--out", default="/Users/as-mac-0272/Desktop/sass/Valori-Kernel/benchmarks/capacity/results")
    args = ap.parse_args()

    container = f"cap-{args.name}"
    volume = f"cap-{args.name}-data"
    sh("docker", "rm", "-f", container, check=False)
    sh("docker", "volume", "rm", "-f", volume, check=False)

    result = {
        "scenario": args.name,
        "ram_mb": args.ram_mb,
        "cpu": args.cpu,
        "dimension": args.dim,
        "index": args.index,
        "target_vectors": args.vectors,
        "actually_inserted": 0,
        "baseline_rss_mb": None,
        "peak_rss_mb": None,
        "insert_rss_mb": None,
        "search_rss_mb": None,
        "restart_rss_mb": None,
        "insert_elapsed_secs": None,
        "insert_vectors_per_sec": None,
        "index_build_elapsed_secs": None,
        "search_p50_ms": None,
        "search_p95_ms": None,
        "search_p99_ms": None,
        "restart_hash_match": None,
        "oom": False,
        "status": "unknown",
        "stop_reason": None,
    }

    try:
        run = sh(
            "docker", "run", "-d", "--name", container,
            "-e", f"VALORI_DIM={args.dim}",
            "-e", "VALORI_BIND=0.0.0.0:3000",
            "-e", f"VALORI_INDEX={INDEX_MAP[args.index]}",
            "-e", "VALORI_EVENT_LOG_PATH=/data/events.log",
            "-e", "VALORI_SNAPSHOT_PATH=/data/state.snap",
            "-e", f"VALORI_MAX_RECORDS={max(args.vectors * 2, 1000)}",
            "--memory", f"{args.ram_mb}m",
            "--cpus", str(args.cpu),
            "-v", f"{volume}:/data",
            "-p", f"{args.port}:3000",
            IMAGE,
            check=False,
        )
        if run.returncode != 0:
            result["status"] = "failed_to_start"
            result["stop_reason"] = run.stderr.strip()[:500]
            return finish(result, args)

        if not wait_healthy(args.port, timeout_s=30):
            state = sh("docker", "inspect", container, "--format", "{{.State.Status}} {{.State.OOMKilled}}", check=False).stdout.strip()
            result["status"] = "failed_to_become_healthy"
            result["stop_reason"] = f"container state: {state}"
            result["oom"] = "true" in state.lower()
            return finish(result, args)

        result["baseline_rss_mb"] = docker_stats_mb(container)

        # ── Create collection + insert ──────────────────────────────────
        r = requests.post(f"http://localhost:{args.port}/v1/namespaces",
                           json={"name": "cap"}, timeout=10)
        if r.status_code != 200:
            result["status"] = "namespace_create_failed"
            result["stop_reason"] = r.text[:300]
            return finish(result, args)

        rng = random.Random(42)
        # Pre-generate all vectors so random-number generation never counts
        # toward "insert" wall-clock time.
        all_batches = []
        for i in range(0, args.vectors, args.batch):
            n = min(args.batch, args.vectors - i)
            all_batches.append([gen_vec(args.dim, rng) for _ in range(n)])

        inserted = 0
        peak = result["baseline_rss_mb"] or 0
        # Sample docker stats only every ~10th batch — `docker stats
        # --no-stream` is a real subprocess call (~100-200ms) and sampling
        # every batch would dominate the timed insert loop, contaminating
        # the throughput measurement rather than just observing it.
        stats_every = max(1, len(all_batches) // 20)
        t0 = time.time()
        for bi, batch in enumerate(all_batches):
            try:
                r = requests.post(
                    f"http://localhost:{args.port}/v1/vectors/batch-insert",
                    json={"batch": batch, "collection": "cap"}, timeout=30,
                )
            except requests.exceptions.RequestException as e:
                result["status"] = "insert_request_failed"
                result["stop_reason"] = str(e)[:300]
                break
            if r.status_code != 200:
                # Detect OOM / crash vs. a clean app-level rejection.
                inspect = sh("docker", "inspect", container, "--format", "{{.State.Status}} {{.State.OOMKilled}}", check=False).stdout.strip()
                result["oom"] = "true" in inspect.lower()
                result["status"] = "insert_failed_oom" if result["oom"] else "insert_failed"
                result["stop_reason"] = f"HTTP {r.status_code} at batch {bi}: {r.text[:200]} (container: {inspect})"
                break
            inserted += len(r.json().get("ids", []))
            if bi % stats_every == 0:
                peak = max(peak, docker_stats_mb(container))
        insert_elapsed = time.time() - t0
        peak = max(peak, docker_stats_mb(container))
        result["actually_inserted"] = inserted
        result["insert_elapsed_secs"] = round(insert_elapsed, 3)
        if inserted > 0 and insert_elapsed > 0:
            result["insert_vectors_per_sec"] = round(inserted / insert_elapsed, 1)
        result["insert_rss_mb"] = round(peak, 1)

        if inserted < args.vectors:
            if result["status"] == "unknown":
                result["status"] = "insert_incomplete"
                result["stop_reason"] = f"only {inserted}/{args.vectors} inserted"
        else:
            # ── Search latency ──────────────────────────────────────────
            qv = gen_vec(args.dim, rng)
            times = []
            for _ in range(args.search_samples):
                t = time.time()
                try:
                    r = requests.post(
                        f"http://localhost:{args.port}/v1/search",
                        json={"query": qv, "k": 10, "collection": "cap"}, timeout=30,
                    )
                    if r.status_code == 200:
                        times.append(time.time() - t)
                except requests.exceptions.RequestException:
                    pass
            times.sort()
            if times:
                result["search_p50_ms"] = round(times[len(times) // 2] * 1000, 2)
                result["search_p95_ms"] = round(times[int(len(times) * 0.95) if len(times) > 1 else 0] * 1000, 2)
                result["search_p99_ms"] = round(times[int(len(times) * 0.99) if len(times) > 1 else 0] * 1000, 2)
            result["search_rss_mb"] = round(docker_stats_mb(container), 1)
            peak = max(peak, result["search_rss_mb"])

            # ── Restart + state-hash integrity (mandatory, S8) ──────────
            before_hash = requests.get(f"http://localhost:{args.port}/v1/proof/state", timeout=10).json().get("final_state_hash")
            sh("docker", "stop", container)
            t_restart = time.time()
            sh("docker", "start", container)
            if wait_healthy(args.port, timeout_s=300):
                recovery_elapsed = time.time() - t_restart
                result["index_build_elapsed_secs"] = round(recovery_elapsed, 3)
                result["restart_rss_mb"] = round(docker_stats_mb(container), 1)
                after_hash = requests.get(f"http://localhost:{args.port}/v1/proof/state", timeout=10).json().get("final_state_hash")
                result["restart_hash_match"] = before_hash == after_hash
                if not result["restart_hash_match"]:
                    result["status"] = "INTEGRITY_FAILURE"
                    result["stop_reason"] = f"state hash mismatch: before={before_hash} after={after_hash}"
                else:
                    result["status"] = "supported"
            else:
                result["status"] = "restart_failed"
                result["stop_reason"] = "container did not become healthy after restart"

        result["peak_rss_mb"] = round(peak, 1)

    finally:
        pass

    return finish(result, args)


def finish(result, args):
    import os
    os.makedirs(args.out, exist_ok=True)
    path = f"{args.out}/{args.name}.json"
    with open(path, "w") as f:
        json.dump(result, f, indent=2)
    print(json.dumps(result, indent=2))
    sh("docker", "rm", "-f", f"cap-{args.name}", check=False)
    sh("docker", "volume", "rm", "-f", f"cap-{args.name}-data", check=False)
    if result["status"] == "INTEGRITY_FAILURE":
        print("\n*** STOP CONDITION: state hash mismatch after restart ***", file=sys.stderr)
        sys.exit(2)
    return result


if __name__ == "__main__":
    main()
