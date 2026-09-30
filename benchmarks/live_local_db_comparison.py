#!/usr/bin/env python3
"""Same-data local retrieval comparison.

Downloads the public BEIR SciFact benchmark, embeds it once, and measures
FAISS, Qdrant, Milvus, Weaviate, and Valori when their local services are
available. Hosted/API-only systems are intentionally excluded.

Run: python benchmarks/live_local_db_comparison.py --dbs valori faiss qdrant
"""
from __future__ import annotations
import argparse, datetime, json, os, shutil, statistics, subprocess, tempfile, time, urllib.request, zipfile
from pathlib import Path
import numpy as np

ROOT = Path(__file__).resolve().parent
CACHE = ROOT / "public-data" / "scifact"
URL = "https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/scifact.zip"

def sh(*args): return subprocess.run(args, text=True, capture_output=True, check=False)

def git_commit():
    r = sh("git", "rev-parse", "HEAD")
    return r.stdout.strip() if r.returncode == 0 else None

def node_version(base_url):
    try:
        import requests
        r = requests.get(base_url + "/v1/version", timeout=10)
        return r.text.strip() if r.ok else None
    except Exception:
        return None

def download():
    CACHE.mkdir(parents=True, exist_ok=True)
    z = CACHE / "scifact.zip"
    if not z.exists():
        import requests
        r = requests.get(URL, timeout=120, verify=False); r.raise_for_status(); z.write_bytes(r.content)
    if not (CACHE / "scifact" / "corpus.jsonl").exists():
        with zipfile.ZipFile(z) as f: f.extractall(CACHE)
    base = CACHE / "scifact"
    def lines(name): return [json.loads(x) for x in (base / name).read_text(encoding="utf-8").splitlines()]
    corpus = lines("corpus.jsonl")
    queries = {x["_id"]: x for x in lines("queries.jsonl")}
    qrels = {}
    for row in (base / "qrels" / "test.tsv").read_text(encoding="utf-8").splitlines():
        cols = row.split("\t")
        if cols[0].lower() in {"query-id", "qid"}: continue
        if len(cols) == 4: q, _, d, score = cols
        else: q, d, score = cols
        qrels.setdefault(q, {})[d] = int(score)
    return corpus, queries, qrels

def metrics(results, qrels, ks=(3, 5, 10)):
    """results[q] must already be ranked and truncated to at least max(ks) items."""
    max_k = max(ks)
    recall = {k: [] for k in ks}
    ndcg = {k: [] for k in ks}
    mrr, complete = [], []
    for q in results:
        goldmap = qrels[q]
        gold = {d for d, s in goldmap.items() if s > 0}
        got = results.get(q, [])[:max_k]
        for k in ks:
            got_k = got[:k]
            recall[k].append(len(set(got_k) & gold) / max(1, len(gold)))
            dcg = sum(1 / np.log2(i + 2) for i, d in enumerate(got_k) if d in gold)
            ideal = sum(1 / np.log2(i + 2) for i in range(min(k, len(gold))))
            ndcg[k].append(dcg / ideal if ideal else 0)
        rr = 0.0
        for i, d in enumerate(got):
            if d in gold:
                rr = 1 / (i + 1)
                break
        mrr.append(rr)
        complete.append(set(gold).issubset(set(got)))
    out = {}
    for k in ks:
        out[f"recall_at_{k}"] = round(float(np.mean(recall[k])), 4)
        out[f"ndcg_at_{k}"] = round(float(np.mean(ndcg[k])), 4)
    out["mrr_at_10"] = round(float(np.mean(mrr)), 4)
    out["complete_context"] = round(float(np.mean(complete)), 4)
    out["query_count"] = len(results)
    return out

def graph_boost(vector_results, selected, doc_to_node, rec_to_doc, client, k, depth):
    """Expand Valori graph seeds, then keep vector order and append graph-only records."""
    boosted = {}
    for q in selected:
        ordered, seen = [], set()
        for doc in vector_results.get(q, []):
            ordered.append(doc); seen.add(doc)
        for doc in vector_results.get(q, []):
            node = doc_to_node.get(doc)
            if node is None:
                continue
            for rec_id in client.expand(node, max_depth=depth):
                rel_doc = rec_to_doc.get(rec_id)
                if rel_doc is not None and rel_doc not in seen:
                    ordered.append(rel_doc); seen.add(rel_doc)
                if len(ordered) >= k:
                    break
            if len(ordered) >= k:
                break
        boosted[q] = ordered[:k]
    return boosted

