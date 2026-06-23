import os
import time
import math
# pyrefly: ignore [missing-import]
from valoricore import MemoryClient

def mock_embed(text: str):
    """
    Deterministic mock embedding function returning exactly 384 floats.
    Used to bypass heavy ML model downloads in CI/CD environments.
    """
    base = sum(ord(c) for c in text)
    # Generate 384 deterministic floats between -1.0 and 1.0
    return [math.sin(base + i) for i in range(384)]

def print_header(title):
    print("\n" + "="*60)
    print(f"🚀 {title}")
    print("="*60)

def main():
    print_header("VALORI KERNEL: END-TO-END WORKFLOW TRACER")
    
    # 1. Initialize MemoryClient
    print("\n[STEP 1] Initialization")
    client = MemoryClient(path="./test_e2e_db", dim=384)
    print(f"✅ Client initialized.")
    print(f"🔐 Initial State Hash: {client.get_state_hash()}")
    
    # 2. Insert Documents
    print("\n[STEP 2] Vector & Node Insertion")
    doc1 = "Valori is an absolutely deterministic vector database."
    doc2 = "Knowledge graphs connect nodes with edges."
    
    print(f"📝 Inserting: '{doc1}'")
    rec1 = client.add_document(text=doc1, embed=mock_embed)
    print(f"✅ Stored as Node ID: {rec1['document_node_id']} (Record IDs: {rec1['record_ids']})")
    
    print(f"📝 Inserting: '{doc2}'")
    rec2 = client.add_document(text=doc2, embed=mock_embed)
    print(f"✅ Stored as Node ID: {rec2['document_node_id']} (Record IDs: {rec2['record_ids']})")
    
    # 3. Create Edge
    print("\n[STEP 3] Knowledge Graph Manipulation")
    print(f"🔗 Linking Node {rec1['document_node_id']} -> Node {rec2['document_node_id']}")
    edge_id = client.create_edge(
        from_id=rec1['document_node_id'], 
        to_id=rec2['document_node_id'], 
        kind=1 # Arbitrary 'RelatesTo' edge kind
    )
    print(f"✅ Graph Edge created successfully (Edge ID: {edge_id})")
    
    # 4. Search
    print("\n[STEP 4] Semantic Search")
    query = "Tell me about vectors"
    print(f"🔍 Searching for: '{query}'")
    
    hits = client.semantic_search(query, embed=mock_embed, k=2)
    
    print("✅ Search Results (L2 Distance):")
    for i, hit in enumerate(hits):
        print(f"   [{i+1}] Record ID: {hit['id']} | Distance Score: {hit['score']:.4f}")
        
    # 5. Check Determinism
    print("\n[STEP 5] Cryptographic Audit")
    final_hash = client.get_state_hash()
    print(f"🔐 Final State Hash (BLAKE3): {final_hash}")
    
    # 6. Timeline
    print("\n[STEP 6] Immutable Event Timeline")
    timeline = client.get_timeline()
    for event in timeline:
        print(f"   - {event}")

    print("\n" + "="*60)
    print("🎉 END TO END WORKFLOW COMPLETE")
    print("="*60 + "\n")

if __name__ == "__main__":
    main()
