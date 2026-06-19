# valori-cli

A command-line tool for inspecting, debugging, and verifying Valori AI memory databases directly from disk — no running server required.

---

## What it does

When you build an AI application on Valori, your database state lives in two files:

```
my_valori_db/
  snapshot.val    ← complete kernel state at a point in time
  events.log      ← every state change, in order, forever
```

The Valori CLI reads these files directly and gives you five commands:

| Command | What it answers |
|---|---|
| `inspect` | Are my database files healthy? How many records, nodes, and edges exist? |
| `verify` | Is my snapshot file structurally valid and uncorrupted? |
| `timeline` | What changed, and in what order? |
| `replay-query` | What did the database look like at event #N? What would a search return then? |
| `diff` | What changed between event #A and event #B? Did any search results shift? |
| `cluster upgrade` | Step-by-step guided rolling upgrade for a live Raft cluster. |

Think of it as `git log` + `git diff` for your AI memory database.

---

## Installation

```bash
# From the Valori-Kernel workspace root
cargo install --path crates/cli

valori --version
```

---

## Commands

### `valori inspect`

Reads your database directory and prints a status table — file sizes, record counts, graph topology, and any structural issues.

```bash
valori inspect --dir ./my_valori_db
```

```bash
# Or point at individual files
valori inspect --snapshot ./backups/snapshot.val --log ./backups/events.log
```

Example output:
```
Valori Status Report  ·  ./my_valori_db
────────────────────────────────────────────────────
┌──────────────┬────────┬──────────────────────────────────────────────────────┐
│ File         │ Status │ Details                                              │
├──────────────┼────────┼──────────────────────────────────────────────────────┤
│ snapshot.val │ OK     │ 58.04 KB  │  120 record(s)  │  45 node(s)  │  dim 384│
│ events.log   │ OK     │ 14.21 KB  │  847 event(s)   │  dim 384               │
└──────────────┴────────┴──────────────────────────────────────────────────────┘
```

---

### `valori verify`

Checks that a snapshot file is structurally valid: correct magic bytes, consistent section lengths, and a decodable kernel state. Prints the canonical BLAKE3 content hash so you can confirm a snapshot matches a known-good value.

```bash
valori verify snapshot.val
```

```
Verify — snapshot.val  (58.04 KB)

✅  STRUCTURAL INTEGRITY   PASSED
    File CRC64:  a3f2c1d4e5b60789  (carry this value for tamper detection)
    BLAKE3 hash: 4a7f3c2e1b...d9f0  (matches db.get_state_hash() from Python SDK)
    Records: 120  Nodes: 45  Edges: 63  Dim: 384

✅  SNAPSHOT VALID
```

Use this to validate a backup before restoring it, or to confirm two snapshots represent identical state.

---

### `valori timeline`

Parses `events.log` and prints every state change in a readable table — record inserts and deletes, node and edge creation, soft deletes, and snapshot checkpoints.

```bash
valori timeline ./my_valori_db/events.log
```

```bash
# Show only the first 50 events
valori timeline ./my_valori_db/events.log --limit 50
```

```
Event Timeline  ·  events.log  (log-version 1, dim 384)

┌─────────┬──────────────────┬─────────────────────────────────────────┐
│ Event # │ Type             │ Details                                 │
├─────────┼──────────────────┼─────────────────────────────────────────┤
│ 1       │ InsertRecord     │ record_id=0 tag=0                       │
│ 2       │ InsertRecord     │ record_id=1 tag=0                       │
│ 3       │ CreateNode       │ node_id=0 kind=Document                 │
│ 4       │ CreateNode       │ node_id=1 kind=Chunk → record_id=0      │
│ 5       │ CreateEdge       │ edge_id=0  0→1  kind=ParentOf           │
│ 6       │ SoftDeleteRecord │ record_id=1 (tombstoned — slot retained)│
│ —       │ Checkpoint       │ snapshot taken at event count 6         │
└─────────┴──────────────────┴─────────────────────────────────────────┘

  Total: 6 event(s)
```

---

### `valori replay-query`

Restores a snapshot as the baseline, then replays events from the log up to a target count. Reports the database state at that point, and optionally executes a nearest-neighbour search.

```bash
# What did the database look like at event #200?
valori replay-query \
  --snapshot snapshot.val \
  --log events.log \
  --at 200
```

```bash
# What would a search return at that point?
valori replay-query \
  --snapshot snapshot.val \
  --log events.log \
  --at 200 \
  --query '[0.12, -0.34, 0.56, ...]' \
  --top-k 10
```

```
Simulation Report
────────────────────────────────────────────
┌──────────────────────┬──────────────────────────────────────────────────┐
│ Metric               │ Value                                            │
├──────────────────────┼──────────────────────────────────────────────────┤
│ Target event         │ 200                                              │
│ Current event        │ 200                                              │
│ Events replayed      │ 200                                              │
│ Replay time          │ 1.243 ms                                         │
│ Records              │ 198                                              │
│ Nodes                │ 85                                               │
│ Edges                │ 112                                              │
│ State Hash (BLAKE3)  │ 4a7f3c2e1b8d6a0f...                              │
└──────────────────────┴──────────────────────────────────────────────────┘

Search Results  ·  top-10  ·  0.041 ms
────────────────────────────────────────────
┌──────┬───────────┬─────────────┐
│ Rank │ Record ID │ L2 Distance │
├──────┼───────────┼─────────────┤
│ 1    │ 42        │ 12048       │
│ 2    │ 7         │ 18391       │
└──────┴───────────┴─────────────┘
```

