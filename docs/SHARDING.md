# Sharding — Horizontal Scale Without Breaking the Proof

> **Status: DESIGN / ROADMAP — not implemented.** This document describes how
> Valori would scale *capacity and write throughput* horizontally, and how the
> single-provable-state-hash guarantee survives the transition. Today Valori
> only **replicates** (every node holds a full copy via Raft); it does not
> **shard** (split the data across nodes).

---

## 1. The distinction that matters

| | Replication (today) | Sharding (this doc) |
|---|---|---|
| Where the data lives | Every node holds **all** records | Each shard holds **1/N** of the records |
| What it buys | Fault tolerance, availability, read fan-out | **Capacity + write throughput** |
| What it does NOT buy | More capacity, faster writes | (on its own) fault tolerance |
| Total dataset size | Limited to **one node's** RAM/disk | **N nodes'** combined RAM/disk |
| Insert work | Done by **every** node | Done by **one** shard |

Replication and sharding are **orthogonal axes**. Production systems do both:
each shard is its own little Raft group (3–5 replicas for safety), and there
are many shards side by side (for scale). Valori has the replication axis built;
this doc is the sharding axis.

---

## 2. Critical clarification: what gets split

A vector `[0.1, 0.2, 0.3, 0.4]` is **one record** (one `RecordId`). Sharding
**never** splits the dimensions of a single vector across machines. The whole
vector always lives, complete, on exactly one shard.

What gets split is **the set of records** — which records live on which shard.

```
Insert R1=[0.1,0.2,0.3,0.4]  R2=[0.9,0.8,0.7,0.6]  R3=[0.5,0.5,0.5,0.5]  R4=[0.2,0.1,0.0,0.3]

Replication:                      Sharding (mod 2 on record id):
  every node: R1 R2 R3 R4           Shard A: R1  R3
                                    Shard B: R2  R4
  (each vector stays whole on its shard — never cut apart)
```

---

## 3. Routing — which shard owns a record

A **shard router** (a thin stateless layer in front of the cluster) maps each
record to a shard. Two practical strategies, not mutually exclusive:

1. **Hash-of-RecordId** — `shard = hash(record_id) % num_shards`. Even spread,
   simple, no hotspots. Best for one giant collection.
2. **By NamespaceId (collection)** — tenant/collection X → shard 1, Y → shard 2.
   Valori already has 16-bit `NamespaceId` (up to 1024 collections); this maps
   naturally onto shards and keeps a tenant's data co-located (cheap per-tenant
   search). Best for multi-tenant deployments.

> **Recommended first cut:** shard by `NamespaceId`. It reuses the existing
> namespace isolation seams (`apply_committed_event_ns`, WAL replay,
> `build_index`) and means a single-collection search never has to fan out.

### Write path (scales)

```
insert(vector, collection)
   → router: collection → Shard B
   → ONLY Shard B's Raft group commits the AutoInsertRecord
   → Shards A, C … do nothing
```

Each insert is handled by one shard → insert throughput grows ~linearly with
shard count.

---

## 4. Search — scatter-gather (the honest catch)

Vector similarity search is **not** a keyed lookup. A query vector's nearest
neighbour may sit on any shard, so the router must ask **every** shard and merge:

```
search(Q, k=5)
   ├──→ Shard A: local top-5 over its slice
   ├──→ Shard B: local top-5 over its slice
   └──→ Shard C: local top-5 over its slice
            │
       Coordinator: merge 3×5 candidates → global top-5
```

What this does and doesn't give you:

- ✅ **Capacity**: each shard indexes only 1/N of the vectors → the total
  dataset can far exceed one machine's RAM.
- ✅ **Lower per-search latency**: each shard searches a smaller pile, in
  parallel.
- ⚠️ **Not linear query-throughput scaling**: every search still touches every
  shard (unlike a key-value store, where one shard answers). Throughput
  improves sub-linearly.

**Exception:** if you shard by collection and the query names a collection, the
router sends the search to just that shard — no fan-out, full throughput
scaling. This is the strongest argument for namespace-based sharding.

---

## 5. The proof — keeping "one number proves everything"

This is the load-bearing problem. Valori's moat is a single BLAKE3 state hash
that proves the entire store is intact and identical. Today that works because
there is **one global event chain**:

```
e1 → e2 → e3 → e4 → … → ONE state hash
```

That single chain is also exactly what *prevents* scaling — it forces every
write through one global total order.

