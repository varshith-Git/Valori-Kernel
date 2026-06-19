# Disaster Recovery & Multi-Region Runbook

---

## Recovery objectives

| Scenario | Target RTO | Target RPO |
|---|---|---|
| Single node failure (3-node cluster) | 0 (automatic failover) | 0 |
| Full cluster loss, snapshot on S3 | < 15 min | Last snapshot interval |
| Full cluster loss, no snapshot | N/A — manual WAL replay | Last WAL entry |
| Region failure, cross-region replica | < 5 min | ~1 replication lag |

---

## 1. Single node failure (cluster auto-recovery)

No operator action required for a minority failure in a 3-node cluster.

```
Timeline:
  T+0s    Node 2 crashes.
  T+0–5s  Remaining nodes detect heartbeat timeout (raft election timeout).
  T+5s    New leader elected; cluster resumes writes.
  T+Xs    Node 2 restarts, rejoins as learner, catches up via snapshot or log tail.
  T+Ys    Node 2 promoted back to voter automatically.
```

**Verify recovery:**
```bash
curl http://node1:3000/v1/cluster/status
# → leader: node1, members: [1, 3], learners: [2]   (while node2 catches up)
# → leader: node1, members: [1, 2, 3]               (after promotion)
```

---

## 2. Snapshot to S3 (backup)

### Automated snapshot + upload

Add this to your crontab or k8s CronJob (runs on the leader):

```bash
#!/usr/bin/env bash
# snapshot_to_s3.sh
set -euo pipefail

VALORI_URL="${VALORI_URL:-http://localhost:3000}"
S3_BUCKET="${S3_BUCKET:-s3://your-valori-backups}"
TIMESTAMP=$(date -u +%Y%m%dT%H%M%SZ)
SNAP_FILE="/tmp/valori_${TIMESTAMP}.snap"

# Download snapshot binary from the node
curl -sf "$VALORI_URL/v1/snapshot/download" -o "$SNAP_FILE"

# Record the state hash alongside the snapshot (used to verify restore)
STATE_HASH=$(curl -sf "$VALORI_URL/v1/proof/state" | python3 -c \
  "import sys,json; print(json.load(sys.stdin)['final_state_hash'])")
echo "$STATE_HASH" > "${SNAP_FILE}.hash"

# Upload to S3
aws s3 cp "$SNAP_FILE"       "${S3_BUCKET}/snapshots/${TIMESTAMP}.snap"
aws s3 cp "${SNAP_FILE}.hash" "${S3_BUCKET}/snapshots/${TIMESTAMP}.hash"

# Retain last 7 daily + 4 weekly snapshots (delete older)
aws s3 ls "${S3_BUCKET}/snapshots/" \
  | awk '{print $4}' | grep '\.snap$' | sort | head -n -7 \
  | xargs -I{} aws s3 rm "${S3_BUCKET}/snapshots/{}"

echo "Snapshot ${TIMESTAMP} uploaded. Hash: ${STATE_HASH}"
rm -f "$SNAP_FILE" "${SNAP_FILE}.hash"
```

**Kubernetes CronJob:**
```yaml
apiVersion: batch/v1
kind: CronJob
metadata:
  name: valori-snapshot
spec:
  schedule: "0 */6 * * *"    # every 6 hours
  jobTemplate:
    spec:
      template:
        spec:
          containers:
          - name: snap
            image: amazon/aws-cli
            command: ["/bin/sh", "/scripts/snapshot_to_s3.sh"]
            env:
            - name: VALORI_URL
              value: "http://valori-leader:3000"
            - name: S3_BUCKET
              value: "s3://your-valori-backups"
          restartPolicy: OnFailure
```

---

## 3. Restore from S3 snapshot

### Step-by-step full cluster restore

