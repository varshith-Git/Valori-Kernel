"""Non-LLM entity/relation extraction for Phase B1.2 arm C.

Subclasses neo4j_graphrag's EntityRelationExtractor interface with a spaCy
noun-chunk + subject-verb-object heuristic -- no LLM, no credential, fully
offline and deterministic. See docs/superpowers/specs/2026-09-16-phase-b1.2-
auto-kg-design.md for why (en_core_web_sm's NER labels don't fit biomedical
text; noun chunks do a better job of capturing domain phrases).
"""
from __future__ import annotations

import asyncio
import re

import spacy
from neo4j_graphrag.components.entity_relation_extractor import EntityRelationExtractor, OnError
from neo4j_graphrag.components.types import Neo4jGraph, Neo4jNode, Neo4jRelationship, TextChunk, TextChunks

LEAKAGE_BLOCKLIST = (
    "qrel", "evidence", "rationale", "support", "contradict", "oracle", "sufficient_label",
)

_NLP = None


def get_nlp():
    global _NLP
    if _NLP is None:
        _NLP = spacy.load("en_core_web_sm")
    return _NLP


def assert_no_leakage(chunk_metadatas, extra_strings=()):
    """Fail loud if any oracle/qrel-shaped data could reach extraction."""
    haystacks = []
    for m in chunk_metadatas:
        if m:
            haystacks.extend(str(k).lower() for k in m.keys())
            haystacks.extend(str(v).lower() for v in m.values())
    haystacks.extend(s.lower() for s in extra_strings)
    for h in haystacks:
        for bad in LEAKAGE_BLOCKLIST:
            assert bad not in h, f"leakage guard: forbidden token {bad!r} found in {h!r}"


def _normalize(text):
    return re.sub(r"\s+", " ", text.strip().lower())


def _is_kept_chunk(nc):
    norm = _normalize(nc.text)
    if len(norm) < 3:
        return False
    return not all(t.is_stop or t.is_punct or t.pos_ == "PRON" for t in nc)


def extract_entities_and_relations(text, nlp):
    """Returns (entities, relations) local to this one text/chunk."""
    doc = nlp(text)
    entities_by_name = {}
    ordered_entities = []
    token_to_entity = {}
    for nc in doc.noun_chunks:
        if not _is_kept_chunk(nc):
            continue
        norm = _normalize(nc.text)
        if norm not in entities_by_name:
            eid = f"e{len(entities_by_name)}"
            entities_by_name[norm] = eid
            ordered_entities.append({"id": eid, "name": norm, "raw_text": nc.text})
        eid = entities_by_name[norm]
        for tok in nc:
            token_to_entity[tok.i] = eid

    relations = []
    seen_pairs = set()
    for sent in doc.sents:
        sent_entities_in_order, seen_in_sent = [], set()
        for tok in sent:
            eid = token_to_entity.get(tok.i)
            if eid is not None and eid not in seen_in_sent:
                sent_entities_in_order.append(eid)
                seen_in_sent.add(eid)
        svo_found = False
        for tok in sent:
            if tok.dep_ in ("nsubj", "nsubjpass") and tok.head.pos_ == "VERB":
                subj_eid = token_to_entity.get(tok.i)
                if subj_eid is None:
                    continue
                for child in tok.head.children:
                    if child.dep_ in ("dobj", "pobj", "attr"):
                        obj_eid = token_to_entity.get(child.i)
                        if obj_eid is not None and obj_eid != subj_eid:
                            pair = (subj_eid, obj_eid, tok.head.lemma_.upper())
                            if pair not in seen_pairs:
                                seen_pairs.add(pair)
                                relations.append({"start_id": subj_eid, "end_id": obj_eid, "type": pair[2]})
                                svo_found = True
        if not svo_found and len(sent_entities_in_order) >= 2:
            a, b = sent_entities_in_order[0], sent_entities_in_order[1]
            pair = (a, b, "CO_OCCURS_WITH")
            if pair not in seen_pairs:
                seen_pairs.add(pair)
                relations.append({"start_id": a, "end_id": b, "type": "CO_OCCURS_WITH"})
    return ordered_entities, relations


class SpacyEntityRelationExtractor(EntityRelationExtractor):
    def __init__(self):
        super().__init__(on_error=OnError.IGNORE, create_lexical_graph=False)

    async def run(self, chunks: TextChunks, document_info=None, lexical_graph_config=None, **kwargs) -> Neo4jGraph:
        assert_no_leakage([c.metadata for c in chunks.chunks])
        nlp = get_nlp()
        nodes, rels = [], []
        for chunk in chunks.chunks:
            source_id = (chunk.metadata or {}).get("source_id", chunk.chunk_id)
            entities, relations = extract_entities_and_relations(chunk.text, nlp)
            id_map = {}
            for ent in entities:
                node_id = f"{source_id}:{ent['id']}"
                id_map[ent["id"]] = node_id
                nodes.append(Neo4jNode(
                    id=node_id, label="Concept",
                    properties={"name": ent["name"], "raw_text": ent["raw_text"], "source_id": source_id},
                ))
            for rel in relations:
                rels.append(Neo4jRelationship(
                    start_node_id=id_map[rel["start_id"]],
                    end_node_id=id_map[rel["end_id"]],
                    type=rel["type"],
                ))
        return Neo4jGraph(nodes=nodes, relationships=rels)


def extract_graph_for_texts(id_text_pairs):
    """id_text_pairs: list[(source_id, text)]. Top-level entry point."""
    chunks = TextChunks(chunks=[
        TextChunk(text=text, index=i, metadata={"source_id": sid})
        for i, (sid, text) in enumerate(id_text_pairs)
    ])
    assert_no_leakage([c.metadata for c in chunks.chunks])
    extractor = SpacyEntityRelationExtractor()
    return asyncio.run(extractor.run(chunks))