### Sharded proof: per-shard chains + Merkle root

Each shard keeps its **own independent BLAKE3 chain** over **its own events**,
in parallel, with no cross-shard coordination. The global proof is the hash of
the shard heads:

```
Shard A chain:  a1 → a2 → a3  ──→ head H_A
Shard B chain:  b1 → b2 → b3  ──→ head H_B
Shard C chain:  c1 → c2 → c3  ──→ head H_C

        MASTER_ROOT = BLAKE3( H_A ‖ H_B ‖ H_C )
                     /        |         \
                   H_A       H_B        H_C
```

(For many shards, arrange the heads as a binary Merkle tree so a single shard's
proof is `O(log N)` hashes, not `O(N)`.)

### Why the guarantee still holds

- Tamper with **any** record on Shard B → Shard B's chain replays to a
  **different** head `H_B'` → `MASTER_ROOT` changes → **detected**.
- Offline verification (the `valori-verify` model) becomes: for each shard,
  replay its `events.log` and confirm it reproduces `H_shard`; then confirm
  `MASTER_ROOT == BLAKE3(H_A ‖ H_B ‖ …)`. If all pass, the whole store is
  provably intact — the proof is simply **assembled from independent pieces**.

### The one real price

A single global chain gives a strict total order over **all** events — you can
prove "event X happened before event Y" for any two events anywhere. Sharding
gives strict order only **within** each shard. Cross-shard "which happened
first" between two unrelated inserts is no longer absolutely defined. For vector
search this almost never matters; it is the deliberate trade for escaping the
single-queue bottleneck.

---

## 6. Topology — sharding × replication together

Each shard is itself a Raft group (reuse everything in `valori-consensus`):

```
        Shard 1 (ns 0–340)     Shard 2 (ns 341–681)   Shard 3 (ns 682–1023)
        ┌────┬────┬────┐       ┌────┬────┬────┐        ┌────┬────┬────┐
        │ R  │ R  │ R  │       │ R  │ R  │ R  │        │ R  │ R  │ R  │
        └────┴────┴────┘       └────┴────┴────┘        └────┴────┴────┘
        own Raft + own chain   own Raft + own chain    own Raft + own chain
                     \               |               /
                          Shard Router (stateless)
                                   │
                       MASTER_ROOT = Merkle(H1, H2, H3)
```

- **Within a shard:** unchanged — Raft replication, identical `KernelState`,
  one `H_shard`, the existing determinism guarantees apply verbatim.
- **Across shards:** the router fans writes to the owning shard and scatter-
  gathers searches; a coordinator rolls the shard heads into `MASTER_ROOT`.

---

## 7. Implementation sketch (future phases)

1. **Shard router crate** (`valori-router`?) — stateless; owns the
   record→shard map; fans out search; merges top-k; exposes the same HTTP
   surface as a single node so clients/SDK are unchanged.
2. **Master-root coordinator** — periodically pulls each shard's
   `final_state_hash`, computes the Merkle root, publishes it as the new
   `/v1/proof/state` for the cluster.
3. **Verifier extension** — `valori-verify` learns to verify a *set* of
   `events.log` files plus a `MASTER_ROOT` manifest.
4. **Rebalancing** — moving a namespace between shards (snapshot ship +
   chain hand-off) when shards grow uneven. Hardest part; defer.
5. **SDK** — ideally transparent (router speaks the node API). Cross-shard
   consistency level (`local` vs `linearizable`) semantics need defining.

---

## 8. When you actually need this

You do **not** need sharding until one node can no longer hold your working set
in RAM, or single-node write throughput is the bottleneck. Until then,
replication (3–5 nodes) is the right answer: simpler, and it preserves the
single global chain — the strongest form of the proof. Sharding is the tool for
*scale*, not *safety*, and it costs you the global total order. Reach for it
only when capacity or write throughput — not availability — is the wall you hit.

---

## See also

- [`docs/CLUSTER.md`](CLUSTER.md) — current replication / Raft operations
- [`docs/CAPACITY.md`](CAPACITY.md) — single-node capacity limits (the wall that
  triggers sharding)
- [`docs/THREAT_MODEL.md`](THREAT_MODEL.md) — BLAKE3 chain security model the
  Merkle root extends
- [`docs/MULTINODE_ROADMAP.md`](MULTINODE_ROADMAP.md) — phase roadmap this would
  slot into
