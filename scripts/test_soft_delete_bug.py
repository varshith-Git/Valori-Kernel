import os
import valoricore
from valoricore import MemoryClient
from valoricore.embeddings import SentenceTransformerEmbedder
from valoricore.kinds import NODE_CONCEPT, EDGE_REFERS_TO

embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")

db_path = "./valori_test_db_debug"
client = MemoryClient(path=db_path)

vec_ai = embedder("Artificial Intelligence is evolving rapidly.")
vec_cpu = embedder("Intel CPUs process data sequentially.")

# Insert vectors to get their unique Record IDs
rid_ai, _ = client._db.insert_with_proof(vec_ai)
rid_cpu, _ = client._db.insert_with_proof(vec_cpu)

# Create nodes linked to those vectors
node_ai = client.create_node(kind=NODE_CONCEPT, record_id=rid_ai)
node_cpu = client.create_node(kind=NODE_CONCEPT, record_id=rid_cpu)

# Create an Edge!
edge_id = client.create_edge(from_id=node_ai, to_id=node_cpu, kind=EDGE_REFERS_TO)

print("Records before delete:", client.record_count())
client.soft_delete(0)
print("Records after soft delete:", client.record_count())

updated_vector = embedder("This is the V2 updated version of the text.")
new_rid, _ = client._db.insert_with_proof(updated_vector)
print("SUCCESS!")