**Practical use case:** Your agent gave a wrong answer at 3am. You know roughly which request it was. Replay to that event count and run the same query to see exactly what the retrieval returned.

---

### `valori diff`

Replays to two different event counts from the same snapshot baseline and compares the results. Shows which records entered or left the top-K and which shifted rank for a given query vector.

```bash
valori diff \
  --snapshot snapshot.val \
  --log events.log \
  --from 150 \
  --to 200 \
  --query '[0.12, -0.34, 0.56, ...]' \
  --top-k 5
```

```
State Comparison
──────────────────────────────────────────────
┌──────────────────────┬────────────┬────────────┐
│ Property             │ Event #150 │ Event #200 │
├──────────────────────┼────────────┼────────────┤
│ Records              │ 148        │ 198        │
│ Nodes                │ 62         │ 85         │
│ Edges                │ 79         │ 112        │
│ State hash (BLAKE3)  │ 4a7f3c…    │ 9f2e1b…    │
└──────────────────────┴────────────┴────────────┘
  Status: DRIFTED

Drift Analysis  (50 new event(s))
──────────────────────────────────────────────
┌─────────┬─────────────────────────────────────────┐
│ Event # │ Applied in B, absent in A               │
├─────────┼─────────────────────────────────────────┤
│ 151     │ state-changing event not present in A   │
│ 152     │ state-changing event not present in A   │
│ ...     │ ...                                     │
└─────────┴─────────────────────────────────────────┘

Semantic Diff  ·  top-5
──────────────────────────────────────────────
┌───────────┬────────────────┬────────────────┐
│ Record ID │ Change         │ Detail         │
├───────────┼────────────────┼────────────────┤
│ 172       │ + Entered top-K│ rank 3         │
│ 14        │ ~ Rank shift   │ 2 → 4          │
│ 99        │ - Left top-K   │ was rank 5     │
└───────────┴────────────────┴────────────────┘
```

---

### `valori cluster upgrade`

Interactive guided rolling upgrade for a live Raft cluster. Point `--url` at any
node's HTTP API; the CLI discovers the full topology, sorts nodes non-leaders
first and leader last, then walks you through each one step-by-step.

```bash
valori cluster upgrade --url http://10.0.0.1:3000 --target-version 0.3.0
```

The CLI polls `/health` every 2 s (up to 120 s) after each node restart before
moving to the next. For the leader step it additionally waits for a new election
to complete. No process management — it trusts your deployment tooling.

See [`docs/COMPATIBILITY.md`](../../docs/COMPATIBILITY.md) for the rolling-window
rules, schema version policy, and coexistence matrix.

---

## Working with the Python SDK together

The CLI reads the same files the Python SDK writes. No conversion needed.

```python
# Python: create a snapshot
import valoricore
db = valoricore.Valoricore(path="./my_valori_db")
# ... insert vectors, build graphs ...
snap = db.snapshot()
with open("./my_valori_db/snapshot.val", "wb") as f:
    f.write(snap)
```

```bash
# CLI: inspect it immediately
valori inspect --dir ./my_valori_db
valori verify ./my_valori_db/snapshot.val
valori timeline ./my_valori_db/events.log
```

---

## Query vector format

The `--query` argument accepts a JSON array of **float** values matching your database dimension. The CLI converts them to Q16.16 fixed-point internally, exactly as the Python SDK does.

```bash
# 4-dimensional example
--query '[0.1, -0.5, 0.8, 0.3]'

# Paste from Python: json.dumps(embedding.tolist())
--query '[0.0231, -0.1847, 0.3912, ...]'
```

---

## How it works

The CLI is entirely **offline** — it never connects to a running server. It reads two file formats:

**`snapshot.val`** — A binary blob starting with the magic bytes `VAL1`, followed by three length-prefixed sections: kernel state (vectors + graph topology), metadata, and index. The kernel section encodes everything needed to restore a `KernelState` deterministically.

**`events.log`** — An append-only log of `KernelEvent` frames (bincode-encoded), prefixed by a 16-byte header containing the format version and vector dimension. Each frame is either an `Event` (one of seven operation types) or a `Checkpoint` marker written when a snapshot is taken.

The `replay-query` and `diff` commands restore from the snapshot and then apply events one by one using the same deterministic `KernelState::apply_event` path the live engine uses — so the state you see in the CLI is provably identical to what the live engine held at that moment.

---

## Benchmarks

The CLI ships with five standalone benchmark binaries for evaluating kernel performance on SIFT1M data:

```bash
# End-to-end ingestion throughput
cargo run --bin bench_ingest --release

# Memory bandwidth breakdown (I/O vs parsing vs fixed-point math)
cargo run --bin bench_1m --release

# Tag-filtered search correctness
cargo run --bin bench_filter --release

# Snapshot save/load round-trip latency
cargo run --bin bench_persistence --release

# Recall@1 and Recall@10 vs brute-force ground truth
cargo run --bin bench_recall --release
```

SIFT1M vectors should be placed at `data/sift/sift/sift_base.fvecs`.

---

## License

AGPLv3 — see the root `LICENSE` file.
