# RFC-0007: Local Cluster Orchestration

**Status:** Proposed
**Owner:** `valori-daemon` (control plane), `ui` (Tauri desktop)
**Stability:** Proposed
**Last reviewed:** 2026-08-02
**Prerequisites:** RFC-0006 (Desktop Daemon Architecture)

---

## 1. Summary & Current State

Today, `valori-daemon` is capable of launching and supervising a **single Valori node**. When a project is opened with a replication factor of **1**, the daemon:

1. Allocates an available HTTP port.
2. Starts one `valori-node` process.
3. Monitors its PID.
4. Restarts the node automatically if it crashes.

Conceptually:

```text
Valori Studio (Tauri Frontend)
      │
      ▼
valori-daemon (Supervised Process Manager)
      │
      ▼
valori-node (Single process instance)
```

This provides a managed local development experience similar to how desktop databases launch a single embedded server.

---

## 2. The Gap

Clustered projects require multiple coordinated processes rather than a single process. 

For example, a project configured with **replication = 3** should automatically create a complete local Raft cluster. Instead of launching one node, the daemon becomes a local cluster manager:

```text
                Valori Studio
                      │
                      ▼
                valori-daemon
          ┌───────────┼───────────┐
          ▼           ▼           ▼
      Node 1      Node 2      Node 3
```

The daemon is responsible for orchestrating the entire cluster lifecycle.

---

## 3. Core Responsibilities

### A. Node Provisioning

Create the required number of local nodes. For a three-node cluster:

| Node | HTTP Port | Raft Port (gRPC) | Node ID |
| ---- | --------- | ---------------- | ------- |
| 1    | 3001      | 3101             | 1       |
| 2    | 3002      | 3102             | 2       |
| 3    | 3003      | 3103             | 3       |

Every node must receive unique ports and a stable, isolated database directory.

### B. Cluster Bootstrap

The daemon prepares the environment for every process. Example:

**Node 1**
```text
VALORI_NODE_ID=1
VALORI_CLUSTER_INIT=1
VALORI_CLUSTER_MEMBERS=1=127.0.0.1:3101/127.0.0.1:3001,2=127.0.0.1:3102/127.0.0.1:3002,3=127.0.0.1:3103/127.0.0.1:3003
```

**Node 2**
```text
VALORI_NODE_ID=2
VALORI_CLUSTER_MEMBERS=1=127.0.0.1:3101/127.0.0.1:3001,2=127.0.0.1:3102/127.0.0.1:3002,3=127.0.0.1:3103/127.0.0.1:3003
```

**Node 3**
```text
VALORI_NODE_ID=3
VALORI_CLUSTER_MEMBERS=1=127.0.0.1:3101/127.0.0.1:3001,2=127.0.0.1:3102/127.0.0.1:3002,3=127.0.0.1:3103/127.0.0.1:3003
```

Only the first node initializes the Raft cluster (`VALORI_CLUSTER_INIT=1`). The remaining nodes join it automatically.

### C. Process Supervision

Instead of monitoring a single process, the daemon supervises the entire process group:

```text
Daemon
├── Node 1 (healthy)
├── Node 2 (crashed) ──► restart Node 2 only
└── Node 3 (healthy)
```

A failure of one node should not interrupt the others, preserving the quorum.

### D. Cluster Observability

The daemon should expose a single cluster status to the UI. Rather than reporting the health of individual processes independently, it aggregates:

* **Consensus state-hash convergence**: Verification that all active replicas have converged to the identical Merkle-tree state hash.
* **Leader/Follower topology**: Which node is currently elected leader and which are followers.
* **Replication lag**: Height differences in WAL and Raft logs between the leader and followers.
* **Quorum availability**: Whether a majority of replicas are currently alive.
* **WAL replay progress**: Node startup replay position.

This allows Valori Studio to present the cluster as one logical database while highlighting divergence or partition issues.

### E. Lifecycle Management

Operations initiated from the UI apply to the cluster as a whole:

* **Start / Stop**: Spawning or killing all nodes in the cluster.
* **Restart**: Triggering graceful rolling restarts to avoid quorum loss.
* **Scale**: Adding or removing replicas dynamically.

---

## 4. Why This Matters

This functionality makes the daemon behave like a lightweight local orchestrator. Developers can experiment with distributed features such as:

* Raft consensus
* Leader election
* Replication
* Failover
* Quorum recovery
* Network partitions

without installing Kubernetes, Docker Compose, or multiple virtual machines. Opening a replicated project should feel no different from opening a single-node project—the daemon handles all of the orchestration transparently.

---

## 5. Current Status

The project metadata already contains the information needed to describe clustered deployments (such as replication settings and node configurations). 

The remaining work is implementing the daemon's orchestration layer to allocate ports, spawn/supervise multiple node processes, and expose the aggregated cluster observability.

---

## 6. Future Capabilities

Once multi-node orchestration is in place, the daemon can become the foundation for additional local-cluster features:

* **One-click cluster creation**: Setting replica counts directly from the UI.
* **Chaos testing & simulated failures**: Killing nodes or adding network delays from the dashboard to test application resiliency.
* **Rolling upgrades**: Upgrading database nodes one by one with zero downtime.
* **Automatic snapshot scheduling**: Directing the leader to periodically save and offload snapshots.
* **Local disaster recovery testing**: Simulating cluster corruption and restoring from WAL archives.
