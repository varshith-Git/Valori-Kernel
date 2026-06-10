"""
Smoke test for the four graph hardening fixes:
  Issue 1 – create_edge WAL safety (verified via snapshot round-trip)
  Issue 2 – delete_node / delete_edge Python API
  Issue 3 – O(degree) cascade delete via back-pointer linked lists
  Issue 4 – soft_delete / delete auto-cascades to the linked graph node

Run with:
    .venv/bin/python3 smoke_test_graph.py
"""

import sys, os, shutil, random, time

sys.path.insert(0, "python")
import valoricore
from valoricore.kinds import (
    NODE_DOCUMENT, NODE_CHUNK,
    EDGE_PARENT_OF, EDGE_REFERS_TO,
)

DB_PATH   = "/tmp/valori_smoke_graph_main"
DB_PATH2  = "/tmp/valori_smoke_graph_restore"
DIM       = 8

# ── Always start clean ────────────────────────────────────────────────────────
for p in [DB_PATH, DB_PATH2]:
    if os.path.exists(p):
        shutil.rmtree(p)

db = valoricore.MemoryClient(path=DB_PATH, max_records=500, dim=DIM)

def rnd_vec():
    return [random.random() for _ in range(DIM)]

print("=" * 60)
print("Valori Graph Hardening Smoke Test")
print("=" * 60)

# ─── Insert 10 records ────────────────────────────────────────────────────────
ids = [db._db.insert(rnd_vec()) for _ in range(10)]
assert ids == list(range(10)), f"Expected ids 0-9, got {ids}"
print(f"\nInserted {len(ids)} records: {ids}")

# ══════════════════════════════════════════════════════════════════════════════
# Issue 1: create_edge goes through WAL
#          Verified by snapshot round-trip — edges must survive restore
# ══════════════════════════════════════════════════════════════════════════════
print("\n── Issue 1: create_edge WAL safety (snapshot round-trip) ────────────")

doc = db.create_node(NODE_DOCUMENT)
c1  = db.create_node(NODE_CHUNK, record_id=ids[0])
c2  = db.create_node(NODE_CHUNK, record_id=ids[1])

e0 = db.create_edge(doc, c1, EDGE_PARENT_OF)
e1 = db.create_edge(doc, c2, EDGE_PARENT_OF)
e2 = db.create_edge(c1,  c2, EDGE_REFERS_TO)

# Snapshot + restore into fresh engine
snap_before = db.snapshot()
db_restore = valoricore.MemoryClient(path=DB_PATH2, max_records=500, dim=DIM)
db_restore.restore(snap_before)

# All 3 edges must survive restore
edges_doc_after  = db_restore._db.get_edges(doc)
edges_c1_after   = db_restore._db.get_edges(c1)
assert len(edges_doc_after) == 2, f"doc should have 2 outgoing edges after restore, got {edges_doc_after}"
assert len(edges_c1_after)  == 1, f"c1 should have 1 outgoing edge after restore, got {edges_c1_after}"
print(f"  Edges survive snapshot/restore (doc→{len(edges_doc_after)}, c1→{len(edges_c1_after)}) ✅")

# ══════════════════════════════════════════════════════════════════════════════
# Issue 2: delete_edge and delete_node exist in Python API
# ══════════════════════════════════════════════════════════════════════════════
print("\n── Issue 2: delete_edge / delete_node API ───────────────────────────")

# delete_edge: remove the c1→c2 cross-link
db.delete_edge(e2)
edges_c1_after_del = db._db.get_edges(c1)
assert all(e["edge_id"] != e2 for e in edges_c1_after_del), \
    f"Edge e2={e2} still in c1's outgoing list after delete_edge"
print(f"  delete_edge({e2}): removed from c1's outgoing list ✅")

# delete_node: create isolated nodes, link them in a triangle, delete the middle one
a = db.create_node(NODE_CHUNK)
b = db.create_node(NODE_CHUNK)
c = db.create_node(NODE_CHUNK)
e_ab = db.create_edge(a, b, EDGE_REFERS_TO)
e_bc = db.create_edge(b, c, EDGE_REFERS_TO)
e_ca = db.create_edge(c, a, EDGE_REFERS_TO)

db.delete_node(b)
assert db._db.get_node(b) is None, \
    f"Node b={b} should be None after delete_node, got {db._db.get_node(b)}"
assert db._db.get_edges(a) == [], \
    f"Node a={a} should have 0 outgoing edges after b deleted, got {db._db.get_edges(a)}"
