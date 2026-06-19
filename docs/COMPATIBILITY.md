# Valori Cluster Compatibility Matrix

This document defines which binary versions can coexist in a rolling upgrade
window, what `CURRENT_SCHEMA_VERSION` controls, and the procedure for bumping it.

---

## Schema version

`CURRENT_SCHEMA_VERSION` is a `u8` constant in
`crates/valori-consensus/src/types.rs`. It is stamped onto every `ClientRequest`
by the leader at proposal time and checked by every follower before applying.

A follower receiving an entry with `schema_version > CURRENT_SCHEMA_VERSION`
returns `StorageError`, halting replication on that node until it is upgraded.
The Raft cluster continues to accept writes through the remaining quorum.

---

## Version history

| Schema version | Binary version | `KernelEvent` additions / breaking changes |
|---|---|---|
| 0 | 0.1.x – 0.2.x | Initial wire format. All events from this series. |

---

## Rolling-window rules

During a rolling upgrade from version A to version B:

1. **Quorum must be maintained at all times.** On a 3-node cluster, upgrade one
   node at a time. On a 5-node cluster, you may upgrade two simultaneously, but
   wait for both to rejoin before upgrading a third.

2. **Non-leaders first, leader last.** Upgrading a follower does not transfer
   leadership. Upgrading the leader triggers a new election; upgrade it only
   after all followers are on the new binary.

3. **One `schema_version` gap is the maximum safe window.** A follower at
   version N must be able to apply entries written by a leader at version N+1.
   If a new binary bumps `CURRENT_SCHEMA_VERSION` by more than 1, the upgrade
   must be staged through intermediate versions.

4. **No downgrade after a schema bump.** Once the leader is on version N+1 and
   has committed entries with `schema_version = N+1`, rolling back a node to
   version N is unsafe — it would refuse those entries and stall.

### Coexistence matrix (current: schema version 0)

| Leader binary | Follower binary | Safe? | Notes |
|---|---|---|---|
| 0.2.x (v0) | 0.2.x (v0) | ✅ | Homogeneous cluster |
| 0.2.x (v0) | 0.1.x (pre-3.2) | ✅ | Old follower decodes `schema_version` as 0 via `#[serde(default)]` |
| future (v1) | 0.2.x (v0) | ✅ during upgrade window | v0 follower rejects v1 entries; stalls only that node |
| future (v1) | pre-3.2 | ❌ | pre-3.2 binaries ignore `schema_version` — they apply v1 entries without checking |

---

## How to bump `CURRENT_SCHEMA_VERSION`

Bump when a new `KernelEvent` variant or a breaking field change would produce
entries that an older binary cannot interpret safely.

**Do not bump** for additive changes where `#[serde(default)]` gives a sensible
fallback — those are backward-compatible at the current version.

1. In `crates/valori-consensus/src/types.rs`, increment `CURRENT_SCHEMA_VERSION`.

2. Add a row to the **Version history** table above with the new version number,
   the binary version range, and a description of what changed.

3. Update the **Coexistence matrix** above.

4. Update the test assertion in
   `crates/valori-consensus/tests/type_config.rs::current_schema_version_is_zero`
   to match the new value.

5. Run `cargo test -p valori-consensus` and confirm all tests pass.

6. Add a `CHANGELOG.md` entry under `[Unreleased]` noting the version bump and
   the minimum binary version required for a safe rolling upgrade.

---

## `valori cluster upgrade` walkthrough

```bash
valori cluster upgrade --url http://10.0.0.1:3000 --target-version 0.3.0
```

The CLI:

1. Calls `GET /v1/cluster/status` to discover all nodes and the current leader.
2. Prints an upgrade plan: non-leaders first (alphabetically by node ID), leader last.
3. For each node, prints the exact steps:
   - Stop the `valori-node` process on that host.
   - Replace the binary with the new version.
   - Restart the process with the same environment variables.
4. Waits for the operator to press Enter.
5. Polls `GET /health` every 2 s, up to 120 s, until the node is back.
6. For the leader step: additionally polls until a new leader is elected.
7. Declares the upgrade complete once all nodes have been cycled.

If any node fails to rejoin within 120 s, the CLI exits with a non-zero code and
prints the node's last known status. The cluster is still running on the
remaining nodes; you have time to investigate before retrying.
