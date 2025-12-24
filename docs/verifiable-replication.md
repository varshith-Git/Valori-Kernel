# Verifiable Replication & Node Proofs

**Protocol Version V1**

Valori provides cryptographic proofs of its memory state to enable **Verifiable Replication**.
Any node can be asked to prove its state, and this proof can be verified offline against a trusted snapshot history.

## The Proof Endpoint

`GET /v1/proof/state`

**Headers**:
- `Authorization: Bearer <token>` (Required)

**Response**:
```json
{
  "kernel_version": 1,
  "snapshot_hash": "a1b2c3...",
  "wal_hash": "e5f6g7...",
  "final_state_hash": "123456..."
}
```

## Equivalence Semantics

Two nodes $A$ and $B$ are considered **State Equivalent** if and only if:

$$
Proof(A) == Proof(B)
$$

This implies:
1.  They started from the same **Snapshot** (Canonical content match).
2.  They applied the same **Command Log** (WAL match).
3.  They reached the same **Kernel Memory State** (including empty slots).

**One bit difference** in any vector, flag, or topology link will result in a different `final_state_hash`.

## Divergence Detection

Valori nodes are **Fail-Closed**.
If a node's internal state diverges from the expected replay of its inputs (Snapshot + WAL), the proof will reveal this.

### Common Failure Modes

| Condition | Proof Result | Verification |
|---|---|---|
| **Clean Sync** | Hash matches Snapshot + Empty WAL | **PASS** |
| **Pending Writes** | State Hash differs from Snapshot | **FAIL** (until persisted) |
| **Corruption** | State Hash differs from expected | **FAIL** |
| **Forked History** | WAL Hash differs | **FAIL** |

### "Dirty" Nodes
Since `valori-node` V1 persists via **Snapshots Only** (No WAL persistence), a node that has accepted writes but not yet saved a snapshot is considered "Dirty".
Its proof will show:
- `snapshot_hash`: Hash of last saved snapshot.
- `wal_hash`: Empty (no persisted log).
- `final_state_hash`: Hash of current modified memory.

A verifier will check `Replay(Snapshot, EmptyWAL)` and find it does **not match** `final_state_hash`.
**This is correct behavior.** It indicates the node has uncommitted state.
To fix, trigger `POST /v1/snapshot/save`.

## Verification Tooling

Use `valori-verify` to audit proofs offline.

```bash
# verify equivalence manually or strictly
valori-verify snapshot.bin wal.bin
```