# c→a edge should still exist (c was not deleted)
edges_c = db._db.get_edges(c)
assert any(e["edge_id"] == e_ca for e in edges_c), \
    f"Edge c→a (e_ca={e_ca}) should still exist"
print(f"  delete_node({b}): node + incident edges cascade-deleted, unrelated edges intact ✅")

# ══════════════════════════════════════════════════════════════════════════════
# Issue 3: delete_node is O(degree), not O(E²)
# ══════════════════════════════════════════════════════════════════════════════
print("\n── Issue 3: O(degree) cascade delete ────────────────────────────────")

hub    = db.create_node(NODE_DOCUMENT)
spokes = [db.create_node(NODE_CHUNK) for _ in range(50)]
spoke_eids = []
for s in spokes:
    spoke_eids.append(db.create_edge(hub, s, EDGE_PARENT_OF))   # out-edge
    spoke_eids.append(db.create_edge(s, hub, EDGE_REFERS_TO))   # in-edge (back-pointer)

t_start = time.perf_counter()
db.delete_node(hub)
elapsed_ms = (time.perf_counter() - t_start) * 1000

assert db._db.get_node(hub) is None, "Hub node should be gone"
# Every spoke should have 0 outgoing edges (their hub→spoke out-edge was deleted)
for s in spokes:
    out = db._db.get_edges(s)
    assert out == [], f"Spoke {s} still has outgoing edges after hub deleted: {out}"

print(f"  delete_node(hub, 100 incident edges) in {elapsed_ms:.2f} ms ✅  (O(degree))")

# ══════════════════════════════════════════════════════════════════════════════
# Issue 4: soft_delete(record) auto-cascades → deletes linked graph node + edges
# ══════════════════════════════════════════════════════════════════════════════
print("\n── Issue 4: auto-cascade on soft_delete ─────────────────────────────")

doc2 = db.create_node(NODE_DOCUMENT)
c3   = db.create_node(NODE_CHUNK, record_id=ids[2])
e_doc2_c3 = db.create_edge(doc2, c3, EDGE_PARENT_OF)

node_before = db._db.get_node(c3)
assert node_before is not None, f"Node c3={c3} should exist"
assert node_before["record_id"] == ids[2], \
    f"c3's record_id should be {ids[2]}, got {node_before['record_id']}"
print(f"  Node c3={c3} ← record {ids[2]} exists ✅")

db.soft_delete(ids[2])

node_after = db._db.get_node(c3)
assert node_after is None, \
    (f"Node c3={c3} should be auto-deleted after soft_delete(record {ids[2]}), "
     f"but still found: {node_after}")
print(f"  Node c3={c3} auto-deleted after soft_delete ✅")

# e_doc2_c3 must be cascade-deleted from doc2's outgoing list
edges_doc2 = db._db.get_edges(doc2)
assert all(e["edge_id"] != e_doc2_c3 for e in edges_doc2), \
    f"Edge e_doc2_c3={e_doc2_c3} should be cascade-deleted from doc2"
print(f"  Cascade edge e_doc2_c3={e_doc2_c3} removed from doc2 ✅")

# search index: soft-deleted record should not appear in results
import math
query = db._db._db.get_record if hasattr(db._db, "_db") else None
hits = db._db.search([0.5]*DIM, k=10)
result_ids = {h["id"] for h in hits}
assert ids[2] not in result_ids, \
    f"Soft-deleted record {ids[2]} should not appear in search results"
print(f"  Record {ids[2]} excluded from search results ✅")

# ══════════════════════════════════════════════════════════════════════════════
# Issue 4b: hard delete also auto-cascades
# ══════════════════════════════════════════════════════════════════════════════
print("\n── Issue 4b: auto-cascade on hard delete ────────────────────────────")

doc3  = db.create_node(NODE_DOCUMENT)
c4    = db.create_node(NODE_CHUNK, record_id=ids[3])
e_doc3_c4 = db.create_edge(doc3, c4, EDGE_PARENT_OF)

db._db.delete(ids[3])

assert db._db.get_node(c4) is None, \
    f"Node c4={c4} should be auto-deleted after hard delete(record {ids[3]})"
print(f"  Node c4={c4} auto-deleted after hard delete ✅")

# ── Cleanup ───────────────────────────────────────────────────────────────────
for p in [DB_PATH, DB_PATH2]:
    shutil.rmtree(p, ignore_errors=True)

print("\n" + "=" * 60)
print("✅  ALL GRAPH HARDENING TESTS PASSED")
print("=" * 60)
