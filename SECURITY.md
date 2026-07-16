# Security Policy

---

## Reporting Vulnerabilities

Report security issues privately rather than opening a public GitHub issue.

**Contact:** varshith.gudur17@gmail.com

Include:
- Reproduction steps (minimal code or curl commands)
- Hardware and architecture details
- Input data characteristics
- Whether the issue is determinism-breaking, integrity-breaking, or confidentiality-breaking

We treat all of the following with the same severity as traditional security bugs:
- Determinism failures (two machines apply the same events, produce different state hashes)
- Audit chain breaks (tampered log not detected by `valori-verify`)
- Namespace cross-contamination (tenant A's data appears in tenant B's search results)
- Authentication bypass

**Supported versions:** Only the latest tagged release receives security fixes.

---

## Cryptographic Primitives

### BLAKE3 Audit Chain

Every mutation committed to `events.log` is chained:

```
entry_hash = BLAKE3(prev_hash ‖ event_bytes ‖ crc32)
```

The running `state_hash` in `KernelState` is the BLAKE3 Merkle root over all applied events in order. This hash is:
- Recomputed from scratch after every snapshot restore (no "trust the saved hash" shortcut)
- Surfaced at `/v1/proof` and in every `ClientResponse`
- Independently reproducible from the raw `events.log` using `valori-verify`

**What this detects:** Any insertion, deletion, or reorder of log entries breaks the chain. Modifying an event payload breaks the entry hash. The detection is purely mathematical — no trusted third party is required.

**What this does NOT prevent:** A privileged attacker with write access to both `events.log` and the kernel state could atomically replace both, recomputing the chain. Tamper-evidence is not tamper-prevention. For a genuine security boundary, restrict filesystem access to the node process.

### BLAKE3 Retrieval Receipts

`memory_recall` and Tree-RAG queries return a `receipt`:

```
receipt_digest = BLAKE3(canonical_json(response_body))
```

binding the exact result set to `state_hash`, `event_log_hash`, and `committed_height` at recall time. A client can independently recompute this digest from the response to verify the server did not alter the result.

### Per-Entry CRC32 (V4 Wire Format)

Each log entry in `events.log` carries a 4-byte CRC32 header. `valori-verify` checks both the CRC32 (detects random corruption) and the BLAKE3 chain (detects adversarial modification). V2/V3 entries lack per-entry CRC32 but retain BLAKE3 chaining.

### Crypto-Shredding (GDPR delete path)

When a record is hard-deleted with crypto-shredding enabled, the record's vector data is encrypted under a per-record key stored in `valori-metadata` (redb). The key is then zeroed and deleted. Subsequent reads of the record slot return ciphertext that is undecryptable without the key. The deletion itself is appended to the audit chain (proving shredding happened without exposing the data).

This provides a GDPR-compliant "right to erasure" path while preserving the integrity of the audit chain up to the deletion event.

---

## Authentication

Valori uses a single shared bearer token (`VALORI_AUTH_TOKEN`). If the env var is set, every HTTP request must include:

```
Authorization: Bearer <token>
```

Requests without a valid token receive `401 Unauthorized`. The token is compared with constant-time equality to prevent timing side-channels.

**What this provides:** Protects the data plane from unauthenticated access on a private network.

**What this does NOT provide:** Multi-tenant access control (all authenticated clients share a single namespace hierarchy), per-endpoint authorization, or audit of which client performed which operation. For multi-tenant isolation, enforce namespace-per-tenant at the application layer and restrict each client to its assigned namespace.

---

## Transport Security

**Standalone mode:** HTTP only. For production deployments, place the node behind a TLS-terminating reverse proxy (nginx, Caddy, Cloudflare Tunnel) or VPN.

**Cluster mode (Raft channel):** Mutual TLS is supported and recommended:

| Env var | Purpose |
|---|---|
| `VALORI_TLS_CA` | CA certificate (PEM) for verifying peers |
| `VALORI_TLS_CERT` | Node's TLS certificate (PEM) |
| `VALORI_TLS_KEY` | Node's TLS private key (PEM) |

Without mTLS, any process that can reach the Raft port can submit log entries. In cluster deployments, restrict the Raft port (`VALORI_RAFT_BIND`, default `:3100`) to the cluster's private network.

---

## Threat Categories

### In Scope

| Threat | Mitigation |
|---|---|
| State drift between replicas | Q16.16 determinism + BLAKE3 state hash + convergence watcher |
| Tampered audit log | BLAKE3-chained entries; `valori-verify` detects any modification |
| Snapshot corruption | BLAKE3 state hash verified on restore |
| Duplicate command replay | `request_id` (UUID) dedup table, 65k entries, LRU, travels in snapshots |
| Unauthorized writes in cluster | Followers return HTTP 307 to leader; followers cannot commit |
| Namespace cross-contamination | Isolation at three independent points: event-commit, WAL replay, `build_index()` |
| Embedding model output logged as LLM replay bait | Determinism = replay the logged output; LLM is never re-invoked |

### Out of Scope (operator responsibility)

| Threat | Notes |
|---|---|
| Privileged OS-level attacker | Root access can overwrite any file; use full-disk encryption |
| Compromised leader node | Crash-fault tolerant only, not Byzantine; use BFT consensus for adversarial environments |
| Transport confidentiality (standalone) | Use TLS proxy; Valori standalone is plaintext HTTP |
| Key management for crypto-shredding | Per-record keys live in redb on disk; protect the redb file with filesystem permissions |
| Denial of service | No rate limiting; place a reverse proxy in front |
| Supply chain attacks on dependencies | Audit `Cargo.lock`; pin dependency versions in production |

---

## Data at Rest

Valori does not encrypt data at rest by default. Sensitive deployments should:

1. Use full-disk encryption (LUKS, FileVault, BitLocker) for the volumes holding `events.log`, snapshot files, and the redb metadata database.
2. Use crypto-shredding for individual record deletion where GDPR compliance is required.
3. Restrict filesystem permissions on the WAL and snapshot paths to the node process user.

---

## Determinism as a Security Property

Because Valori is used in incident reconstruction, audit trails, and compliance pipelines, nondeterministic behavior can affect investigative outcomes. We treat determinism failures with the same severity as integrity bugs.

The determinism guarantee: **any machine that has the raw `events.log` can reproduce the exact BLAKE3 state hash that the server reports — using only integer arithmetic, with no floating-point dependency.** This is verifiable by running `valori-verify` against the log file.

---

## Dependency Notes

Key security-relevant dependencies:

| Crate | Version (see Cargo.lock) | Purpose |
|---|---|---|
| `blake3` | latest | Audit chain hashing |
| `openraft` | 0.9 | Consensus; Raft log integrity |
| `redb` | latest | Metadata persistence (B-tree, single-writer) |
| `axum` | latest | HTTP server; TLS termination is upstream |
| `pyo3` | latest | Python FFI; memory safety via Rust ownership |

No cryptographic keys are stored in source code. No hardcoded secrets. The only secret value in normal deployments is `VALORI_AUTH_TOKEN`, which lives in the environment.
