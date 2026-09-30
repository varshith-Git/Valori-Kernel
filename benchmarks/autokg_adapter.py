"""Neo4jGraph -> Valori-comparable edges, provenance, and recovery math for
Phase B1.2. See docs/superpowers/specs/2026-09-16-phase-b1.2-auto-kg-design.md
for why co-mention collapse exists (bridges the extracted graph's natural
two-hop claim->entity->document shape into GraphRAG's fixed depth=1)."""
from __future__ import annotations

import spacy


def collapse_co_mentions(graph):
    """Returns sorted [(claim_id, doc_id), ...] wherever a claim chunk and a
    document chunk share a normalized entity name. Mirrors the shape of
    public_claim_edges() in live_local_db_comparison.py."""
    name_to_sources = {}
    for node in graph.nodes:
        name = node.properties.get("name")
        sid = node.properties.get("source_id")
        if name is None or sid is None:
            continue
        name_to_sources.setdefault(name, set()).add(sid)
    pairs = set()
    for sids in name_to_sources.values():
        claims = [s for s in sids if s.startswith("claim:")]
        docs = [s for s in sids if not s.startswith("claim:")]
        for c in claims:
            for d in docs:
                pairs.add((c, d))
    return sorted(pairs)


def construction_stats(graph, co_mention_edges, extraction_duration_s, graph_build_duration_s):
    rel_type_counts = {}
    connected_node_ids = set()
    for rel in graph.relationships:
        rel_type_counts[rel.type] = rel_type_counts.get(rel.type, 0) + 1
        connected_node_ids.add(rel.start_node_id)
        connected_node_ids.add(rel.end_node_id)
    isolated = sum(1 for n in graph.nodes if n.id not in connected_node_ids)
    unique_names = len({n.properties.get("name") for n in graph.nodes})
    return {
        "nodes_generated": len(graph.nodes),
        "edges_generated": len(graph.relationships),
        "relationship_types": rel_type_counts,
        "isolated_nodes": isolated,
        "unique_entities_after_dedup": unique_names,
        "duplicate_mentions_collapsed": len(graph.nodes) - unique_names,
        "co_mention_bridge_edges": len(co_mention_edges),
        "extraction_duration_s": round(extraction_duration_s, 4),
        "graph_build_duration_s": round(graph_build_duration_s, 4),
        "model_api_usage": "spacy/en_core_web_sm (local, offline)",
        "estimated_cost_usd": 0,
    }


def build_provenance(graph, co_mention_edges):
    version = f"spacy={spacy.__version__}"
    extractor_name = "benchmarks.autokg_extract.SpacyEntityRelationExtractor"
    records = []
    for a, b in co_mention_edges:
        records.append({
            "source_id": a, "target_id": b,
            "source_entity": None, "target_entity": None,
            "relationship_type": "co_mentions_entity",
            "extractor": extractor_name, "extractor_version": version, "confidence": None,
        })
    for rel in graph.relationships:
        records.append({
            "source_id": rel.start_node_id, "target_id": rel.end_node_id,
            "source_entity": rel.start_node_id, "target_entity": rel.end_node_id,
            "relationship_type": rel.type,
            "extractor": extractor_name, "extractor_version": version, "confidence": None,
        })
    return records


def evaluate_edge_quality(co_mention_edges, qrels):
    predicted = set(co_mention_edges)
    gold = {
        (f"claim:{qid}", doc_id)
        for qid, docmap in qrels.items()
        for doc_id, score in docmap.items()
        if score > 0
    }
    tp = len(predicted & gold)
    precision = tp / len(predicted) if predicted else 0.0
    recall = tp / len(gold) if gold else 0.0
    f1 = (2 * precision * recall / (precision + recall)) if (precision + recall) > 0 else 0.0
    return {
        "edge_precision": round(precision, 4), "edge_recall": round(recall, 4), "edge_f1": round(f1, 4),
        "predicted_edges": len(predicted), "gold_edges": len(gold), "true_positive_edges": tp,
    }


def compute_recovery(a_value, b_value, c_value):
    oracle_gain = b_value - a_value
    auto_gain = c_value - a_value
    recovery = round(auto_gain / oracle_gain, 4) if oracle_gain > 0 else "N/A"
    return {"auto_gain": round(auto_gain, 4), "oracle_gain": round(oracle_gain, 4), "recovery": recovery}
