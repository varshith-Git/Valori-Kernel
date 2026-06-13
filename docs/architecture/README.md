# Valori cluster architecture

How a write flows through a 3-node Valori cluster, and what each node
stores. The full narrative lives in the phase reports
([docs/phases/](../phases/README.md)); this page is the picture.

![Valori 3-node write flow](cluster-write-flow.svg)

## The flow, step by step

1. **Write request** — the client hits any node's HTTP API. A follower
   answers `307 Temporary Redirect` with the leader's address in
   `Location`; the leader accepts.
2. **Replicate** — the leader appends the event to its own `raft.redb`
   and pushes it to every follower over mutually-authenticated TLS
   (peers without a certificate from this cluster's CA are refused at
   the handshake).
3. **Commit** — the moment a majority (2 of 3) has the entry on disk it
   is *committed*: it can no longer be lost, even if the leader dies
   immediately after.
4. **Apply** — every node independently applies the committed event to
   its in-memory kernel. The kernel is deterministic, so all three end
   up byte-identical — CI asserts the BLAKE3 state hashes match.
5. **Audit** — only after a successful apply does each node append the
   event to its own `events.log`: the append-only, hash-chained diary
   that `valori-verify` checks. It contains exactly the quorum-agreed
   history, in the agreed order.

## The two files per node

| File | Role | Lifecycle |
|---|---|---|
| `raft.redb` | consensus scratchpad — entries being voted on, this node's ballot | trimmed after every snapshot; stays small |
| `events.log` | the audit diary — committed events only, BLAKE3-chained | append-only forever (rotates into archives) |

Three nodes therefore hold three independently verifiable copies of one
logical history. Any single node's diary is sufficient evidence — the
other two machines can be gone.

## The one rule

**Raft commits, kernel applies, audit log records.** The Raft log is
internal plumbing; the audit log is forever; the kernel is never modified
by the consensus layer — its determinism is the load-bearing wall.
