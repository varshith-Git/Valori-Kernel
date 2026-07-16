#!/usr/bin/env python3
"""
Namespace search latency benchmark — no internet required.
Uses random Gaussian vectors at dim=384 (same distribution as real embeddings).

Usage:
    # Start node first:
    VALORI_INDEX=hnsw VALORI_DIM=384 VALORI_MAX_RECORDS=60000 ./target/release/valori-node &

    # Brute-force baseline:
    VALORI_INDEX=brute VALORI_DIM=384 VALORI_MAX_RECORDS=60000 ./target/release/valori-node &

    python3 benchmarks/hnsw_ns_latency.py
"""

import json, random, time, sys, socket

HOST = "127.0.0.1"
PORT = 3000
DIM  = 384
K    = 10

# Raw socket with keep-alive — avoids http.client's create_connection issues on macOS.
_sock = None

def req(method, path, body=None):
    global _sock
    data = json.dumps(body).encode() if body is not None else b""
    hdrs = (
        f"{method} {path} HTTP/1.1\r\n"
        f"Host: {HOST}:{PORT}\r\n"
        f"Content-Type: application/json\r\n"
        f"Content-Length: {len(data)}\r\n"
        f"Connection: keep-alive\r\n\r\n"
    ).encode()
    for attempt in range(3):
        try:
            if _sock is None:
                _sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                _sock.settimeout(120)
                _sock.connect((HOST, PORT))
            _sock.sendall(hdrs + data)
            # Read HTTP response
            buf = b""
            while b"\r\n\r\n" not in buf:
                buf += _sock.recv(4096)
            header_end = buf.index(b"\r\n\r\n")
            headers_raw = buf[:header_end].decode()
            body_so_far = buf[header_end + 4:]
            # Parse Content-Length
            cl = 0
            for line in headers_raw.split("\r\n")[1:]:
                if line.lower().startswith("content-length:"):
                    cl = int(line.split(":", 1)[1].strip())
            while len(body_so_far) < cl:
                body_so_far += _sock.recv(4096)
            return json.loads(body_so_far[:cl]) if cl else {}
        except Exception:
            try: _sock.close()
            except: pass
            _sock = None
            if attempt == 2:
                raise

def wait_for_node(max_tries=30):
    for _ in range(max_tries):
        try:
            req("GET", "/health"); return
        except Exception:
            time.sleep(0.3)
    raise RuntimeError("Node did not start in time")

rng = random.Random(42)
def rv(): return [rng.gauss(0.0, 0.3) for _ in range(DIM)]

def batch_insert(vecs, coll, bs=200):
    for i in range(0, len(vecs), bs):
        req("POST", "/v1/vectors/batch-insert",
            {"batch": vecs[i:i+bs], "collection": coll})

def measure(query, coll, trials=50):
    times = []
    for _ in range(trials):
        t0 = time.perf_counter()
        req("POST", "/v1/search", {"query": query, "k": K, "collection": coll})
        times.append((time.perf_counter() - t0) * 1000)
    times.sort()
    n = len(times)
    return times[n // 2], times[int(n * 0.95)], times[-1]


def main():
    wait_for_node()
    h = req("GET", "/health")
    idx = h.get("index", h.get("index_type", "?"))
    print(f"Node up  index={idx}  dim={h['dim']}  capacity={h['records']['capacity']}")
    print()

    COLL = "ns_bench"
    try: req("DELETE", f"/v1/namespaces/{COLL}")
    except Exception: pass
    time.sleep(0.1)
    req("POST", "/v1/namespaces", {"name": COLL})

    all_vecs = [rv() for _ in range(50_001)]
    qvec = rv()

    # Previously measured brute-force baselines (dim=384 k=10, same hardware)
    brute_p50 = {1_000: 6.5, 5_000: 28.8, 10_000: 56.3, 25_000: 138.9, 50_000: 275.1}

    checkpoints = [1_000, 5_000, 10_000, 25_000, 50_000]
    results = []
    inserted = 0

    print(f"{'N':>8}  {'p50 ms':>8}  {'p95 ms':>8}  {'p99 ms':>8}  {'vs brute':>10}")
    print("-" * 54)

    for n in checkpoints:
        need = n - inserted
        batch_insert(all_vecs[inserted:n], COLL)
        inserted = n

        p50, p95, p99 = measure(qvec, COLL)
        baseline = brute_p50.get(n)
        speedup  = f"{baseline / p50:.0f}×" if baseline else "?"
        print(f"{n:>8}  {p50:>8.2f}  {p95:>8.2f}  {p99:>8.2f}  {speedup:>10}")
        results.append({"n": n, "p50_ms": round(p50, 2), "p95_ms": round(p95, 2), "p99_ms": round(p99, 2)})

    print()
    print(f"index={idx}  dim={DIM}  k={K}  trials=50 per checkpoint")


if __name__ == "__main__":
    main()
