# Phase 1.6 — Security Model: Threat Model, mTLS, Tenant Keys, Admin Audit

**Status:** design merged · implementation across Phases 2 and 3  
**Roadmap:** [MULTINODE_ROADMAP.md](../MULTINODE_ROADMAP.md) § 1.6  
**Why now:** The security model is not a feature that can be bolted on; it
determines network topology, key hierarchy, log structure, and API shape.
Decisions that land in Phase 1 (before production traffic) cost nothing to
get right; the same decisions in Phase 4 cost a migration.

---

## Goal

Produce a durable, implementation-grade security design that covers:

1. **Threat model** — what we protect, what we don't, and why.
2. **Inter-node mTLS** — cluster wire encryption and peer authentication.
3. **Per-tenant API keys and RBAC** — access control for the data plane.
4. **Admin-action audit events** — cluster membership changes and key
   rotations recorded in the hash-chained event log itself.
5. **Encryption at rest** — scope, mechanism, and Key Encryption Key (KEK)
   hierarchy.
6. **Schema field reservations** — fields that must land now so Phase 2/3
   implementations need no log migration.

---

## Threat Model

### Assets

| Asset | Sensitivity | Location |
|---|---|---|
| Vector embeddings + metadata | High — may contain PII | Record slots, WAL, snapshots |
| Knowledge graph structure | Medium — encodes relationships | Kernel state, WAL |
| Event log / audit trail | High — tamper evidence | `audit/events.log` (append-only) |
| API keys / tenant secrets | Critical | Key Vault (out of log) |
| DEKs (per-record encryption) | Critical | Key Vault (out of log) |
| Raft consensus traffic | High — commands become committed state | gRPC channels |
| Snapshot files | High — full state image | `snapshots/` dir, S3 |

### Threat Actors

| Actor | Capability | In-Scope? |
|---|---|---|
| **Network eavesdropper** (passive) | reads cluster traffic | ✅ mTLS defends |
| **Network attacker** (active, MITM) | injects or modifies Raft RPCs | ✅ mTLS + cert pinning defends |
| **Rogue cluster peer** | joins Raft group without authorization | ✅ mTLS client cert required |
| **Compromised API client** | valid API key, exceeds intended scope | ✅ RBAC defends |
| **Compromised node process** | can read disk files on that host | ⚠️ encryption-at-rest defends (partially) |
| **Compromised Key Vault** | DEKs and API keys exposed | ❌ out of scope — use hardware KMS |
| **Malicious operator with shell access** | can stop/start the process | ❌ trusted-operator assumption |
| **Insider threat (data exfiltration)** | reads from API with valid key | ✅ audit events + RBAC limit blast radius |
| **Log tampering** | rewrites historical WAL bytes | ✅ hash chain + verifier detects |
| **Snapshot substitution** | replaces snapshot with older one | ✅ chain splice (§1.2) detects |

### Non-Goals (Phase 1–3 scope)

- Multi-tenant kernel isolation (separate processes per tenant) — Phase 4.
- DDoS protection — handled by infrastructure layer (LB, WAF).
- Side-channel attacks (timing, cache) on crypto primitives — rely on
  constant-time AES-GCM from ring/aws-lc-rs.
- Zero-knowledge proofs over the event log — future research direction.

---

## Inter-Node mTLS (Phase 2 implementation)

### Channel requirement

All Raft RPC traffic (AppendEntries, RequestVote, InstallSnapshot) MUST be
carried over mutually authenticated TLS 1.3. A peer that cannot present a
valid certificate for this cluster is refused at the TLS handshake — it
never reaches the Raft layer.

### Certificate hierarchy

```
Cluster CA (offline, rotated ≥ annually)
  └── Node certificate (per node-id, rotated ≤ 90 days)
        SAN: node-id URI   e.g.  valori-node://cluster-id/node-42
```

- The Cluster CA is self-signed, generated at cluster init (`valori cluster
  init`), and stored only in the Key Vault (never on-disk in plaintext).
- Each node certificate is signed by the Cluster CA, pinned to the
  `cluster_id` (embedded in the URI SAN) so a certificate from a different
  cluster is rejected even if the CA is shared.
