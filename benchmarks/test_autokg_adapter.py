import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from neo4j_graphrag.components.types import Neo4jGraph, Neo4jNode, Neo4jRelationship
from autokg_adapter import (
    collapse_co_mentions, construction_stats, build_provenance,
    evaluate_edge_quality, compute_recovery,
)


def _graph():
    return Neo4jGraph(
        nodes=[
            Neo4jNode(id="doc1:e0", label="Concept", properties={"name": "metformin", "source_id": "doc1"}),
            Neo4jNode(id="doc1:e1", label="Concept", properties={"name": "unrelated thing", "source_id": "doc1"}),
            Neo4jNode(id="claim:q1:e0", label="Concept", properties={"name": "metformin", "source_id": "claim:q1"}),
            Neo4jNode(id="doc2:e0", label="Concept", properties={"name": "aspirin", "source_id": "doc2"}),
        ],
        relationships=[
            Neo4jRelationship(start_node_id="doc1:e0", end_node_id="doc1:e1", type="CO_OCCURS_WITH"),
        ],
    )


def test_collapse_co_mentions_bridges_shared_entity_claim_to_doc_only():
    edges = collapse_co_mentions(_graph())
    assert edges == [("claim:q1", "doc1")]  # doc2 (aspirin) shares nothing; doc1<->doc1 excluded (not claim<->doc)


def test_construction_stats_counts_nodes_edges_and_isolated():
    stats = construction_stats(_graph(), [("claim:q1", "doc1")], extraction_duration_s=1.0, graph_build_duration_s=0.5)
    assert stats["nodes_generated"] == 4
    assert stats["edges_generated"] == 1
    assert stats["isolated_nodes"] == 2  # claim:q1:e0 and doc2:e0 have no intra-chunk relationship
    assert stats["co_mention_bridge_edges"] == 1
    assert stats["model_api_usage"] == "spacy/en_core_web_sm (local, offline)"
    assert stats["estimated_cost_usd"] == 0


def test_build_provenance_includes_co_mention_and_intra_chunk_edges():
    prov = build_provenance(_graph(), [("claim:q1", "doc1")])
    types = {p["relationship_type"] for p in prov}
    assert "co_mentions_entity" in types
    assert "CO_OCCURS_WITH" in types
    for p in prov:
        assert p["confidence"] is None


def test_evaluate_edge_quality_precision_recall_f1():
    qrels = {"q1": {"doc1": 1, "doc9": 1}, "q2": {"doc5": 1}}  # 3 gold claim->evidence edges total
    edges = [("claim:q1", "doc1"), ("claim:q1", "doc_wrong")]
    result = evaluate_edge_quality(edges, qrels)
    assert result["true_positive_edges"] == 1
    assert result["predicted_edges"] == 2
    assert result["gold_edges"] == 3
    assert result["edge_precision"] == 0.5
    assert result["edge_recall"] == round(1 / 3, 4)


def test_compute_recovery_normal_case():
    r = compute_recovery(a_value=0.72, b_value=0.88, c_value=0.82)
    assert r["oracle_gain"] == round(0.16, 4)
    assert r["auto_gain"] == round(0.10, 4)
    assert r["recovery"] == round(0.10 / 0.16, 4)


def test_compute_recovery_na_when_oracle_gain_not_positive():
    r = compute_recovery(a_value=0.90, b_value=0.90, c_value=0.85)
    assert r["recovery"] == "N/A"
    r2 = compute_recovery(a_value=0.90, b_value=0.80, c_value=0.85)
    assert r2["recovery"] == "N/A"


def test_compute_recovery_reports_negative_recovery_as_is():
    r = compute_recovery(a_value=0.72, b_value=0.88, c_value=0.60)
    assert r["auto_gain"] < 0
    assert r["recovery"] < 0
