import sys
import time
# pyrefly: ignore [missing-import]
from valoricore.remote import SyncRemoteClient

def test_remote_graph():
    print("Connecting to Valori Node server...")
    try:
        client = SyncRemoteClient("http://127.0.0.1:3000")

        # Create records to act as chunks
        print("Inserting records...")
        rid1 = client.insert([0.1]*16)
        rid2 = client.insert([0.2]*16)
        print(f"Record IDs: {rid1}, {rid2}")

        # Create nodes
        print("Creating graph nodes...")
        doc_node = client.create_node(kind=1) # NODE_DOCUMENT = 1
        chunk_node1 = client.create_node(kind=2, record_id=rid1) # NODE_CHUNK = 2
        chunk_node2 = client.create_node(kind=2, record_id=rid2)

        print(f"Created nodes: Doc={doc_node}, Chunk1={chunk_node1}, Chunk2={chunk_node2}")

        # Create edges
        print("Creating edges...")
        e1 = client.create_edge(from_id=doc_node, to_id=chunk_node1, kind=10) # EDGE_PARENT_OF = 10
        e2 = client.create_edge(from_id=doc_node, to_id=chunk_node2, kind=10)
        print(f"Created edges: {e1}, {e2}")

        # Test get_node
        print("Testing get_node...")
        n_data = client.get_node(chunk_node1)
        print(f"Node {chunk_node1} data: {n_data}")
        assert n_data["record_id"] == rid1

        # Test get_edges
        print("Testing get_edges...")
        edges = client.get_edges(doc_node)
        print(f"Edges for doc {doc_node}: {edges}")
        assert len(edges) == 2

        # Test expand
        print("Testing expand...")
        records = client.expand(doc_node)
        print(f"Expanded records from doc {doc_node}: {records}")
        assert set(records) == {rid1, rid2}

        print("SUCCESS! Remote Graph API works perfectly.")

        # Test get_state_hash
        print("Testing get_state_hash...")
        state_hash = client.get_state_hash()
        print(f"State hash: {state_hash}")
        assert isinstance(state_hash, str) and len(state_hash) == 64

        print("SUCCESS! Remote Client fully tested.")
    except Exception as e:
        print(f"Test failed: {e}")
        sys.exit(1)

if __name__ == "__main__":
    test_remote_graph()
