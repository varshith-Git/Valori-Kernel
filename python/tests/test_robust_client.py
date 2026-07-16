import asyncio
import pytest
import shutil
import os
import sys
from typing import List

# Setup path
sys.path.append(os.path.join(os.path.dirname(__file__), ".."))

from valoricore import AsyncMemoryClient, AsyncValoricore, ValidationError, IntegrityError

def dummy_embedder(text: str) -> List[float]:
    return [0.1] * 384

@pytest.mark.asyncio
async def test_concurrency_stress_local():
    """
    Spawns 100 concurrent tasks inserting documents into a LOCAL engine.
    This verifies the threading.Lock + asyncio.to_thread shielding works.
    """
    db_path = "./test_async_stress_db"
    if os.path.exists(db_path):
        shutil.rmtree(db_path)
        
    client = AsyncMemoryClient(dim=384)  # dim must match dummy_embedder
    
    tasks = []
    for i in range(100):
        tasks.append(client.add_document(
            text=f"Task {i} document content",
            embed=dummy_embedder,
            title=f"Doc {i}"
        ))
        
    print(f"\n🚀 Spawning 100 concurrent ingestion tasks...")
    results = await asyncio.gather(*tasks)
    
    assert len(results) == 100
    print(f"✅ Successfully ingested 100 docs concurrently.")
    
    # Verify we can still search
    hits = await client.semantic_search("Task 50", embed=dummy_embedder, k=1)
    assert len(hits) == 1
    print(f"✅ Search verified post-concurrency.")
    
    if os.path.exists(db_path):
        shutil.rmtree(db_path)

@pytest.mark.asyncio
async def test_exception_handling():
    """Verifies custom exceptions like ValidationError are raised."""
    client = AsyncMemoryClient(dim=384)

    # Test invalid dimension
    invalid_vec = [0.1] * 10  # Wrong dim (384 expected)
    
    with pytest.raises(ValidationError) as excinfo:
        await client.upsert_vector(vector=invalid_vec)
    
    assert "dimension" in str(excinfo.value)
    print(f"✅ ValidationError caught correctly: {excinfo.value}")

if __name__ == "__main__":
    # If run directly without pytest
    asyncio.run(test_concurrency_stress_local())
    asyncio.run(test_exception_handling())