- `cluster_id` is a random UUID generated at `cluster init`, stored in
  the genesis segment header (a new field — see schema reservation below).

### TLS library

`rustls` with `aws-lc-rs` backend (FIPS-friendly, auditable, no OpenSSL
dependency). `tonic` provides the gRPC transport; rustls is plugged in via
`tonic::transport::Server::tls_config`.

### Leaf certificate format (on-disk storage)

Leaf certs and private keys are stored in PEM files inside `$DATA_DIR/certs/`
on each node, auto-rotated by `valori cluster rotate-certs`. This directory
MUST be covered by OS-level permissions (`chmod 700`) — the deployment
runbook enforces this.

### Revocation

Phase 2: short-lived certificates (90-day max) + manual rotation via
`valori cluster rotate-certs` (signed admin event in the log — see audit
section). Phase 3+: OCSP stapling or CA CRL if an enterprise customer
requires it.

### Schema reservation (`valori-wire`)

```rust
/// v3 header gains cluster_id in the 3 reserved bytes (bytes[9..12]).
/// Phase 2 uses bytes[9..12] for a u24 cluster_id truncation hint;
/// actual cluster_id is a 16-byte UUID stored in the genesis entry payload.
///
/// Reserved in Phase 1; written in Phase 2.
///
/// GenesisEntry (new LogEntry variant — append-only, Phase 2):
/// LogEntry::GenesisCluster {
///     cluster_id: [u8; 16],
///     node_count: u8,
///     created_at: u64,
/// }
```

---

## Per-Tenant API Keys and RBAC (Phase 3 implementation)

### Key structure

```
Master API Key (cluster-wide, admin-only)
  └── Tenant API Key (per tenant, created by admin)
        └── Scoped token (short-lived, created by tenant service)
```

An API key is a 32-byte random value encoded as base64url. On the wire it is
passed in the `Authorization: Bearer <key>` header (identical to the current
single-token auth — backward compatible).

### Role model (minimal, expandable)

| Role | Bit | Permitted operations |
|---|---|---|
| `READ` | 0x01 | search, get, list |
| `WRITE` | 0x02 | insert, upsert, delete |
| `GRAPH` | 0x04 | create/delete nodes and edges |
| `ERASE` | 0x08 | GDPR erasure (Phase 3 crypto-shredding) |
| `ADMIN` | 0xFF | all of the above + key management, cluster ops |

A tenant's API key has a bitmask of permitted roles. The engine checks the
mask before executing any command. `ERASE` is deliberately separated from
`WRITE` — a service account that writes data should not be able to silently
destroy it.

### Tenant isolation (Phase 3, single-kernel mode)

In Phase 3 the kernel is still a single shared state; tenants are isolated by
**collection** (Phase 1.4 seam). A tenant API key is bound to one or more
allowed collections. Cross-collection access is rejected before the command
reaches the engine. True process-level isolation is Phase 4.

### Key storage

Tenant API keys are stored in the Key Vault (same vault as DEKs — see
Phase 1.5). The key map is: `key_hash(api_key) → {tenant_id, role_mask,
allowed_collections, created_at, expires_at}`. The API key itself is never
stored — only its BLAKE3 hash. Verification: BLAKE3-hash the incoming key,
look up in the vault.

### Schema reservation (`valori-wire` — Phase 3 variant)

```rust
/// Reserved LogEntry variants (append-only, Phase 3):
/// LogEntry::CreateTenantKey {
///     tenant_id: [u8; 16],
///     key_hash:  [u8; 32],
///     role_mask: u8,
///     collections: Vec<String>,
///     created_by: [u8; 16],  // admin key_hash
///     expires_at: u64,
/// }
/// LogEntry::RevokeTenantKey {
///     key_hash:   [u8; 32],
///     revoked_by: [u8; 16],
///     reason:     String,
/// }
```

Both events are recorded in the hash-chained audit log — key creation and
revocation are permanently auditable facts.

### HTTP header reservation

`X-Valori-Tenant-Id` is reserved now. Phase 3 populates it; Phase 1–2
servers ignore it (no auth regression). Clients may start sending it.

---

## Admin-Action Audit Events (Phase 2/3 implementation)

