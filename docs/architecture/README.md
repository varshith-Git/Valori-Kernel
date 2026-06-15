# Valori cluster architecture

How a 3-node Valori cluster moves data, stays consistent, and onboards new
nodes — and what each node stores. The full narrative lives in the phase
reports ([docs/phases/](../phases/README.md)); this page is the picture.

A status tag marks each protocol: **[wired]** is in the current build,
**[designed]** is the intended protocol not yet in code. Drawing the designed
paths here is deliberate — the design is only solid once they are explicit.

---

## 1. Write flow

![Valori 3-node write flow](cluster-write-flow.svg)

Steps **1–3 are network movements**; steps **4–5 run locally and identically
on every node**. Keeping that distinction visible is the point of the redraw —
apply and audit are not things the *leader* does, they are things *each* node
does once an entry is committed.

1. **Write request** — the client hits any node. A follower answers
   `307 Temporary Redirect` with the leader's address in `Location`; the
   leader accepts. **[wired]**
2. **Replicate** — the leader appends the event to its own `raft.redb` and
   pushes it to every follower over mutually-authenticated TLS (peers without
   a certificate from this cluster's CA are refused at the handshake). **[wired]**
3. **Commit** — the moment a majority (2 of 3) has the entry on disk it is
   *committed*: it cannot be lost, even if the leader dies immediately after. **[wired]**
4. **Apply** — every node independently applies the committed event to its
   in-memory kernel. The kernel is deterministic, so all three end up
   byte-identical — CI asserts the BLAKE3 state hashes match. **[wired]**
5. **Audit** — only after a successful apply does each node append the event
   to its own `events.log`: the append-only, hash-chained diary that
   `valori-verify` checks. **[wired]**

## 2. Linearizable read (read-index)

![Valori linearizable read](cluster-read-flow.svg)

Reads are served locally on any node — that is what the replicas' RAM buys.
But "locally" is not automatically "currently": a follower at applied index
1019 must not answer a query that should reflect a write committed at 1024.

The **read-index protocol** closes that gap. Before serving, the follower asks
the leader for its commit index `C`; the leader confirms it is still the leader
via a heartbeat to a quorum and returns `C`; the follower blocks until its own
applied index reaches `C`, then runs the query. The result then reflects every
write committed before the read began.

- **[designed]** — this is the required path for the strong-consistency default.
- **[wired]** today: follower reads are served **without** read-index — i.e.
  eventually consistent. The fix is to route the read handler through
  openraft's read-index before the local scan (leader reads need only the
  heartbeat confirmation; follower reads also pay the catch-up wait). One extra
  leader round trip per read is the cost.

This honesty matters: the write flow above is strongly consistent, but a read
served from a lagging replica is not — until the read-index step is wired, the
cluster offers strong writes with eventually-consistent reads, not end-to-end
linearizability.

## 3. The snapshot's two jobs

![Snapshot onboarding and audit-log rotation](snapshot-and-rotation.svg)

One periodic snapshot of kernel state does double duty.

**Job A — onboarding (InstallSnapshot). [wired]**
When node 4 joins, or a follower falls so far behind that the leader has
already trimmed the Raft entries it needs, the leader ships the kernel snapshot
via the `InstallSnapshot` RPC. The joiner installs it to jump to the snapshot
index `S`, then replays the remaining tail `S+1…C` through normal
`AppendEntries`. openraft drives this automatically; the state machine
implements `get_snapshot_builder` / `begin_receiving_snapshot` /
`install_snapshot`, and the gRPC transport carries the RPC. Without this path,
a new node could never catch up once the log it needs has been compacted.

**Job B — rotation. [designed]**
"Append-only forever" is correct for audit but unbounded on disk. The same
snapshot point seals the current `events.log` segment, a fresh segment opens
chaining from the sealed segment's final hash, and the sealed segment is
archived to cold storage (S3). Recovery then needs only `{snapshot @ S}` plus
the live segment; the BLAKE3 chain carries across segment boundaries, so a
verifier reassembles archived + live segments into one unbroken history.
Snapshotting already exists — the seal-and-rotate trigger is the piece to wire.

---

## The two files per node

| File | Role | Lifecycle |
|---|---|---|
| `raft.redb` | consensus scratchpad — entries being voted on, this node's ballot | trimmed after every snapshot; stays small |
| `events.log` | the audit diary — committed events only, BLAKE3-chained | append-only, rotated into sealed+archived segments (Job B) |

Three nodes therefore hold three independently verifiable copies of one logical
history. Any single node's diary (plus its archived segments) is sufficient
evidence — the other two machines can be gone.

## The one rule

**Raft commits, kernel applies, audit log records.** The Raft log is internal
plumbing; the audit log is forever; the kernel is never modified by the
consensus layer — its determinism is the load-bearing wall.

## Not pictured (Phase 3)

The cluster-wide BLAKE3 proof broadcast (`/v1/cluster/proof`) — every node
periodically gossiping its state hash so divergence is detected actively rather
than at audit time — is a Phase 3 addition, not part of the core data path.
