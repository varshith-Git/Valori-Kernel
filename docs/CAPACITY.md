# Capacity Planning

All figures are for the **BruteForce index** (exact search) unless noted.
HNSW reduces search time to O(log N) at the cost of ~1.5× RAM overhead for
the graph structure.

---

## Memory layout per record

Each record in the `RecordPool` occupies:

| Field | Size |
|---|---|
| Vector (`dim × 4 bytes`, Q16.16 i32) | `dim × 4` bytes |
| Active flag (u8) | 1 byte |
| Tag (u64) | 8 bytes |
| Namespace ID (u16) | 2 bytes |
| Linked-list pointers: next_in_ns, prev_in_ns (u32 × 2) | 8 bytes |
| Padding (alignment) | 1 byte |
| **Total** | `dim × 4 + 20` bytes |

---

## Vectors per GB by dimension

| Dimension | Bytes/record | Vectors/GB | Typical model |
|---|---|---|---|
| 8 | 52 B | ~19.5 M | Toy / embedded |
| 128 | 532 B | ~1.9 M | FastText, GloVe |
| 384 | 1,556 B | ~650 K | all-MiniLM-L6-v2, BGE-small |
| 768 | 3,092 B | ~327 K | all-mpnet-base-v2, BGE-base |
| 1,536 | 6,164 B | ~164 K | OpenAI ada-002 |
| 3,072 | 12,308 B | ~82 K | OpenAI text-embedding-3-large |

> Rule of thumb: `vectors_per_GB ≈ 1_073_741_824 / (dim × 4 + 20)`

---

## RAM per 1 M vectors

| Dimension | RAM (BruteForce) | RAM (HNSW, M=16) |
|---|---|---|
| 384 | ~1.5 GB | ~2.2 GB |
| 768 | ~3.0 GB | ~4.5 GB |
| 1,536 | ~5.9 GB | ~8.8 GB |
| 3,072 | ~11.7 GB | ~17.6 GB |

HNSW overhead = base RAM × 1.5 (approximate; depends on M and ef_construction).

---

## Search latency (BruteForce, single-threaded, Apple M2)

| Vectors in index | dim=384 | dim=768 | dim=1,536 |
|---|---|---|---|
| 10 K | < 1 ms | ~1 ms | ~2 ms |
| 100 K | ~8 ms | ~15 ms | ~30 ms |
| 500 K | ~40 ms | ~75 ms | ~150 ms |
| 1 M | ~80 ms | ~150 ms | ~300 ms |

> BruteForce is O(N × dim). For N > 100 K at high dimension, switch to HNSW.

---

## Recommended node count by workload

| Workload | Standalone | Cluster (HA) | Cluster (throughput) |
|---|---|---|---|
| Development / prototype | 1 node | — | — |
| < 100 K vectors, low QPS | 1 node | 3 nodes | 3 nodes |
| 100 K–1 M vectors, < 100 QPS | 1 node (8 GB RAM) | 3 nodes | 3–5 nodes |
| 1 M–10 M vectors, < 1 K QPS | 1 node (32 GB RAM) | 3 nodes (32 GB each) | 5–7 nodes |
| > 10 M vectors | Shard by namespace | 5 nodes min | 7+ nodes |

**Cluster sizing rules:**
- Minimum 3 nodes for fault tolerance (Raft needs quorum = ⌊N/2⌋ + 1).
- Reads served locally from any node — add read replicas by adding nodes.
- Writes route to the leader — a 3-node cluster sustains leader throughput ≈ a single node minus Raft overhead (~10–15% at 100 B payloads).
- Each node must hold the full dataset in RAM (Valori is not sharded at the storage layer today).

---

## Snapshot size

Snapshot size ≈ RAM usage + 8 KB (namespace heads) + NSRG JSON (< 1 KB for < 1 024 collections).

| 1 M vectors, dim=384 | ~1.5 GB snapshot |
|---|---|
| 1 M vectors, dim=768 | ~3.0 GB snapshot |

Allow 2× snapshot size in free disk for safe atomic snapshot writes.

---

## WAL / event log growth

Each event appended to `events.log`:

| Event type | Approximate log entry size |
|---|---|
| InsertRecord (dim=384) | ~1.6 KB |
| InsertRecord (dim=768) | ~3.1 KB |
| DeleteRecord | ~80 B |
| CreateNode / CreateEdge | ~100 B |
| CreateNamespace | ~120 B |

At 1 000 inserts/sec (dim=384): ~1.6 GB/hour of event log growth.
Compact via snapshot + purge: `POST /v1/snapshot/save` then delete log entries
older than the snapshot's log index.

---

## S3 snapshot schedule (recommended)

| Dataset size | Snapshot interval | S3 cost (us-east-1, Standard) |
|---|---|---|
| < 1 GB | Every 6 hours | ~$0.07/month |
| 1–10 GB | Every 12 hours | ~$0.35/month |
| 10–100 GB | Every 24 hours | ~$3.50/month |

Retain the last 7 daily snapshots plus one weekly for 4 weeks.