The principle: **any action that changes cluster membership, key material, or
access control policy is a first-class log event, chained into the same
BLAKE3 hash chain as data operations.** This means the audit trail cannot be
separated from the data history — you can't rotate a key without it appearing
in the chain.

### Audited admin actions

| Action | Log event | Triggered by |
|---|---|---|
| Node added to cluster | `AdminEvent::NodeJoined { node_id, cert_fingerprint }` | Phase 2 |
| Node removed from cluster | `AdminEvent::NodeLeft  { node_id, reason }` | Phase 2 |
| Node cert rotated | `AdminEvent::CertRotated { node_id, old_fp, new_fp }` | Phase 2 |
| DEK destroyed (GDPR erase) | `AdminEvent::EraseRecord { record_id, erased_by }` | Phase 3 |
| Tenant key created | `AdminEvent::TenantKeyCreated { … }` | Phase 3 |
| Tenant key revoked | `AdminEvent::TenantKeyRevoked { … }` | Phase 3 |
| Cluster CA rotated | `AdminEvent::ClusterCaRotated { old_ca_fp, new_ca_fp }` | Phase 3 |
| Format migration | `AdminEvent::FormatMigration { from, to, state_hash }` | Phase 4 |

### `AdminEvent` in the wire format

```rust
/// New LogEntry variant — append-only, Phase 2:
/// LogEntry::AdminEvent(AdminEvent)
///
/// AdminEvent is a separate enum to keep LogEntry arms legible.
/// All variants carry `authorized_by: [u8; 16]` (admin key hash or
/// node-cert fingerprint) for accountability.
#[repr(u8)]
pub enum AdminEvent {
    NodeJoined      { node_id: u32, cert_fingerprint: [u8; 32], authorized_by: [u8; 16] },
    NodeLeft        { node_id: u32, reason: String,              authorized_by: [u8; 16] },
    CertRotated     { node_id: u32, old_fp: [u8; 32], new_fp: [u8; 32], authorized_by: [u8; 16] },
    EraseRecord     { record_id: u64, erased_by: [u8; 16] },
    TenantKeyCreated { tenant_id: [u8; 16], key_hash: [u8; 32], role_mask: u8, authorized_by: [u8; 16] },
    TenantKeyRevoked { key_hash: [u8; 32], authorized_by: [u8; 16] },
    ClusterCaRotated { old_ca_fp: [u8; 32], new_ca_fp: [u8; 32], authorized_by: [u8; 16] },
}
```

> **Immutability guarantee:** `authorized_by` is the BLAKE3 hash of the
> admin's API key or the node's TLS cert at the time of the action. Rotating
> a key does not change the historical attribution — the audit record stands.

---

## Encryption at Rest (Phase 3 implementation)

### Scope

| Data | Encrypted at rest? | Mechanism |
|---|---|---|
| Record metadata / user payload | Yes (per-record DEK, Phase 1.5) | AES-256-GCM |
| Vector data (FXP scalars) | No — vectors are derived data, not PII | — |
| WAL segment files | Optional (volume-level) | OS disk encryption (e.g. LUKS, FileVault) |
| Snapshot files | Yes (same DEKs apply — shredded records remain shredded in snapshots) | AES-256-GCM |
| Raft log files | No (contains committed KernelEvents, not raw payloads) | volume-level |
| Key Vault | Yes (Hardware KMS backend in production) | HSM or cloud KMS |

### Key Encryption Key (KEK) hierarchy (Phase 3)

```
Root KEK (HSM-backed, per-cluster)
  └── Tenant KEK (per-tenant, derived or randomly generated, wrapped by Root KEK)
        └── Data DEK (per-record, randomly generated, wrapped by Tenant KEK)
```

In Phase 3 the "Tenant KEK" is the tenant's API-key-derived key (HKDF-SHA256
over the tenant's master secret). Phase 4 adds true per-tenant HSM partitions.

Unwrap path: `incoming request with tenant API key → derive Tenant KEK →
unwrap record DEK → decrypt payload`. No plaintext key material exists on
disk at any point; all key material lives in the Key Vault.

---

## Schema Fields Reserved in This Phase

