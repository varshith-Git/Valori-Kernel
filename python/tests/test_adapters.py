# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
import pytest
from unittest.mock import MagicMock, patch
import numpy as np

from valoricore.adapters.base import ValoricoreAdapter
from valoricore.adapters.langchain import ValoricoreRetriever
from valoricore.adapters.llamaindex import ValoricoreVectorStore as LlamaValoricoreVectorStore
from valoricore.adapters.utils import validate_float_range

# Mock Embedder
def mock_embed(text: str):
    # Deterministic mock
    np.random.seed(len(text)) 
    # Return 16-D float
    return np.random.uniform(-1, 1, 16).tolist()

class MockProtocolClient:
    def __init__(self, *args, **kwargs):
        self.upsert_calls = []
        self.search_calls = []
        
    def search_vector(self, vector, k=4):
        self.search_calls.append(vector)
        # Return mock hits
        return {
            "results": [
                {
                    "memory_id": "rec:1",
                    "record_id": 1, 
                    "score": 100, 
                    "metadata": {"text": "Retrieved Text", "doc_id": "doc1"}
                }
            ]
        }
        
    def upsert_vector(self, vector, metadata=None):
        self.upsert_calls.append((vector, metadata))
        return {"memory_id": "rec:1"}

@pytest.fixture
def mock_adapter():
    with patch("valoricore.adapters.base.ProtocolRemoteClient", side_effect=MockProtocolClient) as mock:
        adapter = ValoricoreAdapter(base_url="http://mock", api_key="test-key", embed_fn=mock_embed)
        yield adapter, mock

from valoricore.protocol import ValidationError

# ...

def test_validate_float_range():
    # Valid
    vec = [0.1, -0.5, 32767.0]
    assert validate_float_range(vec) == vec
    
    # Invalid: Out of bounds
    with pytest.raises(ValidationError, match="must be within"):
        validate_float_range([32768.0]) # Just above max
        
    with pytest.raises(ValidationError, match="must be within"):
        validate_float_range([-32769.0])
        
    # Invalid: NaN/Inf
    with pytest.raises(ValidationError, match="finite"):
        validate_float_range([float("nan")])

    with pytest.raises(ValidationError, match="finite"):
         validate_float_range([float("inf")])

def test_langchain_retriever(mock_adapter):
    adapter, _ = mock_adapter
    # Verify adapter client is our mock (due to side_effect init)
    assert isinstance(adapter.client, MockProtocolClient)
    
    retriever = ValoricoreRetriever(adapter, mock_embed)
    docs = retriever.get_relevant_documents("test query")
    
    assert len(docs) == 1
    assert docs[0].page_content == "Retrieved Text"
    assert docs[0].metadata["doc_id"] == "doc1"
    
def test_llamaindex_store_add(mock_adapter):
    adapter, _ = mock_adapter
    store = LlamaValoricoreVectorStore(adapter)
    
    # Mock LlamaIndex Node
    class MockNode:
        node_id = "node1"
        metadata = {"foo": "bar"}
        def get_embedding(self): return [0.1] * 16
        def get_content(self): return "Node Content"
        
    nodes = [MockNode()]
    ids = store.add(nodes)
    
    assert len(ids) == 1
    assert ids[0] == "node1"
    
    # Check calling info
    client = adapter.client
    assert len(client.upsert_calls) == 1
    vec, meta = client.upsert_calls[0]
    assert len(vec) == 16
    assert meta["text"] == "Node Content"
    assert meta["foo"] == "bar"
