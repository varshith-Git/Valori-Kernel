# RFC-0007: Local Cluster Orchestration

**Status:** Proposed
**Owner:** `valori-daemon` (control plane), `ui` (Tauri desktop)
**Stability:** Proposed
**Last reviewed:** 2026-08-02
**Prerequisites:** RFC-0006 (Desktop Daemon Architecture)

---

## TL;DR

Right now, opening a **single-node** project in Valori Studio "just works" — the daemon starts one process, watches it, restarts it if it dies. Opening a **replicated** project (3 nodes, say) does not work yet: nothing spawns the other two nodes or wires them into a cluster.

This RFC scopes the work to close that gap: teach `valori-daemon` to stand up, supervise, and report on a whole local Raft cluster — so that from the developer's point of view, opening a 3-node project feels exactly like opening a 1-node project. No Docker, no Kubernetes, no manual port juggling.

**What we're asking reviewers to sign off on:** the daemon becoming a *local cluster manager*, not just a *single-process supervisor* — including the five responsibilities in §3 and the explicit non-goals in §2.1.

---

## 1. Current State

Today, `valori-daemon` can launch and supervise exactly **one** `valori-node`. When a project is opened with replication factor **1**, the daemon:

1. Allocates an available HTTP port.
2. Starts one `valori-node` process.
3. Monitors its PID.
4. Restarts the node automatically if it crashes.

```text
Valori Studio (Tauri Frontend)
      │
      ▼
valori-daemon (Supervised Process Manager)
      │
      ▼
valori-node (Single process instance)
```

This is comparable to how a desktop database (SQLite, etc.) launches a single embedded server — simple, and it's why single-node projects already feel effortless to open.

---

## 2. The Gap

A clustered project needs **multiple coordinated processes**, not one. A project configured with **replication = 3** should transparently produce a working 3-node Raft cluster on the developer's machine — today it doesn't produce anything beyond a single node.

Closing this gap turns the daemon from a single-process babysitter into a local cluster manager:

```text
                Valori Studio
                      │
                      ▼
                valori-daemon
          ┌───────────┼───────────┐
          ▼           ▼           ▼
      Node 1      Node 2      Node 3
```

### 2.1 Non-Goals

To keep review scope tight, this RFC explicitly does **not** cover:

* Multi-**machine** clusters (this is local-only — all nodes run on the developer's own laptop).
* Production/cloud cluster orchestration (that's the separate Cloud SaaS control plane).
* Automatic replica-count changes without a UI-initiated action (no autoscaling).
* Cross-project resource sharing or port pooling beyond what's needed to avoid collisions.

---

## 3. What the Daemon Needs to Do

Five responsibilities, roughly in the order a developer would experience them.

### A. Provision the nodes

Spin up the right number of local node processes, each with a unique port and its own isolated database directory. For a three-node cluster:

| Node | HTTP Port | Raft Port (gRPC) | Node ID |
| ---- | --------- | ----------------- | ------- |
| 1    | 3001      | 3101              | 1       |
| 2    | 3002      | 3102              | 2       |
| 3    | 3003      | 3103              | 3       |

**Why it matters:** without this, two clustered projects opened side-by-side would collide on ports or overwrite each other's data.

### B. Bootstrap the Raft cluster

Give each process the environment it needs to find the others and agree on who starts the cluster.

**Node 1** (the bootstrap node)
```text
VALORI_NODE_ID=1
VALORI_CLUSTER_INIT=1
VALORI_CLUSTER_MEMBERS=1=127.0.0.1:3101/127.0.0.1:3001,2=127.0.0.1:3102/127.0.0.1:3002,3=127.0.0.1:3103/127.0.0.1:3003
```

**Nodes 2 and 3** (joiners — same `VALORI_CLUSTER_MEMBERS`, no `VALORI_CLUSTER_INIT`)
```text
VALORI_NODE_ID=2
VALORI_CLUSTER_MEMBERS=1=127.0.0.1:3101/127.0.0.1:3001,2=127.0.0.1:3102/127.0.0.1:3002,3=127.0.0.1:3103/127.0.0.1:3003
```

**Why it matters:** exactly one node must initialize the Raft group; the rest must join it automatically. Get this wrong and you either get three independent single-node clusters or a cluster that never forms.

### C. Supervise every node independently

One crashed node shouldn't take the others down with it, and shouldn't require a manual restart.

```text
Daemon
├── Node 1 (healthy)
├── Node 2 (crashed) ──► restart Node 2 only
└── Node 3 (healthy)
```

**Why it matters:** this is what makes Raft's fault tolerance actually demonstrable locally — a developer can kill a node and watch the cluster keep serving from the remaining quorum, the same failure mode they'd see in production.

### D. Roll individual node health into one cluster status

The UI shouldn't have to reason about three separate processes. The daemon aggregates:

* **State-hash convergence** — have all replicas converged on the same data?
* **Leader/follower topology** — who's currently leading?
* **Replication lag** — how far behind is each follower?
* **Quorum availability** — is a majority of the cluster alive right now?
* **WAL replay progress** — is a restarting node still catching up?

**Why it matters:** this is the difference between "Valori Studio shows a 3-node cluster" and "Valori Studio shows *one database* that happens to be replicated" — the latter is the actual product goal.

### E. Handle whole-cluster lifecycle actions from the UI

Start, stop, restart, and scale operations issued from Valori Studio need to apply to the whole cluster, coordinated by the daemon — not to be manually repeated per node by the user.

---

## 4. Why This Matters (Product Impact)

Today, trying out Raft consensus, leader election, replication, or failover means standing up Docker Compose or a small VM fleet — a real barrier for anyone just evaluating Valori or debugging a cluster-specific issue.

With this RFC done: **opening a replicated project feels identical to opening a single-node one.** One click, one loading state, one health indicator — the daemon absorbs all the orchestration complexity. That turns "distributed systems features" from something you read about in docs into something you can click into and break on purpose, locally, in seconds.

---

## 5. Current Status

The project metadata format already has everything needed to *describe* a clustered deployment (replication factor, node config). What's missing is the daemon's orchestration layer itself — port allocation, multi-process spawning, Raft membership wiring, per-node supervision, and health aggregation, all described in §3.

---

## 6. Future Capabilities (Out of Scope Here, Enabled By This)

Once multi-node orchestration lands, it becomes the foundation for:

* **One-click cluster creation** — set replica count directly in the UI.
* **Chaos testing** — kill nodes or inject network delay from the dashboard to test resiliency.
* **Rolling upgrades** — upgrade nodes one at a time with zero downtime.
* **Automatic snapshot scheduling** — leader periodically saves and offloads snapshots.
* **Local disaster-recovery drills** — simulate corruption, restore from WAL archives.