def public_claim_edges(selected, queries):
    """Use SciFact's public query metadata as claim -> evidence-paper edges."""
    edges = []
    for q in selected:
        for doc_id in queries[q].get("metadata", {}):
            edges.append((f"claim:{q}", doc_id))
    return edges

def post_json(session, base_url, path, payload):
    r = session.post(base_url + path, json=payload, timeout=60)
    r.raise_for_status()
    return r.json()

def latency_stats(durations_s):
    ms = [d * 1000 for d in durations_s]
    return {
        "latency_mean_ms": round(float(np.mean(ms)), 4),
        "latency_p50_ms": round(float(np.percentile(ms, 50)), 4),
        "latency_p95_ms": round(float(np.percentile(ms, 95)), 4),
        "latency_p99_ms": round(float(np.percentile(ms, 99)), 4),
    }

def valori_http(base_url, ids, emb, selected, queries, qrels, qemb, rel_edges, k):
    import requests
    session = requests.Session()
    collection = f"scifact_live_{int(time.time() * 1000)}"
    post_json(session, base_url, "/v1/namespaces", {
        "name": collection,
        "dimension": int(emb.shape[1]),
        "metric": "squared_l2",
    })
    rec_to_doc, doc_to_node = {}, {}
    t = time.perf_counter()
    for doc_id, vec in zip(ids, emb):
        up = post_json(session, base_url, "/v1/memory/upsert_vector", {
            "collection": collection,
            "vector": vec.tolist(),
        })
        rec_to_doc[up["record_id"]] = doc_id
        doc_to_node[doc_id] = up["chunk_node_id"]
    for q, vec in zip(selected, qemb):
        claim_id = f"claim:{q}"
        up = post_json(session, base_url, "/v1/memory/upsert_vector", {
            "collection": collection,
            "vector": vec.tolist(),
        })
        rec_to_doc[up["record_id"]] = claim_id
        doc_to_node[claim_id] = up["chunk_node_id"]
    graph_edges = 0
    for a, b in rel_edges:
        if a in doc_to_node and b in doc_to_node:
            post_json(session, base_url, "/v1/graph/edge", {
                "collection": collection,
                "from": doc_to_node[a],
                "to": doc_to_node[b],
                "kind": 5,
            })
            graph_edges += 1
    build = time.perf_counter() - t
    vector_results, graph_results = {}, {}
    vector_durations, graph_durations = [], []
    for q, vec in zip(selected, qemb):
        t0 = time.perf_counter()
        data = post_json(session, base_url, "/v1/memory/search_vector", {
            "collection": collection,
            "query_vector": vec.tolist(),
            "k": k,
            "rerank": False,
        })
        vector_durations.append(time.perf_counter() - t0)
        vector_results[q] = [
            rec_to_doc[x["record_id"]] for x in data["results"]
            if not rec_to_doc[x["record_id"]].startswith("claim:")
        ]
    for q, vec in zip(selected, qemb):
        t0 = time.perf_counter()
        data = post_json(session, base_url, "/v1/graphrag", {
            "collection": collection,
            "query_vector": vec.tolist(),
            "retrieval_k": k,
            "final_k": k + 10,
            "depth": 1,
            "graph_weight": 0.3,
        })
        graph_durations.append(time.perf_counter() - t0)
        graph_results[q] = [
            rec_to_doc[x["record_id"]] for x in data["hits"]
            if not rec_to_doc[x["record_id"]].startswith("claim:")
        ]
    assert set(vector_results) == set(graph_results) == set(selected), \
        "vector/graph/query id sets must match exactly"
    state_hash = None
    try:
        proof_resp = session.get(base_url + "/v1/proof/event-log", timeout=60)
        if proof_resp.ok and proof_resp.text.strip():
            proof = proof_resp.json()
            state_hash = proof.get("final_state_hash") or proof.get("state_hash")
    except Exception:
        state_hash = None
    return {
        "vector": {**metrics(vector_results, qrels), **latency_stats(vector_durations), "build_s": round(build, 4)},
        "vector_graph": {
            **metrics(graph_results, qrels),
            **latency_stats(graph_durations),
            "graph_edges": graph_edges,
            "state_hash": state_hash,
            "collection": collection,
            "note": "same paper+claim vectors as FAISS; Valori follows public SciFact claim-to-evidence graph edges",
        },
        "ingestion": {
            "documents_inserted": len(ids),
            "vectors_inserted": len(ids) + len(selected),
            "oracle_edges_inserted": graph_edges,
            "ingestion_duration_s": round(build, 4),
        },
    }

