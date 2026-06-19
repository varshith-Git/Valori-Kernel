# Valori Threat Model

This document defines what Valori protects against, what it explicitly does not
protect against, and where the responsibility boundary lies between the engine
and the operator.

---

## In scope

### 1. Silent state drift across replicas

**Threat:** Two nodes apply the same events but diverge due to floating-point
non-determinism, clock skew, or OS scheduler effects.

**Mitigation:** All arithmetic is Q16.16 fixed-point. The BLAKE3 state hash is
recomputed after every apply. The cluster's `/v1/cluster/status` and
state-hash watcher surface divergence within one poll interval (default 30 s).
The Raft consensus protocol prevents two leaders from being active simultaneously
in the same term.

---

### 2. Tampered audit log

**Threat:** An attacker (or rogue operator) modifies or deletes entries in
`events.log` after they are written.

**Mitigation:** Each entry in `events.log` carries the BLAKE3 hash of the
previous entry (chained). Any modification — insert, delete, reorder — breaks
the chain, which is detectable by replaying the log and recomputing the root.
The root hash is independently stored in the kernel state (BLAKE3 Merkle tree),
so a post-hoc chain forgery also requires recomputing the Merkle root to match.

**Limitation:** Valori does not currently prevent a privileged process from
atomically replacing *both* the log and the kernel state (see out-of-scope §1).

---

### 3. Snapshot corruption

**Threat:** A snapshot file is corrupted in transit or on disk.

**Mitigation:** Every snapshot carries the BLAKE3 hash of the kernel state at
the time of serialization. `restore()` recomputes the hash from the deserialized
state and refuses a mismatch.

---

### 4. Replay of duplicate commands

**Threat:** A network retry causes the same write to be applied twice, leading
to ghost records in the corpus.

**Mitigation:** Every `ClientRequest` carries a `request_id` (UUID). The
state machine maintains a dedup table (max 65 536 entries, LRU-evicted). Entries
in the dedup table travel in snapshots so every replica makes the same dedup
decision even after a leader failover.

---

### 5. Unauthorized writes in cluster mode

**Threat:** A client writes directly to a follower, bypassing the leader.

**Mitigation:** Followers return HTTP 307 with a `Location` header pointing at
the current leader. The leader is the only node that can commit to the Raft log.
Writes routed around the leader are structurally impossible to commit.

---

### 6. Namespace cross-contamination (multi-tenancy)

**Threat:** A tenant's vectors appear in another tenant's search results.

**Mitigation:** Per-namespace isolation is enforced at three independent points
in the engine: the event-commit path, the WAL replay path, and `build_index()`
after snapshot restore. Non-default namespace records are never inserted into
the global BruteForce/HNSW index. Namespace IDs are integers that cannot be
guessed from names — the registry resolves names to IDs server-side.

---

## Out of scope

### 1. Privileged OS-level attacker

An attacker with root access to the host can read or overwrite any file,
including `events.log`, the redb database, and snapshot files. Valori does not
encrypt data at rest. Use full-disk encryption (LUKS, FileVault, BitLocker) or
a secrets manager for key material.

### 2. Compromised leader node

If the leader node itself is compromised, it can commit arbitrary log entries
to the Raft cluster. Valori does not implement Byzantine fault tolerance — it
assumes all nodes are honest (crash-fault tolerant only). For Byzantine
environments, use a BFT consensus protocol (e.g. HotStuff, PBFT).

### 3. Transport confidentiality (plaintext HTTP)

The default HTTP API is plaintext. Run behind a TLS-terminating reverse proxy
(nginx, Caddy, AWS ALB) or enable mTLS on the gRPC Raft transport if inter-node
traffic crosses untrusted networks.

### 4. Authentication and authorization

Valori has a single bearer-token mechanism (`VALORI_AUTH_TOKEN`). There is no
RBAC, per-tenant API key, or namespace-level access control. Enforce
authorization at the API gateway layer.

### 5. Embedding model confidentiality

Valori stores raw float vectors, not the original text. If embedding inversion
is a concern (recovering approximate text from vectors), encrypt the vectors
before insertion or use a keyed embedding model. Valori does not address this.

### 6. Volume of writes (DoS)

Valori does not implement write-rate limiting or admission control. A client
with a valid token can fill the record pool. Enforce quotas at the API gateway.

---

## Keyed BLAKE3 — MAC mode for writer authentication

### Current state

The BLAKE3 chain in `events.log` provides **integrity** (tampering is
detectable) but not **authenticity** (it does not prove who wrote an entry).
Any process with write access to the audit sink can append valid-looking entries.

### Recommendation

BLAKE3 supports a **keyed mode** (`blake3::keyed_hash(key, data)`) that
functions as a MAC (message authentication code). Enabling it would mean:

1. Each writer holds a 32-byte symmetric key.
2. Every `events.log` entry is signed with `BLAKE3-keyed(key, entry_bytes)`.
3. A verifier holding the key can confirm that entries were produced by a
   legitimate writer — not injected by a process that can write to the log file
   but does not hold the key.

### Trade-offs

| Aspect | Impact |
|---|---|
| **Performance** | Negligible — keyed BLAKE3 is as fast as plain BLAKE3 (single-pass). |
| **Key distribution** | Requires a secure key distribution mechanism (HashiCorp Vault, AWS KMS, k8s Secrets). |
| **Multi-node clusters** | All nodes that write audit entries must share the key, or use per-node keys with a key ID in each entry header. |
| **Backward compatibility** | Existing `events.log` files without a MAC cannot be verified retroactively. A migration flag or log rotation is needed. |
| **Threat it closes** | A rogue process with log-write access but no key cannot forge valid audit entries. |

### Implementation path

```rust
// In EventLogWriter::append():
let mac = blake3::keyed_hash(&self.key, &entry_bytes);
// Prepend or append mac.as_bytes() to each log entry.

// In the verifier (audit replay):
let expected_mac = blake3::keyed_hash(&key, &entry_bytes_without_mac);
assert_eq!(expected_mac.as_bytes(), &stored_mac);
```

**Status:** Not yet implemented. Tracked as a roadmap item. Operators with
strict writer-authentication requirements should run Valori behind an
authenticated write proxy until this is shipped.

---

## Defense in depth summary

| Layer | Current control | Gap |
|---|---|---|
| Data integrity | BLAKE3-chained audit log + snapshot hash | No MAC (writer not authenticated) |
| Replay protection | request_id dedup table, cluster-replicated | Evicts after 65 536 entries |
| Tenant isolation | Kernel-level namespace guard (3 enforcement points) | No per-tenant API keys |
| Transport | Plaintext HTTP (TLS at proxy) + optional Raft mTLS | Operator responsibility |
| Auth | Single bearer token | No RBAC |
| At-rest encryption | None | OS-level FDE required |
| Byzantine faults | Out of scope | CFT only (Raft) |