All of the following are **declared now but implemented in Phase 2 or 3**.
They are either constants, comment-level reservations, or `Option<…>` fields
with `#[serde(default)]` — so existing code paths decode them as `None` / 0.

| Location | Field / constant | Reserved for | Implemented |
|---|---|---|---|
| `valori-wire` `LogEntry` | `AdminEvent(AdminEvent)` variant | Admin audit events | Phase 2 |
| `valori-wire` `LogEntry` | `GenesisCluster { … }` variant | mTLS cluster init | Phase 2 |
| `valori-wire` `EntryV3` | `cluster_id: Option<[u8;16]>` | Cluster identity in entries | Phase 2 |
| `valori-wire` `LogEntry` | `CreateTenantKey`, `RevokeTenantKey` | RBAC key lifecycle | Phase 3 |
| `storage/record.rs` | `FLAG_ENCRYPTED = 0x02`, `FLAG_SHREDDED = 0x04` | Crypto-shredding (Phase 1.5) | Phase 3 |
| HTTP headers | `X-Valori-Tenant-Id` | Tenant routing | Phase 3 |
| `KernelState` | `audit_seq: u64` monotone counter | Admin event dedup | Phase 2 |

> Note: `FLAG_ENCRYPTED` and `FLAG_SHREDDED` are also the Phase 1.5
> reservation — listed here for cross-reference completeness.

---

## Implementation Sequence (cross-phase)

```
Phase 1 (now):
  ✅ This design doc
  ✅ FLAG_ENCRYPTED, FLAG_SHREDDED constants (Phase 1.5 overlap)
  ✅ Reserved variant / field names in comments (this file as the contract)

Phase 2 (cluster mode):
  ☐ mTLS: rustls + tonic TLS config in valori-consensus
  ☐ Cluster CA + node cert generation in valori-cli (cluster init / rotate-certs)
  ☐ GenesisCluster log event (first entry of every new cluster)
  ☐ NodeJoined / NodeLeft / CertRotated admin events in Raft membership changes
  ☐ audit_seq in KernelState

Phase 3 (GA):
  ☐ Per-tenant API keys + role bitmask enforcement in valori-node API layer
  ☐ CreateTenantKey / RevokeTenantKey log events
  ☐ X-Valori-Tenant-Id routing + collection binding
  ☐ EraseRecord admin event (ties to Phase 1.5 DEK destruction)
  ☐ ClusterCaRotated event + CA rotation procedure in valori-cli
  ☐ KEK hierarchy + Tenant KEK derivation in Key Vault implementation
  ☐ Deployment runbook: cert rotation, key vault backup purge, RBAC setup
```

---

## Findings

Design-only phase — no runtime findings. Two forward concerns noted:

**Audit log and Raft log divergence:** the audit log (`audit/events.log`)
is written at APPLY time (after Raft commits). This means admin events appear
in the audit log only after quorum — a `NodeJoined` admin event is written by
the leader when it applies the Raft membership change. Followers also apply
the change and write the same event. The audit log is NOT consensus-replicated
(each node writes its own copy). Therefore `valori-verify` must treat all
nodes' audit logs as independently verifiable chains for the same logical
sequence, not as identical byte copies. Document this in the verifier runbook.

**First-write-wins admin events:** if a node crashes between the Raft commit
and the audit log write, the admin event may be missing from that node's
audit copy. Recovery: on next apply of the same Raft index (idempotent via
`request_id`), the audit event is re-written. This requires Phase 2's
`request_id` dedup (Phase 1.2) to also apply to admin events.

## Follow-ups

- Phase 2: `valori cluster` subcommand (init, add-node, remove-node,
  rotate-certs) + corresponding audit events.
- Phase 2: mTLS integration test — reject peer with wrong cluster_id cert.
- Phase 3: RBAC enforcement unit tests (role mask logic).
- Phase 3: Red-team exercise — external contractor given access to a test
  cluster, asked to escalate privileges or exfiltrate data without audit trace.
- Phase 3: SOC 2 Type II controls mapping — each control maps to a specific
  audit event or mechanism in this document.
- Phase 4: Per-tenant process isolation (separate kernel instance per tenant).
