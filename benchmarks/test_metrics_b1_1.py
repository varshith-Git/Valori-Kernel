import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from live_local_db_comparison import metrics


def test_metrics_multi_cutoff_and_mrr_at_10():
    qrels = {
        "q1": {"d1": 1, "d2": 1, "d3": 0},
        "q2": {"d4": 1},
    }
    results = {
        "q1": ["d1", "dX", "d2", "dY", "dZ", "d6", "d7", "d8", "d9", "d10"],
        "q2": ["dA", "d4", "dB", "dC", "dD", "dE", "dF", "dG", "dH", "dI"],
    }
    out = metrics(results, qrels, ks=(3, 5, 10))

    # q1: gold {d1, d2}, both within top-3 ("d1","dX","d2") -> 2/2; same at 5, 10.
    # q2: gold {d4}, within top-3 ("dA","d4","dB") -> 1/1; same at 5, 10.
    assert out["recall_at_3"] == 1.0
    assert out["recall_at_5"] == 1.0
    assert out["recall_at_10"] == 1.0
    assert out["recall_at_10"] >= out["recall_at_5"] >= out["recall_at_3"]

    # MRR@10: q1 first hit at rank 1 -> 1.0; q2 first hit at rank 2 -> 0.5.
    assert out["mrr_at_10"] == round((1.0 + 0.5) / 2, 4)

    # complete_context@10: both queries' full gold sets are subsets of their top-10.
    assert out["complete_context"] == 1.0

    assert out["query_count"] == 2
    # nDCG is NOT asserted to be monotonic across cutoffs (the ideal-DCG
    # denominator changes with k) -- only that each value is in range.
    assert 0.0 <= out["ndcg_at_3"] <= 1.0
    assert 0.0 <= out["ndcg_at_5"] <= 1.0
    assert 0.0 <= out["ndcg_at_10"] <= 1.0


def test_metrics_no_hits_scores_zero():
    qrels = {"q1": {"d1": 1}}
    results = {"q1": ["dX", "dY", "dZ"]}
    out = metrics(results, qrels, ks=(3,))
    assert out["recall_at_3"] == 0.0
    assert out["ndcg_at_3"] == 0.0
    assert out["mrr_at_10"] == 0.0
    assert out["complete_context"] == 0.0


from live_local_db_comparison import latency_stats


def test_latency_stats_percentiles():
    durations_s = [0.010, 0.020, 0.030, 0.040, 0.100]  # 10,20,30,40,100 ms
    out = latency_stats(durations_s)
    assert out["latency_mean_ms"] == 40.0
    assert out["latency_p50_ms"] == 30.0
    assert out["latency_p99_ms"] >= out["latency_p95_ms"] >= out["latency_p50_ms"]