```bash
# 1. Download the most recent snapshot
LATEST=$(aws s3 ls s3://your-valori-backups/snapshots/ \
  | grep '\.snap$' | sort | tail -1 | awk '{print $4}')
aws s3 cp "s3://your-valori-backups/snapshots/${LATEST}" /tmp/restore.snap
aws s3 cp "s3://your-valori-backups/snapshots/${LATEST%.snap}.hash" /tmp/restore.hash

EXPECTED_HASH=$(cat /tmp/restore.hash)
echo "Restoring from: ${LATEST}"
echo "Expected hash:  ${EXPECTED_HASH}"

# 2. Start a single fresh node (no cluster, no WAL)
docker run --rm -d \
  --name valori-restore \
  -p 3000:3000 \
  -e VALORI_DIM=384 \
  -e VALORI_MAX_RECORDS=2000000 \
  valori-node:latest

sleep 3

# 3. Upload the snapshot
curl -sf -X POST http://localhost:3000/v1/snapshot/upload \
  -H "Content-Type: application/octet-stream" \
  --data-binary @/tmp/restore.snap

# 4. Verify the hash matches
ACTUAL_HASH=$(curl -sf http://localhost:3000/v1/proof/state \
  | python3 -c "import sys,json; print(json.load(sys.stdin)['final_state_hash'])")

if [ "$ACTUAL_HASH" = "$EXPECTED_HASH" ]; then
  echo "✓  Hash verified — restore is bit-exact"
else
  echo "✗  HASH MISMATCH — do not proceed"
  echo "   Expected: $EXPECTED_HASH"
  echo "   Got:      $ACTUAL_HASH"
  exit 1
fi

# 5. Bootstrap remaining cluster nodes from this node's snapshot
# (openraft's InstallSnapshot RPC handles distribution automatically)
```

---

## 4. Cross-region active-passive

### Architecture

```
Region A (primary)          Region B (DR)
┌─────────────────────┐     ┌─────────────────────┐
│  node-1 (leader)    │     │  node-4 (learner)   │
│  node-2 (follower)  │────►│  node-5 (learner)   │
│  node-3 (follower)  │     └─────────────────────┘
└─────────────────────┘     (read-only DR replica)
```

Nodes in Region B are added as **learners** (non-voting members). They receive
all log entries via the standard Raft AppendEntries flow but do not participate
in leader elections, so a Region B network partition cannot cause a split-brain.

### Adding DR learners

```bash
# On the Region A leader
curl -X POST http://node1-region-a:3000/v1/cluster/add-node \
  -H "Content-Type: application/json" \
  -d '{
    "node_id": 4,
    "raft_addr": "node4.region-b.internal:3100",
    "api_addr":  "node4.region-b.internal:3000",
    "learner_only": true
  }'
```

Learners consume the full replication stream. Their state hash should converge
to the leader's within one replication round-trip (typically < 100 ms over a
cross-region link with low write volume).

**Monitor convergence:**
```bash
# Region B node should report same state hash as Region A leader
HASH_A=$(curl -sf http://node1-region-a:3000/v1/proof/state | jq -r .final_state_hash)
HASH_B=$(curl -sf http://node4-region-b:3000/v1/proof/state | jq -r .final_state_hash)
[ "$HASH_A" = "$HASH_B" ] && echo "✓ regions in sync" || echo "✗ lagging"
```

### Region A failure — promoting Region B to primary

```bash
# 1. Confirm Region A is truly down (avoid split-brain before promoting)
curl http://node1-region-a:3000/v1/cluster/health   # should timeout

# 2. Promote Region B learners to voters on node4
curl -X POST http://node4-region-b:3000/v1/cluster/add-node \
  -H "Content-Type: application/json" \
  -d '{"node_id": 4, "raft_addr": "node4.region-b.internal:3100", "api_addr": "node4.region-b.internal:3000"}'

curl -X POST http://node4-region-b:3000/v1/cluster/add-node \
  -H "Content-Type: application/json" \
  -d '{"node_id": 5, "raft_addr": "node5.region-b.internal:3100", "api_addr": "node5.region-b.internal:3000"}'

# 3. Update DNS / load-balancer to route traffic to Region B
# 4. When Region A recovers, add its nodes back as learners first,
#    verify hash convergence, then promote.
```

---

## 5. Verification checklist after any restore

```bash
# 1. State hash matches the recorded pre-failure hash
curl http://localhost:3000/v1/proof/state

# 2. Record count is plausible
curl http://localhost:3000/health | jq .record_count

# 3. A known vector returns the expected top-1 hit
curl -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [<known_vector>], "k": 1}'

# 4. Collections are intact
curl http://localhost:3000/v1/namespaces

# 5. Cluster health (if multi-node)
curl http://localhost:3000/v1/cluster/status
```

---

## 6. Snapshot retention policy

| Snapshot age | Action |
|---|---|
| < 7 days | Keep all daily snapshots |
| 7–28 days | Keep one per week |
| > 28 days | Delete (or archive to S3 Glacier) |

Store the `.hash` file alongside every snapshot. Never restore a snapshot
without verifying its hash — a truncated upload or S3 multipart error will
produce a snapshot whose hash check fails immediately, before any data is loaded.
