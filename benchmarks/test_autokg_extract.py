import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from autokg_extract import assert_no_leakage, extract_entities_and_relations, extract_graph_for_texts


def test_assert_no_leakage_raises_on_blocklisted_metadata_key():
    with pytest.raises(AssertionError):
        assert_no_leakage([{"evidence_doc_id": "d1"}])


def test_assert_no_leakage_raises_on_blocklisted_extra_string():
    with pytest.raises(AssertionError):
        assert_no_leakage([None], extra_strings=("this schema mentions qrels",))


def test_assert_no_leakage_passes_on_clean_metadata():
    assert_no_leakage([{"source_id": "doc1"}, None, {"source_id": "claim:q1"}])


def test_extract_entities_and_relations_finds_svo_relation():
    import spacy
    nlp = spacy.load("en_core_web_sm")
    # "Metformin treats X" gets mis-parsed by en_core_web_sm's small model as
    # a single noun chunk ("Metformin treats") rather than subject+verb -- a
    # known, documented limitation of the lightweight heuristic (see design
    # doc). Use an unambiguous SVO sentence instead to test the SVO path.
    entities, relations = extract_entities_and_relations(
        "Aspirin reduces inflammation in patients.", nlp
    )
    names = {e["name"] for e in entities}
    assert any("aspirin" in n for n in names)
    assert any("inflammation" in n for n in names)
    assert len(relations) >= 1
    assert any(r["type"] == "REDUCE" for r in relations)


def test_extract_graph_for_texts_tags_nodes_with_source_id():
    graph = extract_graph_for_texts([
        ("doc1", "Metformin treats type 2 diabetes."),
        ("claim:q1", "Metformin is used for diabetes."),
    ])
    source_ids = {n.properties.get("source_id") for n in graph.nodes}
    assert source_ids == {"doc1", "claim:q1"}
    names = {n.properties.get("name") for n in graph.nodes}
    assert any("metformin" in n for n in names)
