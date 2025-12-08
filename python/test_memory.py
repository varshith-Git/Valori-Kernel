import pytest
import os
import sys
from unittest.mock import MagicMock
from valori.memory import MemoryClient
from valori.kinds import NODE_DOCUMENT, NODE_CHUNK
from valori.ingest import load_text_from_file

# --- MOCK INFRASTRUCTURE ---

class MockLocalClient:
    def __init__(self):
        self.record_counter = 0
        self.node_counter = 0
        self.edge_counter = 0
        self.records = {}
        self.nodes = {}
        self.edges = []

    def insert(self, vector):
        self.record_counter += 1
        rid = self.record_counter
        self.records[rid] = vector
        return rid

    def create_node(self, kind, record_id=None):
        self.node_counter += 1
        nid = self.node_counter
        self.nodes[nid] = {'kind': kind, 'record_id': record_id}
        return nid

    def create_edge(self, from_id, to_id, kind):
        self.edge_counter += 1
        eid = self.edge_counter
        self.edges.append({'id': eid, 'from': from_id, 'to': to_id, 'kind': kind})
        return eid

    def search(self, vector, k):
        # Fake search results: return existing records with dummy score
        hits = []
        for rid in list(self.records.keys())[:k]:
            hits.append((rid, 100)) # (id, score)
        return hits
        
    def snapshot(self):
        return b"mock_snapshot"
        
    def restore(self, data):
        pass

# --- HELPERS ---

def dummy_embed(text: str) -> list[float]:
    """
    Deterministic embedding function that maps text to a 16-dimensional vector.
    """
    import hashlib
    hash_val = hashlib.sha256(text.encode()).hexdigest()
    res = []
    for i in range(16):
        byte_val = int(hash_val[i*2:(i+1)*2], 16)
        res.append((byte_val / 255.0) * 2 - 1)
    return res

# --- FIXTURE ---

@pytest.fixture
def memory_client(monkeypatch):
    """
    Yields a MemoryClient.
    If 'valori_ffi' cannot be imported, patches Valori to use MockLocalClient.
    """
    use_mock = False
    try:
        import valori_ffi
    except ImportError:
        use_mock = True
        
    # User can force mock with env var
    if os.environ.get("VALORI_TEST_USE_MOCK"):
        use_mock = True

    if use_mock:
        print("\n[NOTE] Rust FFI not found or forced off. Running tests with MockLocalClient.")
        mock_client = MockLocalClient()
        def mock_factory(remote=None):
            if remote is None:
                return mock_client
            raise ValueError("Remote not mocked")
            
        monkeypatch.setattr("valori.memory.Valori", mock_factory)
        
    return MemoryClient(remote=None)

# --- TESTS ---

def test_add_document_basic(memory_client):
    # Use double newline to ensure splitting even in naive chunkers if that's what's used
    text = "Hello world.\n\nThis is a test."
    
    res = memory_client.add_document(text, embed=dummy_embed, title="Test Doc")
    
    assert res['document_node_id'] is not None
    assert isinstance(res['document_node_id'], int)
    
    # "Hello world." and "This is a test." -> Should be at least 1 or 2 chunks depending on chunker
    # split_by_sentences calls it 2 chunks.
    assert len(res['chunk_node_ids']) > 0
    assert len(res['record_ids']) == len(res['chunk_node_ids'])
    assert res['title'] == "Test Doc"

    # Verify search finds it
    hits = memory_client.semantic_search("Hello", embed=dummy_embed, k=1)
    assert len(hits) == 1
    # Check that the ID returned is known
    # Note: Hits return {id, score}, res returns record_ids list.
    assert hits[0]['id'] in res['record_ids']

def test_add_chunks_with_parent(memory_client):
    # 1. Create a parent document 
    parent_res = memory_client.add_document("Parent doc content.", embed=dummy_embed)
    parent_id = parent_res['document_node_id']
    
    # 2. Add extra chunks
    chunks = ["Extra chunk one.", "Extra chunk two."]
    res = memory_client.add_chunks(chunks, embed=dummy_embed, parent_document_node=parent_id)
    
    assert res['document_node_id'] == parent_id
    assert len(res['chunk_node_ids']) == 2
    
def test_ingest_text_file_roundtrip(memory_client, tmp_path):
    # Create temp file
    fpath = tmp_path / "test.txt"
    content = "Line 1.\nLine 2.\nLine 3."
    fpath.write_text(content, encoding='utf-8')
    
    # Load
    loaded_text = load_text_from_file(str(fpath))
    assert loaded_text == content
    
    # Ingest
    res = memory_client.add_document(loaded_text, embed=dummy_embed)
    # The splitting behavior depends on specific implementation of chunking
    # But for this short text with naive_paragraph_chunker, it should be 1 chunk.
    assert len(res['chunk_node_ids']) == 1

def test_chunking_semantics():
    from valori.chunking import split_by_sentences
    text = "Sentence one. Sentence two! Sentence three?"
    # Default max_chars is 512, but new logic splits strictly by sentence
    chunks = split_by_sentences(text)
    assert len(chunks) == 3
    assert "Sentence one." in chunks

    # Force hard-split with small max_chars
    chunks_small = split_by_sentences(text, max_chars=20)
    # The structure might vary depending on hard-split impl, but len should be >= 3
    assert len(chunks_small) >= 3
