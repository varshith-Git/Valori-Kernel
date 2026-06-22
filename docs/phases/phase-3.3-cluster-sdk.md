# Phase 3.3 — Cluster-aware Python SDK

## Goal

Upgrade the Python SDK so application code requires zero cluster-specific logic:
writes find the leader automatically, local reads are spread across all replicas
for throughput, and every mutating call carries an idempotency key so a retry
after a connection failure is deduplicated server-side rather than applied twice.

## Delivered

### `python/valoricore/remote.py`

**`SyncRemoteClient._post()` — idempotency key threading**

New optional `idempotency_key: Optional[bytes]` parameter.  When supplied,
injects `"request_id": list(key)` into the JSON body *before* the retry loop so
the exact same 16-byte token is sent on every attempt.  The cluster server's
`/records` handler already accepted `request_id: Option<[u8; 16]>` and
returns `"deduplicated": true` when the key matches a previously applied write.

**`SyncRemoteClient.insert()` — auto-generated idempotency key**

Generates `uuid4().bytes` as the default idempotency key on every call.  Callers
can override with `idempotency_key=my_bytes` to control the token explicitly
(e.g., for application-level dedup across process restarts).

**`SyncRemoteClient.soft_delete()` / `delete()` — idempotency keys**

Same pattern — auto UUID4 by default, override accepted.

**`SyncRemoteClient.leader_url()` — new method**

Returns `self._leader_url` (the base URL of the leader learned from the last
307 redirect), or `None` on a fresh client or after a failover resets the cache.

**`SyncRemoteClient.get_cluster_role()` — new method**

`GET /v1/cluster/role` → `"leader"` or `"follower"`.  Raises `ConnectionError`
if the node is standalone (endpoint 404s).

**`AsyncRemoteClient.timeline()` — httpx consolidation**

Replaced the `aiohttp` dependency with the `httpx.AsyncClient` (`self.client`)
already used by the rest of `AsyncRemoteClient`.  Eliminates the mixed-client
inconsistency noted in the Phase 3.4 findings.

**`AsyncRemoteClient.leader_url()` / `get_cluster_role()` — new async methods**

Mirrors the sync additions.

**`ClusterClient` — new class**

Wraps a list of node URLs, each backed by a `SyncRemoteClient`.

| Concern | Strategy |
|---|---|
| Write routing | `_write_client()` picks the client whose `_leader_url` is set (i.e., has seen a redirect). Falls back to `_clients[0]` so the first write self-heals via the 307. |
| Local reads | `_read_client("local")` round-robins across all nodes with a simple counter. |
| Linearizable reads | `_read_client("linearizable")` delegates to `_write_client()`. |
| Leader failover | Handled transparently by the underlying `SyncRemoteClient` retry logic; `ClusterClient` picks up the newly cached leader on the next call. |

Methods exposed: `insert`, `insert_batch`, `delete`, `soft_delete`,
`create_collection`, `list_collections`, `drop_collection`, `search`,
`get_state_hash`, `timeline`, `snapshot`, `restore`, `cluster_status`,
`cluster_health`, `get_cluster_role`, `leader_url`.

**`AsyncClusterClient` — new class**

Async mirror of `ClusterClient`, backed by `AsyncRemoteClient` instances.
`cluster_health()` fans out to all nodes with `asyncio.gather`.
`close()` closes all underlying httpx clients.

### `python/valoricore/__init__.py`

Added `ClusterClient` and `AsyncClusterClient` to the package import and `__all__`.

## Findings

1. **Batch insert has no per-vector `request_id` on the cluster server** —
   `BatchInsertRequest` in `cluster_server.rs` issues one Raft entry per vector
   with `request_id: None`.  Per-batch idempotency would require either (a) a
   single batch entry in the Raft log (larger change to the kernel) or (b)
   N derived keys per vector.  Deferred; batch idempotency is low-priority
   because the primary concern is single-record writes from agents.

2. **Standalone nodes silently ignore `request_id`** — serde deserializes
   `InsertRecordRequest` and ignores unknown fields.  No server-side change needed;
   the SDK sends the field unconditionally and it is simply dropped on standalone.

3. **`_leader_url` is not shared across `ClusterClient._clients`** — each
   `SyncRemoteClient` in the pool learns the leader independently via its own 307
   redirect.  Once one client sees it the `_write_client()` picker immediately
   routes to that client.  All three nodes typically converge within one election
   round-trip.

## Validation

```
python3 -c "
from python.valoricore.remote import ClusterClient, SyncRemoteClient
c = ClusterClient(['http://a:3000', 'http://b:3000', 'http://c:3000'])
assert c.leader_url() is None
r0 = c._read_client().base_url
r1 = c._read_client().base_url
assert r0 != r1, 'round-robin must change node each call'
print('ClusterClient: ok')
"
```

```
cargo test -p valori-kernel -p valori-node
```

```
test result: ok.  (all suites pass, 0 failures, same counts as pre-phase)
```

Python syntax validated:
```
python3 -c "from python.valoricore.remote import SyncRemoteClient, AsyncRemoteClient, ClusterClient, AsyncClusterClient; print('imports ok')"
imports ok
```

## Follow-ups

- **Batch idempotency** — add `request_id: Option<[u8; 16]>` to `BatchInsertRequest`
  in `cluster_server.rs`; derive per-vector tokens from a batch-level UUID.
- **Leader-URL sharing across pool** — propagate a known `_leader_url` from any
  pool client to all others after discovery, eliminating parallel 307 hops.
- **`AsyncClusterClient` insert idempotency** — `AsyncRemoteClient.insert()`
  does not yet auto-generate an idempotency key; mirror the sync client pattern.