def validate_ab(out):
    """Data/math sanity checks only. Deliberately does NOT assert that the
    graph arm outperforms the vector arm -- a regression there is a valid
    experimental result, not a benchmark failure. Also deliberately does NOT
    assert any ordering across ndcg_at_3/5/10 -- nDCG's ideal-DCG denominator
    changes with the cutoff, so a correct implementation is not guaranteed to
    be monotonic in k (e.g. ndcg_at_3=0.91, ndcg_at_5=0.87, ndcg_at_10=0.89
    is legitimate)."""
    for name, m in out.get("systems", {}).items():
        if not isinstance(m, dict) or "recall_at_10" not in m:
            continue
        assert 0.0 <= m["recall_at_3"] <= m["recall_at_5"] <= m["recall_at_10"] <= 1.0, \
            f"{name}: recall_at_k not monotonic/in-range"
        assert 0.0 <= m["ndcg_at_3"] <= 1.0, f"{name}: ndcg_at_3 out of range"
        assert 0.0 <= m["ndcg_at_5"] <= 1.0, f"{name}: ndcg_at_5 out of range"
        assert 0.0 <= m["ndcg_at_10"] <= 1.0, f"{name}: ndcg_at_10 out of range"
        assert 0.0 <= m["mrr_at_10"] <= 1.0, f"{name}: mrr_at_10 out of range"
        assert 0.0 <= m["complete_context"] <= 1.0, f"{name}: complete_context out of range"
        assert m["latency_p99_ms"] >= m["latency_p95_ms"] >= m["latency_p50_ms"] >= 0.0, \
            f"{name}: latency percentiles not ordered"
        if "queries" in out:
            assert m["query_count"] == out["queries"], f"{name}: query_count does not match dataset query count"

def valori_http_autokg(base_url, ids, emb, selected, queries, qrels, qemb, co_mention_edges, k):
    """Sibling of valori_http() for arm C -- a FRESH collection so C's graph
    can never mix with B's oracle edges. Deliberately duplicates some of
    valori_http()'s shape rather than refactoring it: valori_http() is frozen
    (Phase B1.1) and this keeps that code path completely unmodified."""
    import requests
    session = requests.Session()
    collection = f"scifact_autokg_{int(time.time() * 1000)}"
    post_json(session, base_url, "/v1/namespaces", {
        "name": collection, "dimension": int(emb.shape[1]), "metric": "squared_l2",
    })
    rec_to_doc, doc_to_node = {}, {}
    t = time.perf_counter()
    for doc_id, vec in zip(ids, emb):
        up = post_json(session, base_url, "/v1/memory/upsert_vector", {"collection": collection, "vector": vec.tolist()})
        rec_to_doc[up["record_id"]] = doc_id
        doc_to_node[doc_id] = up["chunk_node_id"]
    for q, vec in zip(selected, qemb):
        claim_id = f"claim:{q}"
        up = post_json(session, base_url, "/v1/memory/upsert_vector", {"collection": collection, "vector": vec.tolist()})
        rec_to_doc[up["record_id"]] = claim_id
        doc_to_node[claim_id] = up["chunk_node_id"]
    graph_edges = 0
    for a, b in co_mention_edges:
        if a in doc_to_node and b in doc_to_node:
            post_json(session, base_url, "/v1/graph/edge", {
                "collection": collection, "from": doc_to_node[a], "to": doc_to_node[b], "kind": 5,
            })
            graph_edges += 1
    build = time.perf_counter() - t

    vector_results, graph_results = {}, {}
    vector_durations, graph_durations = [], []
    for q, vec in zip(selected, qemb):
        t0 = time.perf_counter()
        data = post_json(session, base_url, "/v1/memory/search_vector", {
            "collection": collection, "query_vector": vec.tolist(), "k": k, "rerank": False,
        })
        vector_durations.append(time.perf_counter() - t0)
        vector_results[q] = [rec_to_doc[x["record_id"]] for x in data["results"] if not rec_to_doc[x["record_id"]].startswith("claim:")]
    for q, vec in zip(selected, qemb):
        t0 = time.perf_counter()
        data = post_json(session, base_url, "/v1/graphrag", {
            "collection": collection, "query_vector": vec.tolist(),
            "retrieval_k": k, "final_k": k + 10, "depth": 1, "graph_weight": 0.3,
        })
        graph_durations.append(time.perf_counter() - t0)
        graph_results[q] = [rec_to_doc[x["record_id"]] for x in data["hits"] if not rec_to_doc[x["record_id"]].startswith("claim:")]
    assert set(vector_results) == set(graph_results) == set(selected), "vector/graph/query id sets must match exactly"

    return {
        "vector": {**metrics(vector_results, qrels), **latency_stats(vector_durations), "build_s": round(build, 4)},
        "vector_graph": {
            **metrics(graph_results, qrels), **latency_stats(graph_durations),
            "graph_edges": graph_edges, "collection": collection,
            "note": "same paper+claim vectors as B1.1; edges are automatically constructed (co_mentions_entity), not oracle",
        },
        "ingestion": {
            "documents_inserted": len(ids), "vectors_inserted": len(ids) + len(selected),
            "auto_edges_inserted": graph_edges, "ingestion_duration_s": round(build, 4),
        },
    }


