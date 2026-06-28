# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
import asyncio
from fastapi import FastAPI, HTTPException
from typing import List, Dict, Any
from valoricore import AsyncValoricore, AsyncMemoryClient, ValidationError, IntegrityError

app = FastAPI(title="Valoricore Async API")

# Initialize clients (Local or Remote)
# We use a global variable or a dependency for production
client = AsyncMemoryClient()

def dummy_embedder(text: str) -> List[float]:
    """In production, this would call OpenAI or SentenceTransformers."""
    return [0.1] * 384

@app.on_event("startup")
async def startup():
    print("🛡️ Valoricore Async Engine Started")

@app.get("/search")
async def search(q: str, k: int = 5):
    """
    Non-blocking semantic search. 
    Even if 1,000 users hit this at once, the event loop stays free.
    """
    try:
        results = await client.semantic_search(q, embed=dummy_embedder, k=k)
        return {"query": q, "hits": results}
    except ValidationError as e:
        raise HTTPException(status_code=400, detail=str(e))
    except Exception as e:
        raise HTTPException(status_code=500, detail="Internal Search Error")

@app.post("/ingest")
async def ingest(text: str, title: str):
    """Async ingestion with deterministic proof generation."""
    try:
        result = await client.add_document(text, embed=dummy_embedder, title=title)
        return {
            "status": "ingested",
            "document_id": result["document_node_id"],
            "proof_count": len(result["proof_hashes"])
        }
    except IntegrityError:
        raise HTTPException(status_code=400, detail="Cryptographic verification failed")

@app.get("/health")
async def health():
    # Example using the low-level async factory
    # Note: Local mode is synchronous FFI, but AsyncValoricore makes it safe to use.
    local_db = AsyncValoricore()
    return {
        "status": "ready",
        "record_count": local_db.record_count(),
        "state_hash": local_db.get_state_hash()
    }

if __name__ == "__main__":
    import uvicorn
    print("🚀 Running FastAPI Demo at http://127.0.0.1:8000")
    print("📖 API Docs: http://127.0.0.1:8000/docs")
    uvicorn.run(app, host="127.0.0.1", port=8000)
