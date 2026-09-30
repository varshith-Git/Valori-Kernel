# Deploy: control plane (Raft cluster membership)

## Scope note — read this first
This repo has **no SaaS/billing control plane** (no Postgres, no Supabase, no tenant provisioning — see
`.claude/rules/architecture.md`). "Control plane" here means the **Raft cluster management plane**:
growing/shrinking cluster membership and checking cluster health via `valori-consensus` +
`valori-node`'s `/v1/cluster/*` API and the `valori cluster` CLI. If a task actually means "deploy a new
`valori-node` binary/image," that's `.claude/skills/deploy-worker/SKILL.md` instead — membership changes
and binary/version rollouts are different operations with different risk profiles.

## Purpose
Safely grow, shrink, or inspect a running Raft cluster's membership without breaking quorum or losing a
voter unrecoverably.

## When to use
Adding a node to an existing cluster, removing a node, or diagnosing cluster health/leadership —
explicitly requested, on a cluster the user identifies (never against an inferred or guessed host).

## Preconditions
1. Identify the target cluster's leader (or any member — the CLI follows redirects for leader-only
   actions): `valori cluster status --url http://<any-node>:3000`.
2. Confirm current health: `valori cluster health --url http://<node>:3000` (exit 0 iff a leader exists).
3. For **add-node**: the new node must already be running, configured with the cluster's full
   `VALORI_CLUSTER_MEMBERS` topology, and explicitly **without** `VALORI_CLUSTER_INIT` set (that flag is
   bootstrap-only — setting it on a node joining an existing cluster is a real, documented mistake).
4. For **remove-node**: confirm current voter count first — `docs/CLUSTER.md` notes removing the last
   voter is refused by the system itself, but removing a voter that drops the cluster below its fault
   tolerance threshold is not refused and is the operator's responsibility to avoid.

## Steps (verified against `docs/CLUSTER.md`)

**Grow:**
```bash
# 1. Start the new node first (already configured, no VALORI_CLUSTER_INIT)
# 2. Join it:
valori cluster add-node --url http://<leader>:3000 \
    --id <N> --raft-addr <host>:3100 --api-addr <host>:3000
# 3. Confirm:
valori cluster status --url http://<leader>:3000   # new id should appear as a voter
```
`add-node` performs the two-step openraft dance automatically: adds as a **learner** (catches up on the
log without affecting quorum), then promotes to **voter**. Don't try to skip the learner step manually.

**Shrink:**
```bash
valori cluster remove-node --url http://<leader>:3000 --id <N>
```

## Verification after any membership change
- `valori cluster status` — confirm the expected member set and that a leader still exists.
- Check the replication/hash-convergence signal (`state_hash_match` gauge, per `AGENTS.md`'s
  `src/replication.rs` reference) — a member that joined but hasn't converged its state hash is not
  actually ready to serve reads that need cluster consistency.
- Read `/health` on the changed node directly, not just the cluster-wide status.

## Failure handling
- If `add-node` fails after the learner step but before promotion: re-run `add-node` with the same id —
  the operation is designed to be safe to retry (openraft handles the learner state).
- If a node won't rejoin after a restart: check it's using the exact same `VALORI_CLUSTER_MEMBERS`
  topology string as the rest of the cluster — a mismatch here is the most common real cause.

## Never do
- Never set `VALORI_CLUSTER_INIT` on a node joining an existing cluster.
- Never remove a voter without checking the resulting fault-tolerance margin (`docs/CLUSTER.md`'s "Fault
  tolerance" section) — the system refuses only the degenerate last-voter case, not "you now have zero
  redundancy."
- Never treat a `learner` as ready to serve linearizable reads — it hasn't been promoted yet.
