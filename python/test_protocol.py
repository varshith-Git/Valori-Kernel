# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
import unittest
import os
from valori.protocol import ProtocolClient
from valori import ProtocolClient as PublicProtocolClient # Verify export

# Re-use dummy embed from test_memory or define locally
def dummy_embed(text: str) -> list[float]:
    """
    Deterministic embedding function.
    """
    import hashlib
    hash_val = hashlib.sha256(text.encode()).hexdigest()
    res = []
    for i in range(16):
        byte_val = int(hash_val[i*2:(i+1)*2], 16)
        res.append((byte_val / 255.0) * 2 - 1)
    return res

@pytest.fixture
def protocol_client():
    # Uses local FFI by default (remote=None)
    # If FFI missing, we might need mocking similar to test_memory.py?
    # But now we know FFI is installed.
    return ProtocolClient(embed=dummy_embed, remote=None)

def test_protocol_export():
    assert PublicProtocolClient is not None

def test_upsert_text_basic(protocol_client):
    text = "Hello protocol world."
    res = protocol_client.upsert_text(text, doc_id="my-doc-1")
    
    assert res["chunk_count"] > 0
    assert len(res["memory_ids"]) == res["chunk_count"]
    assert res["memory_ids"][0].startswith("rec:")
    
    # Search text
    hits = protocol_client.search_text("Hello", k=1)
    assert len(hits["results"]) == 1
    assert hits["results"][0]["memory_id"] in res["memory_ids"]

def test_upsert_vector_explicit(protocol_client):
    vec = [0.5] * 16 # D=16
    
    res = protocol_client.upsert_vector(vec)
    
    assert res["memory_id"].startswith("rec:")
    assert res["record_id"] >= 0
    assert res["chunk_node_id"] > 0
    assert res["document_node_id"] >= 0
    
    # Search vector
    hits = protocol_client.search_vector(vec, k=1)
    assert len(hits["results"]) >= 1
    
    best = hits["results"][0]
    
    # Check if we found our record
    found = False
    for h in hits["results"]:
        if h["memory_id"] == res["memory_id"]:
            found = True
            break
    assert found

def test_dim_mismatch(protocol_client):
    bad_vec = [0.0] * 3
    with pytest.raises(ValueError):
        protocol_client.upsert_vector(bad_vec)
        
    with pytest.raises(ValueError):
        protocol_client.search_vector(bad_vec)
