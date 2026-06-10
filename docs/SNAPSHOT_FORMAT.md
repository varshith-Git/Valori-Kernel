# VAL1 Snapshot Format

**Version:** 1  
**Magic:** `VAL1` (bytes `56 41 4C 31`)  
**Crate:** `valori-node` — `node/src/engine.rs::Engine::snapshot()` / `::restore()`

---

## Overview

A snapshot is a point-in-time image of the entire engine state written as a
single flat byte string.  It is used for:

* **Fast restart** — load instead of replaying the full event log on boot.
* **Follower bootstrap** — a fresh follower downloads the leader's snapshot
  and then streams only subsequent events.
* **Manual backup / restore** via `POST /v1/snapshot/upload`.

The canonical truth on a running server is **always** the event log.  A
snapshot is a *cache*.  If both a snapshot and an event log are present at
startup, the event log wins (`Engine::try_recover()` priority 1).

---

## Wire Format

```
┌──────────────────────────────────────────────────────────┐
│  Magic        4 bytes   0x56 0x41 0x4C 0x31  ("VAL1")   │
├──────────────────────────────────────────────────────────┤
│  k_len        4 bytes   u32 little-endian                │
│  k_data       k_len bytes   kernel state blob            │
├──────────────────────────────────────────────────────────┤
│  m_len        4 bytes   u32 little-endian                │
│  m_data       m_len bytes   metadata JSON blob           │
├──────────────────────────────────────────────────────────┤
│  i_len        4 bytes   u32 little-endian                │
│  i_data       i_len bytes   index blob (may be 0 bytes)  │
└──────────────────────────────────────────────────────────┘
```

Minimum valid snapshot: **16 bytes** (magic + three zero-length sections with
no data).  The restore path validates this with an explicit size check before
accessing any byte.

---

## Section Descriptions

### Kernel state (`k_data`)

Serialized by `valori_kernel::snapshot::encode::encode_state()` and
deserialized by `::decode::decode_state()`.  Contains:

* Record arena: vectors (Q16.16 fixed-point), IDs, tags, soft-delete flags, metadata blobs.
* Graph node pool: node IDs, kinds, linked record references, edge adjacency lists.
* Graph edge pool: edge IDs, kinds, `from`/`to` node IDs.

The encoding is deterministic: the same kernel state always produces the same
bytes, which is what makes the BLAKE3 state hash reproducible across
architectures.

### Metadata (`m_data`)

JSON-encoded `HashMap<String, serde_json::Value>` from the node-layer
`MetadataStore`.  Keys follow the convention `"record_<id>"`.  May be empty
(`m_len = 0`).

### Index (`i_data`)

Vector search index serialized by the active `VectorIndex` implementation:

* **BruteForce** — empty blob (rebuilt from kernel state on restore).
* **HNSW** — proprietary layered graph bytes.
* **IVF** — centroid table + inverted lists.

If `i_len = 0` or the blob is absent, `restore()` calls `Engine::rebuild_index()`
to reconstruct the index from the kernel state.  This is always correct but
slower for HNSW/IVF.

---

## Restore algorithm (`engine.rs::restore()`)

```
1.  Check len ≥ 16, check magic == b"VAL1"
2.  Read k_len  (4 bytes); bounds-check k_data slice
3.  Read m_len  (4 bytes); bounds-check m_data slice
4.  Read i_len  (4 bytes); bounds-check i_data slice (optional — may be absent)
5.  Call restore_from_components(k_data, m_data, i_data):
      a. decode_state(k_data) → engine.state
      b. if m_data non-empty → MetadataStore::restore(m_data)
      c. if i_data Some && non-empty → index.restore(i_data)
         else → engine.rebuild_index()
      d. engine.rebuild_record_to_node()
```

Steps 2–4 return `EngineError::InvalidInput` on any truncation; the server
never panics on a malformed snapshot.

---

## HTTP Endpoints

| Method | Path                   | Description                         |
|--------|------------------------|-------------------------------------|
| GET    | `/v1/snapshot/download`| Download current snapshot as bytes  |
| POST   | `/v1/snapshot/upload`  | Upload bytes to restore engine      |
| POST   | `/v1/snapshot/save`    | Trigger save to configured path     |

---

## Versioning

The magic `VAL1` identifies format version 1.  Future incompatible changes
will use a different magic string (e.g. `VAL2`) so that restore() can detect
and reject mismatched snapshots with a clear error rather than corrupt state.

---

## Related

* [`docs/crash-recovery-proof.md`](crash-recovery-proof.md) — durability guarantees
* [`docs/wal-replay-guarantees.md`](wal-replay-guarantees.md) — event log recovery
* [`node/src/engine.rs`](../node/src/engine.rs) — `snapshot()`, `restore()`, `try_recover()`
* [`src/snapshot/`](../src/snapshot/) — `encode_state`, `decode_state`, BLAKE3