def validate_abc(out):
    """Extends validate_ab() with the one invariant specific to the 3-arm
    design: the vector-only arm must be identical regardless of which graph
    (oracle or autokg) was layered on top of it, since vector search doesn't
    consult edges. A mismatch means the two collections' vectors diverged --
    a real bug, not an experimental outcome."""
    validate_ab(out)
    va = out["systems"].get("valori_http_vector")
    va2 = out["systems"].get("valori_http_vector_from_autokg_run")
    if va and va2:
        for key in ("recall_at_3", "recall_at_5", "recall_at_10", "ndcg_at_3", "ndcg_at_5",
                    "ndcg_at_10", "mrr_at_10", "complete_context", "query_count"):
            assert va[key] == va2[key], f"vector-only arm diverged between runs (key={key}): {va[key]} != {va2[key]}"


def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--dbs", nargs="+", default=["valori", "faiss", "qdrant", "milvus", "weaviate"])
    ap.add_argument("--valori-url", default=os.environ.get("VALORI_BENCH_URL", "http://127.0.0.1:3307"))
    ap.add_argument("--docs", type=int, default=1000, help="Document cap; always keeps judged evidence docs for selected queries.")
    ap.add_argument("--k", type=int, default=10); ap.add_argument("--queries", type=int, default=200)
    ap.add_argument("--out", default=None, help="Output JSON path; default benchmarks/LIVE_LOCAL_RESULTS.json")
    args = ap.parse_args()
    corpus, queries, qrels = download()
    docs = [x["title"] + "\n" + x["text"] for x in corpus]; ids = [x["_id"] for x in corpus]
    selected = [q for q in qrels if q in queries][:args.queries]
    emb_cache = CACHE / "all-MiniLM-L6-v2-corpus.npy"
    query_cache = CACHE / f"all-MiniLM-L6-v2-test-q{len(selected)}.npy"
    model = None
    def get_model():
        nonlocal model
        if model is None:
            from sentence_transformers import SentenceTransformer
            model = SentenceTransformer("sentence-transformers/all-MiniLM-L6-v2")
        return model
    if emb_cache.exists():
        emb = np.load(emb_cache).astype("float32")
    else:
        emb = get_model().encode(docs, batch_size=256, normalize_embeddings=True, show_progress_bar=True).astype("float32")
        np.save(emb_cache, emb)
    if query_cache.exists():
        qemb = np.load(query_cache).astype("float32")
    else:
        qemb = get_model().encode([queries[q]["text"] for q in selected], batch_size=256, normalize_embeddings=True).astype("float32")
        np.save(query_cache, qemb)
    rel_edges = public_claim_edges(selected, queries)
    if args.docs and args.docs < len(ids):
        must_keep = {d for q in selected for d, s in qrels[q].items() if s > 0}
        keep, seen = [], set()
        for i, doc_id in enumerate(ids):
            if doc_id in must_keep:
                keep.append(i); seen.add(i)
        for i, doc_id in enumerate(ids):
            if len(keep) >= args.docs:
                break
            if i not in seen:
                keep.append(i); seen.add(i)
        keep = sorted(keep)
        docs = [docs[i] for i in keep]
        ids = [ids[i] for i in keep]
        emb = emb[keep]
        kept = set(ids)
        rel_edges = [(a, b) for a, b in rel_edges if b in kept]
    all_emb = np.vstack([emb, qemb])
    all_ids = ids + [f"claim:{q}" for q in selected]
    out = {"dataset": "BEIR SciFact", "documents": len(docs), "queries": len(selected), "dimension": emb.shape[1], "relation_edges": len(rel_edges), "systems": {}}
    if "faiss" in args.dbs:
        import faiss
        idx = faiss.IndexFlatIP(emb.shape[1]); t = time.perf_counter(); idx.add(all_emb); build = time.perf_counter() - t
        t = time.perf_counter(); _, pos = idx.search(qemb, args.k); elapsed = time.perf_counter() - t
        res = {q: [all_ids[i] for i in row if not all_ids[i].startswith("claim:")] for q, row in zip(selected, pos)}
        out["systems"]["faiss"] = {**metrics(res, qrels), "build_s": round(build, 4), "search_ms": round(elapsed * 1000 / len(selected), 4)}
    if "valori" in args.dbs:
        try:
            from valoricore import MemoryClient
            from valoricore.kinds import NODE_CHUNK, EDGE_REFERS_TO
            valori_dir = tempfile.mkdtemp(prefix="valori_live_")
            c = MemoryClient(path=valori_dir, dim=emb.shape[1]); t = time.perf_counter()
            rec_to_doc, doc_to_rec, doc_to_node = {}, {}, {}
            for i in range(len(docs)):
                up = c.upsert_vector(emb[i].tolist())
                rec_to_doc[up["record_id"]] = ids[i]
                doc_to_rec[ids[i]] = up["record_id"]
                doc_to_node[ids[i]] = up["chunk_node_id"]
            for q, v in zip(selected, qemb):
                up = c.upsert_vector(v.tolist())
                claim_id = f"claim:{q}"
                rec_to_doc[up["record_id"]] = claim_id
                doc_to_rec[claim_id] = up["record_id"]
                doc_to_node[claim_id] = up["chunk_node_id"]
            graph_edges = 0
            for a, b in rel_edges:
                if a in doc_to_node and b in doc_to_node:
                    c.create_edge(doc_to_node[a], doc_to_node[b], EDGE_REFERS_TO)
                    graph_edges += 1
            build = time.perf_counter() - t; res = {}; t = time.perf_counter()
            for q, v in zip(selected, qemb):
                res[q] = [rec_to_doc[x["id"]] for x in c.semantic_search(queries[q]["text"], embed=lambda _: v.tolist(), k=args.k) if not rec_to_doc[x["id"]].startswith("claim:")]
            elapsed = time.perf_counter() - t
            out["systems"]["valori_vector"] = {**metrics(res, qrels), "build_s": round(build, 4), "search_ms": round(elapsed * 1000 / len(selected), 4)}
            t = time.perf_counter()
            seed_results = {}
            for q, v in zip(selected, qemb):
                seed_results[q] = [rec_to_doc[x["id"]] for x in c.semantic_search(queries[q]["text"], embed=lambda _: v.tolist(), k=args.k)]
            boosted = graph_boost(seed_results, selected, doc_to_node, rec_to_doc, c, args.k, depth=1)
            boosted = {q: [d for d in docs if not d.startswith("claim:")][:args.k] for q, docs in boosted.items()}
            graph_elapsed = time.perf_counter() - t
            out["systems"]["valori_vector_graph"] = {
                **metrics(boosted, qrels),
                "graph_edges": graph_edges,
                "graph_expand_ms": round(graph_elapsed * 1000 / len(selected), 4),
                "state_hash": c.get_state_hash(),
                "note": "same vectors as FAISS; adds public SciFact claim-to-evidence edges and deterministic graph expansion",
            }
            shutil.rmtree(valori_dir, ignore_errors=True)
        except Exception as e: out["systems"]["valori"] = {"status": "unavailable", "error": str(e)}
    if "valori-http" in args.dbs:
        try:
            http_res = valori_http(args.valori_url.rstrip("/"), ids, emb, selected, queries, qrels, qemb, rel_edges, args.k)
            out["systems"]["valori_http_vector"] = http_res["vector"]
            out["systems"]["valori_http_vector_graph"] = http_res["vector_graph"]
            out["ingestion"] = http_res["ingestion"]
        except Exception as e:
            out["systems"]["valori_http"] = {"status": "unavailable", "error": str(e)}
    if "valori-http-autokg" in args.dbs:
        if "valori_http_vector" not in out["systems"] or "valori_http_vector_graph" not in out["systems"]:
            raise SystemExit("valori-http-autokg requires valori-http to also run in the same invocation (recovery needs A and B)")
        from autokg_extract import extract_graph_for_texts
        from autokg_adapter import collapse_co_mentions, construction_stats, build_provenance, evaluate_edge_quality, compute_recovery
        try:
            id_text_pairs = [(ids[i], docs[i]) for i in range(len(ids))] + [(f"claim:{q}", queries[q]["text"]) for q in selected]
            t0 = time.perf_counter()
            graph = extract_graph_for_texts(id_text_pairs)
            extraction_duration = time.perf_counter() - t0
            t0 = time.perf_counter()
            co_mention_edges = collapse_co_mentions(graph)
            graph_build_duration = time.perf_counter() - t0
            autokg_res = valori_http_autokg(args.valori_url.rstrip("/"), ids, emb, selected, queries, qrels, qemb, co_mention_edges, args.k)
            out["systems"]["valori_http_vector_from_autokg_run"] = autokg_res["vector"]
            out["systems"]["valori_http_vector_autokg"] = autokg_res["vector_graph"]
            out["autokg"] = {
                "construction_stats": construction_stats(graph, co_mention_edges, extraction_duration, graph_build_duration),
                "edge_quality": evaluate_edge_quality(co_mention_edges, qrels),
            }
            recovery = {}
            va, vb, vc = out["systems"]["valori_http_vector"], out["systems"]["valori_http_vector_graph"], out["systems"]["valori_http_vector_autokg"]
            for metric_key in ("recall_at_3", "recall_at_5", "recall_at_10", "ndcg_at_3", "ndcg_at_5", "ndcg_at_10", "mrr_at_10", "complete_context"):
                recovery[metric_key] = compute_recovery(va[metric_key], vb[metric_key], vc[metric_key])
            out["recovery"] = recovery
            provenance_path = CACHE / "auto_kg_graph_b1_2.json"
            provenance_path.write_text(json.dumps({
                "graph": json.loads(graph.model_dump_json()),
                "co_mention_edges": co_mention_edges,
                "provenance": build_provenance(graph, co_mention_edges),
            }, indent=2), encoding="utf-8")
        except Exception as e:
            out["systems"]["valori_http_autokg"] = {"status": "unavailable", "error": str(e)}
    for name in ("qdrant", "milvus", "weaviate"):
        if name in args.dbs: out["systems"][name] = {"status": "not-run", "note": "local adapter pending; no fabricated result"}
    out["meta"] = {
        "phase": "B1.2" if "valori-http-autokg" in args.dbs else "B1.1",
        "git_commit": git_commit(),
        "timestamp_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "dataset": "BEIR SciFact",
        "documents": len(docs),
        "queries": len(selected),
        "embedding_model": "sentence-transformers/all-MiniLM-L6-v2",
        "embedding_dim": int(emb.shape[1]),
        "index": "valori-http (brute-force default)" if "valori-http" in args.dbs else None,
        "graph_source": "oracle_scifact",
        "graphrag_params": {"retrieval_k": args.k, "final_k": args.k + 10, "depth": 1, "graph_weight": 0.3},
        "valori_url": args.valori_url if "valori-http" in args.dbs else None,
        "node_version": node_version(args.valori_url.rstrip("/")) if "valori-http" in args.dbs else None,
    }
    validate_abc(out)
    out_path = Path(args.out) if args.out else (ROOT / "LIVE_LOCAL_RESULTS.json")
    out_path.write_text(json.dumps(out, indent=2), encoding="utf-8")
    print(json.dumps(out, indent=2))
if __name__ == "__main__": main()
