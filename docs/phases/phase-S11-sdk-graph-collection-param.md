# Phase S11 — Python SDK: `collection` param on low-level graph node/edge methods

Branch: `Node-scaleup` (S1-S10 merged, `cc57f16`).

## Goal

Auditing SDK coverage of S1-S9's routing work (user asked directly: "does
the Python SDK support all these new features?") found one real gap:
`create_node()`, `get_node()`, `create_edge()`, `get_edges()`,
`subgraph()`, and `neighbors()` on both `SyncRemoteClient` and
`AsyncRemoteClient` had no `collection` parameter at all — unlike
`graphrag()`, `list_nodes()`, `insert()`, `search()`, `insert_encrypted()`,
and every other collection-aware SDK method. The server (standalone always,
cluster as of S8) fully supports `collection` on these endpoints; the SDK
simply never exposed it, so a Python caller could only reach the default
collection's graph data through these six methods.

## Delivered

Added `collection: str = "default"` to all six methods, on both clients:

- `create_node()` / `create_edge()` — added to the JSON body only when
  non-default (`if collection != "default": data["collection"] = collection`),
  matching the existing convention used by `insert()`/`search()`/etc.
- `get_node()` / `get_edges()` / `subgraph()` — GET requests, so the
  parameter goes through `requests`/`httpx`'s `params=` (matching the
  existing `list_nodes()` pattern: `params = {} if collection == "default"
  else {"collection": collection}`), not manual query-string concatenation.
- `neighbors()` — forwards its new `collection` param into the `get_edges()`
  call it wraps.

All additions are backward compatible: `collection` is the last parameter
with a `"default"` default, so every existing positional or keyword call
site continues to work unchanged (verified against
`python/tests/test_unified.py` and `python/tests/test_graphrag_sdk.py`,
the only two files calling these methods).

## Findings

- `ClusterClient`/`AsyncClusterClient` (the convenience wrappers that
  round-robin reads and auto-discover the leader across multiple node
  URLs) do not forward these six graph methods at all — this predates S11,
  is not a regression, and was not addressed here since it's a separate,
  pre-existing scope limit of that wrapper class rather than part of the
  "does the SDK support the new routing features" audit.
- `shred_key()`/`shred_key_status()` needed **no changes** — they return
  `resp.json()` unmodified, so S5's new fan-out response shape
  (`{"shredded": bool, "shards": {...}}`) already passes through
  automatically.

## Validation

```
python3 -c "import ast; ast.parse(open('python/valoricore/remote.py').read())"   # syntax OK
```

No live-node test run this pass (matches S7's precedent — a Python
syntax/static check, not exercised against a running node). Grepped
`python/tests/` for every call site of the six changed methods and
confirmed none pass a positional argument in the position `collection` now
occupies.

## Follow-ups

- `ClusterClient`/`AsyncClusterClient` still don't wrap the low-level graph
  methods (`create_node`/`get_node`/`create_edge`/`get_edges`/`subgraph`/
  `neighbors`) at all, collection-aware or not — worth adding if a caller
  needs cluster-aware read/leader-routing for graph data specifically.
- Not exercised against a live node/cluster in this pass.
